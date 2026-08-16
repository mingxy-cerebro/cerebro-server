pub mod prompts;

use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;

use crate::api::event_bus::{ServerEvent, SharedEventBus};
use crate::domain::error::OmemError;
use crate::llm::{complete_json, LlmService};

/// 任务级超时:LLM read 90s × 内部重试 3 ≈ 270s,complete_json 最多两轮调用 ≈ 540s,600s 封顶(SPEC.md ADR-3)。
pub const DREAM_TIMEOUT: Duration = Duration::from_secs(600);
/// 单请求 sessions 上限。
pub const MAX_SESSIONS: usize = 50;
/// memory + sessions 合计字节上限(P1-2:防超大 prompt 占满任务槽烧 600s 配额)。
pub const MAX_PAYLOAD_BYTES: usize = 512 * 1024;

/// 终态 job 保留时长,过期自动清理。
const JOB_TTL: Duration = Duration::from_secs(3600);
/// 内存表硬顶,防无界增长(SPEC.md ADR-1)。
const MAX_JOBS: usize = 128;
/// 排队上限(pending + running),超出 429(SPEC.md ADR-4)。
const MAX_ACTIVE: usize = 8;

#[derive(Debug, Clone, Deserialize)]
pub struct DreamRequest {
    /// 记忆档全文(自由文本,通常为带 frontmatter 的 Markdown)
    pub memory: String,
    /// 待消化的 session 文本
    pub sessions: Vec<String>,
    /// RFC3339 时间戳,食材时间下界(元数据,原样回显,不参与推理)
    pub since: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntrySource {
    Merged,
    Updated,
    Added,
    Kept,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryType {
    User,
    Feedback,
    Project,
    Reference,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DreamEntry {
    pub name: String,
    pub description: String,
    #[serde(rename = "type")]
    pub entry_type: EntryType,
    pub body: String,
    #[serde(default)]
    pub links: Vec<String>,
    pub source: EntrySource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DreamStats {
    pub merged: usize,
    pub updated: usize,
    pub added: usize,
    pub dropped: usize,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DreamResult {
    pub entries: Vec<DreamEntry>,
    pub stats: DreamStats,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DreamStatus {
    Pending,
    Running,
    Completed,
    Failed,
}

impl DreamStatus {
    fn is_terminal(self) -> bool {
        matches!(self, DreamStatus::Completed | DreamStatus::Failed)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DreamJob {
    pub id: String,
    pub tenant_id: String,
    pub status: DreamStatus,
    pub result: Option<DreamResult>,
    pub error: Option<String>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

/// 校验食材 payload(SPEC.md §5.1)。
pub fn validate_request(req: &DreamRequest) -> Result<(), OmemError> {
    if req.memory.trim().is_empty() {
        return Err(OmemError::Validation("memory must not be empty".into()));
    }
    if req.sessions.is_empty() {
        return Err(OmemError::Validation("sessions must not be empty".into()));
    }
    if req.sessions.len() > MAX_SESSIONS {
        return Err(OmemError::Validation(format!(
            "sessions exceeds limit: {} (max {MAX_SESSIONS})",
            req.sessions.len()
        )));
    }
    let payload_bytes = req.memory.len() + req.sessions.iter().map(|s| s.len()).sum::<usize>();
    if payload_bytes > MAX_PAYLOAD_BYTES {
        return Err(OmemError::Validation(format!(
            "memory + sessions too large: {payload_bytes} bytes (max {MAX_PAYLOAD_BYTES})"
        )));
    }
    if let Some(since) = &req.since {
        chrono::DateTime::parse_from_rfc3339(since)
            .map_err(|e| OmemError::Validation(format!("since must be RFC3339: {e}")))?;
    }
    Ok(())
}

/// Dream 引擎:纯函数,唯一外部依赖是 llm 参数。
/// 输入只读、无任何存储副作用(SPEC.md 铁律)。
pub async fn run_dream(llm: &dyn LlmService, req: &DreamRequest) -> Result<DreamResult, OmemError> {
    let (system, user) = prompts::build_dream_prompt(req);
    let mut result: DreamResult = complete_json(llm, &system, &user)
        .await
        .map_err(|e| OmemError::Llm(format!("dream engine failed: {e}")))?;
    reconcile_stats(&mut result);
    Ok(result)
}

/// LLM 上报的 stats 不自洽时按 entries 重算;dropped 无法从 entries 反推,保留 LLM 判断。
fn reconcile_stats(result: &mut DreamResult) {
    result.stats = DreamStats {
        merged: result
            .entries
            .iter()
            .filter(|e| e.source == EntrySource::Merged)
            .count(),
        updated: result
            .entries
            .iter()
            .filter(|e| e.source == EntrySource::Updated)
            .count(),
        added: result
            .entries
            .iter()
            .filter(|e| e.source == EntrySource::Added)
            .count(),
        dropped: result.stats.dropped,
        total: result.entries.len(),
    };
}

/// 内存 job 表:承载 DreamJob 生命周期,不持久化(ADR-1)。
/// 串行闸 Semaphore(1) 内置(ADR-4):单 LLM 通道,并行无益。
pub struct DreamJobStore {
    jobs: DashMap<String, DreamJob>,
    lane: Arc<Semaphore>,
}

impl Default for DreamJobStore {
    fn default() -> Self {
        Self::new()
    }
}

impl DreamJobStore {
    pub fn new() -> Self {
        Self {
            jobs: DashMap::new(),
            lane: Arc::new(Semaphore::new(1)),
        }
    }

    /// 按 id + tenant 取 job;不匹配返回 None(handler 层转 404,不泄露存在性,D-2)。
    pub fn get(&self, id: &str, tenant_id: &str) -> Option<DreamJob> {
        self.jobs.get(id).filter(|j| j.tenant_id == tenant_id).map(|j| j.clone())
    }

    fn active_count(&self) -> usize {
        self.jobs
            .iter()
            .filter(|j| !j.status.is_terminal())
            .count()
    }

    fn evict_oldest_finished(&self) {
        let victim = self
            .jobs
            .iter()
            .filter(|j| j.status.is_terminal())
            .min_by_key(|j| j.completed_at.clone().unwrap_or_default())
            .map(|j| j.id.clone());
        if let Some(id) = victim {
            self.jobs.remove(&id);
        }
    }

    /// 受理一个 dream job:校验外部已做,这里管排队上限 + 入表 + 后台执行。
    pub fn spawn(
        self: &Arc<Self>,
        llm: Option<Arc<dyn LlmService>>,
        req: DreamRequest,
        tenant_id: String,
        event_bus: SharedEventBus,
    ) -> Result<DreamJob, OmemError> {
        let llm = llm.ok_or_else(|| {
            OmemError::Validation("dream requires dream LLM (set OMEM_DREAM_LLM_*)".into())
        })?;

        if self.active_count() >= MAX_ACTIVE {
            return Err(OmemError::RateLimited);
        }
        if self.jobs.len() >= MAX_JOBS {
            self.evict_oldest_finished();
        }

        let job = DreamJob {
            id: uuid::Uuid::new_v4().to_string(),
            tenant_id: tenant_id.clone(),
            status: DreamStatus::Pending,
            result: None,
            error: None,
            created_at: chrono::Utc::now().to_rfc3339(),
            started_at: None,
            completed_at: None,
        };
        self.jobs.insert(job.id.clone(), job.clone());

        let store = self.clone();
        let job_id = job.id.clone();
        tokio::spawn(async move {
            store.run(job_id, llm, req, tenant_id, event_bus).await;
        });
        Ok(job)
    }

    async fn run(
        self: Arc<Self>,
        id: String,
        llm: Arc<dyn LlmService>,
        req: DreamRequest,
        tenant_id: String,
        event_bus: SharedEventBus,
    ) {
        let _permit = self.lane.clone().acquire_owned().await;

        if let Some(mut j) = self.jobs.get_mut(&id) {
            j.status = DreamStatus::Running;
            j.started_at = Some(chrono::Utc::now().to_rfc3339());
        }

        let outcome = tokio::time::timeout(DREAM_TIMEOUT, run_dream(llm.as_ref(), &req)).await;
        let (result, error) = match outcome {
            Ok(Ok(r)) => (Some(r), None),
            Ok(Err(e)) => (None, Some(e.to_string())),
            Err(_) => (None, Some("dream engine timeout".into())),
        };

        let stats = result.as_ref().map(|r| r.stats.clone());
        if let Some(mut j) = self.jobs.get_mut(&id) {
            j.status = if error.is_some() {
                DreamStatus::Failed
            } else {
                DreamStatus::Completed
            };
            j.result = result;
            j.error = error;
            j.completed_at = Some(chrono::Utc::now().to_rfc3339());
        }

        // SSE 钩子:仅 completed 发事件,failed 一期无消费方不发(SPEC.md §5.3)
        if let Some(stats) = stats {
            event_bus.publish(ServerEvent {
                event_type: "dream.completed".to_string(),
                tenant_id,
                data: Some(serde_json::json!({"job_id": id, "stats": stats})),
                timestamp: chrono::Utc::now().to_rfc3339(),
            });
        }

        // TTL 清理:终态保留 1h 后移除
        let cleanup = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(JOB_TTL).await;
            cleanup.jobs.remove(&id);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeLlm {
        response: String,
        fail: bool,
    }

    #[async_trait::async_trait]
    impl LlmService for FakeLlm {
        async fn complete_text(&self, _system: &str, _user: &str) -> Result<String, OmemError> {
            if self.fail {
                return Err(OmemError::Llm("upstream down".into()));
            }
            Ok(self.response.clone())
        }
    }

    fn ok_llm() -> Arc<dyn LlmService> {
        Arc::new(FakeLlm {
            // stats 故意不自洽(merged=9),验证 reconcile_stats 兜底
            response: r#"{"entries":[
                {"name":"a","description":"d","type":"user","body":"b","links":[],"source":"kept"},
                {"name":"b","description":"d","type":"feedback","body":"b","links":[],"source":"merged"},
                {"name":"c","description":"d","type":"project","body":"b","links":["a"],"source":"added"}
            ],"stats":{"merged":9,"updated":9,"added":9,"dropped":2,"total":99}}"#.to_string(),
            fail: false,
        })
    }

    fn sample_req() -> DreamRequest {
        DreamRequest {
            memory: "---\ntype: user\n---\n# 记忆档".to_string(),
            sessions: vec!["session1".to_string()],
            since: Some("2026-08-15T00:00:00Z".to_string()),
        }
    }

    // ── validate_request ─────────────────────────────────────────

    #[test]
    fn validate_accepts_valid_request() {
        assert!(validate_request(&sample_req()).is_ok());
    }

    #[test]
    fn validate_rejects_empty_memory() {
        let mut req = sample_req();
        req.memory = "   ".to_string();
        let err = validate_request(&req).unwrap_err();
        assert!(err.to_string().contains("memory"));
    }

    #[test]
    fn validate_rejects_empty_sessions() {
        let mut req = sample_req();
        req.sessions = vec![];
        let err = validate_request(&req).unwrap_err();
        assert!(err.to_string().contains("sessions"));
    }

    #[test]
    fn validate_rejects_too_many_sessions() {
        let mut req = sample_req();
        req.sessions = vec!["s".to_string(); MAX_SESSIONS + 1];
        let err = validate_request(&req).unwrap_err();
        assert!(err.to_string().contains("limit"));
    }

    #[test]
    fn validate_rejects_oversized_payload() {
        let mut req = sample_req();
        req.memory = "x".repeat(MAX_PAYLOAD_BYTES + 1);
        let err = validate_request(&req).unwrap_err();
        assert!(err.to_string().contains("too large"));
    }

    #[test]
    fn validate_rejects_bad_since() {
        let mut req = sample_req();
        req.since = Some("not-a-date".to_string());
        let err = validate_request(&req).unwrap_err();
        assert!(err.to_string().contains("RFC3339"));
    }

    #[test]
    fn validate_allows_missing_since() {
        let mut req = sample_req();
        req.since = None;
        assert!(validate_request(&req).is_ok());
    }

    // ── run_dream ────────────────────────────────────────────────

    #[tokio::test]
    async fn run_dream_reconciles_stats() {
        let result = run_dream(ok_llm().as_ref(), &sample_req()).await.unwrap();
        assert_eq!(result.stats.total, 3);
        assert_eq!(result.stats.merged, 1);
        assert_eq!(result.stats.added, 1);
        assert_eq!(result.stats.dropped, 2); // 无法反推,保留 LLM 判断
        assert_eq!(result.entries[2].entry_type, EntryType::Project);
        assert_eq!(result.entries[2].links, vec!["a".to_string()]);
    }

    #[tokio::test]
    async fn run_dream_propagates_llm_failure() {
        let llm: Arc<dyn LlmService> = Arc::new(FakeLlm {
            response: String::new(),
            fail: true,
        });
        let err = run_dream(llm.as_ref(), &sample_req()).await.unwrap_err();
        assert!(matches!(err, OmemError::Llm(_)));
    }

    #[tokio::test]
    async fn run_dream_rejects_garbage_output() {
        let llm: Arc<dyn LlmService> = Arc::new(FakeLlm {
            response: "不是JSON".to_string(),
            fail: false,
        });
        let err = run_dream(llm.as_ref(), &sample_req()).await.unwrap_err();
        assert!(matches!(err, OmemError::Llm(_)));
    }

    // ── DreamJobStore ────────────────────────────────────────────

    async fn wait_terminal(store: &DreamJobStore, id: &str) -> DreamJob {
        for _ in 0..500 {
            if let Some(j) = store.get(id, "t1") {
                if j.status.is_terminal() {
                    return j;
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("job never reached terminal state");
    }

    #[tokio::test]
    async fn job_lifecycle_completes_with_result() {
        let store = Arc::new(DreamJobStore::new());
        let bus: SharedEventBus = Arc::new(crate::api::event_bus::EventBus::new());

        let job = store
            .spawn(Some(ok_llm()), sample_req(), "t1".into(), bus)
            
            .unwrap();
        assert_eq!(job.status, DreamStatus::Pending);

        let done = wait_terminal(&store, &job.id).await;
        assert_eq!(done.status, DreamStatus::Completed);
        let result = done.result.expect("result present");
        assert_eq!(result.stats.total, 3);
        assert!(done.error.is_none());
        assert!(done.started_at.is_some());
        assert!(done.completed_at.is_some());
    }

    #[tokio::test]
    async fn job_fails_on_llm_error() {
        let store = Arc::new(DreamJobStore::new());
        let bus: SharedEventBus = Arc::new(crate::api::event_bus::EventBus::new());
        let llm: Arc<dyn LlmService> = Arc::new(FakeLlm {
            response: String::new(),
            fail: true,
        });

        let job = store.spawn(Some(llm), sample_req(), "t1".into(), bus).unwrap();
        let done = wait_terminal(&store, &job.id).await;
        assert_eq!(done.status, DreamStatus::Failed);
        assert!(done.error.is_some());
        assert!(done.result.is_none());
    }

    #[tokio::test]
    async fn spawn_rejects_missing_llm() {
        let store = Arc::new(DreamJobStore::new());
        let bus: SharedEventBus = Arc::new(crate::api::event_bus::EventBus::new());
        let err = store.spawn(None, sample_req(), "t1".into(), bus).unwrap_err();
        assert!(matches!(err, OmemError::Validation(_)));
    }

    #[tokio::test]
    async fn get_is_tenant_scoped() {
        let store = Arc::new(DreamJobStore::new());
        let bus: SharedEventBus = Arc::new(crate::api::event_bus::EventBus::new());

        let job = store.spawn(Some(ok_llm()), sample_req(), "t1".into(), bus).unwrap();
        assert!(store.get(&job.id, "t1").is_some());
        assert!(store.get(&job.id, "someone-else").is_none()); // D-2:归属不符当不存在
        assert!(store.get("no-such-id", "t1").is_none());
    }

    #[tokio::test]
    async fn spawn_rejects_when_queue_full() {
        let store = Arc::new(DreamJobStore::new());
        let bus: SharedEventBus = Arc::new(crate::api::event_bus::EventBus::new());

        // 卡死 lane 的 llm:第一次调用永久挂起,job 停在 running
        struct StalledLlm;
        #[async_trait::async_trait]
        impl LlmService for StalledLlm {
            async fn complete_text(&self, _: &str, _: &str) -> Result<String, OmemError> {
                futures::future::pending::<()>().await;
                unreachable!()
            }
        }

        let _first = store
            .spawn(
                Some(Arc::new(StalledLlm) as Arc<dyn LlmService>),
                sample_req(),
                "t1".into(),
                bus.clone(),
            )
            
            .unwrap();

        // 灌满到 MAX_ACTIVE
        for _ in 0..(MAX_ACTIVE - 1) {
            store
                .spawn(
                    Some(Arc::new(StalledLlm) as Arc<dyn LlmService>),
                    sample_req(),
                    "t1".into(),
                    bus.clone(),
                )
                
                .unwrap();
        }
        let err = store
            .spawn(Some(ok_llm()), sample_req(), "t1".into(), bus)
            
            .unwrap_err();
        assert!(matches!(err, OmemError::RateLimited));
    }
}
