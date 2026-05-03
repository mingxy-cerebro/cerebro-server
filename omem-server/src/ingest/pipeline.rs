use std::sync::Arc;
use std::sync::OnceLock;

use regex::RegexSet;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::domain::category::Category;
use crate::domain::error::OmemError;
use crate::embed::EmbedService;
use crate::ingest::admission::AdmissionControl;
use crate::ingest::extractor::FactExtractor;
use crate::ingest::noise::NoiseFilter;
use crate::ingest::privacy::{is_fully_private, strip_private_content};
use crate::cluster::assigner::ClusterAssigner;
use crate::cluster::cluster_store::ClusterStore;
use crate::cluster::manager::ClusterManager;
use crate::ingest::reconciler::Reconciler;
use crate::ingest::session::{SessionMessage, SessionStore};
use crate::ingest::types::{IngestMessage, IngestMode, IngestRequest, IngestResponse};
use crate::llm::LlmService;
use crate::store::LanceStore;

const BYTE_BUDGET: usize = 200_000;
const MESSAGE_BUDGET: usize = 20;

pub struct IngestPipeline {
    extractor: Arc<FactExtractor>,
    reconciler: Arc<Reconciler>,
    cluster_assigner: Arc<ClusterAssigner>,
    cluster_manager: Arc<ClusterManager>,
    store: Arc<LanceStore>,
    session_store: Arc<SessionStore>,
    noise_filter: Arc<tokio::sync::Mutex<NoiseFilter>>,
    admission: Arc<AdmissionControl>,
    embed: Arc<dyn EmbedService>,
    ingest_semaphore: Option<Arc<tokio::sync::Semaphore>>,
}

impl IngestPipeline {
    pub async fn new(
        store: Arc<LanceStore>,
        session_store: Arc<SessionStore>,
        embed: Arc<dyn EmbedService>,
        llm: Arc<dyn LlmService>,
        cluster_store: Arc<ClusterStore>,
        admission_preset: &str,
        admission_reject_threshold: Option<f32>,
        admission_admit_threshold: Option<f32>,
    ) -> Result<Self, OmemError> {
        let extractor = Arc::new(FactExtractor::new(llm.clone()));
        let reconciler = Arc::new(Reconciler::new(llm.clone(), store.clone(), embed.clone()));
        let cluster_manager = Arc::new(ClusterManager::new(cluster_store.clone(), Some(llm.clone())));
        let cluster_assigner = Arc::new(ClusterAssigner::new(cluster_store, embed.clone()).with_llm(llm.clone()));
        let noise_filter = Arc::new(tokio::sync::Mutex::new(NoiseFilter::new(Vec::new())));
        let admission = Arc::new(
            AdmissionControl::from_preset_str(admission_preset, embed.clone(), store.clone())
                .with_custom_thresholds(admission_reject_threshold, admission_admit_threshold)
        );
        Ok(Self {
            extractor,
            reconciler,
            cluster_assigner,
            cluster_manager,
            store,
            session_store,
            noise_filter,
            admission,
            embed,
            ingest_semaphore: None,
        })
    }

    pub fn with_ingest_semaphore(mut self, sem: Arc<tokio::sync::Semaphore>) -> Self {
        self.ingest_semaphore = Some(sem);
        self
    }

    pub async fn ingest(&self, request: IngestRequest) -> Result<IngestResponse, OmemError> {
        if request.messages.is_empty() {
            return Err(OmemError::Validation("no messages provided".to_string()));
        }

        let task_id = Uuid::new_v4().to_string();
        let session_id = request
            .session_id
            .clone()
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let agent_id = request.agent_id.clone().filter(|s| !s.is_empty());
        let agent_id_str = agent_id.clone().unwrap_or_else(|| "unknown".to_string());

        let session_messages: Vec<SessionMessage> = request
            .messages
            .iter()
            .map(|m| SessionMessage::new(&session_id, &agent_id_str, &m.role, &m.content, vec![]))
            .collect();

        let stored_count = self.session_store.bulk_create(&session_messages).await?;

        if matches!(request.mode, IngestMode::Raw) {
            return Ok(IngestResponse {
                task_id,
                stored_count,
            });
        }

        let selected = select_messages(&request.messages);

        // Layer 1: Pre-filter meta-operation messages before LLM extraction.
        // Private content (marked with <private>) is preserved for quality evaluation.
        let selected: Vec<IngestMessage> = selected
            .into_iter()
            .filter(|m| {
                if m.content.contains("<private>") {
                    true
                } else if should_skip_content(&m.content) {
                    debug!("skipping meta operation message (3+ patterns matched): {} chars", m.content.len());
                    false
                } else if is_meta_operation(&m.content) {
                    debug!("skipping meta operation message: {} chars", m.content.len());
                    false
                } else {
                    true
                }
            })
            .collect();

        let extractor = self.extractor.clone();
        let reconciler = self.reconciler.clone();
        let cluster_assigner = self.cluster_assigner.clone();
        let cluster_manager = self.cluster_manager.clone();
        let store = self.store.clone();
        let noise_filter = self.noise_filter.clone();
        let admission = self.admission.clone();
        let embed = self.embed.clone();
        let entity_context = request.entity_context.clone();
        let tenant_id = request.tenant_id.clone();
        let bg_task_id = task_id.clone();
        let ingest_sem = self.ingest_semaphore.clone();

        tokio::spawn(async move {
            let _permit = match ingest_sem {
                Some(sem) => Some(sem.acquire_owned().await.expect("ingest semaphore")),
                None => None,
            };

            info!(task_id = %bg_task_id, message_count = selected.len(), "slow path: starting extraction");

            let sanitized: Vec<IngestMessage> = selected
                .iter()
                .map(|m| IngestMessage {
                    role: m.role.clone(),
                    content: strip_private_content(&m.content),
                })
                .filter(|m| !is_fully_private(&m.content))
                .collect();

            if sanitized.is_empty() {
                info!(task_id = %bg_task_id, "all messages fully private, skipping");
                return;
            }

            let facts = match extractor
                .extract(&sanitized, entity_context.as_deref())
                .await
            {
                Ok(f) => f,
                Err(e) => {
                    error!(error = %e, task_id = %bg_task_id, "extraction failed — raw mode fallback");
                    return;
                }
            };

            if facts.is_empty() {
                info!(task_id = %bg_task_id, "no facts extracted");
                return;
            }

            let mut noise_guard = noise_filter.lock().await;
            let mut clean_facts = Vec::new();
            for fact in &facts {
                let fact_vector = match embed.embed(std::slice::from_ref(&fact.l0_abstract)).await {
                    Ok(vecs) => vecs.into_iter().next(),
                    Err(_) => None,
                };
                if noise_guard.is_noise(&fact.l0_abstract, fact_vector.as_deref()) {
                    debug!(fact = %fact.l0_abstract, "filtered as noise");
                    if let Some(vec) = fact_vector {
                        noise_guard.learn_noise(vec);
                    }
                    continue;
                }
                clean_facts.push(fact.clone());
            }
            drop(noise_guard);

            if clean_facts.is_empty() {
                info!(task_id = %bg_task_id, "all facts filtered as noise");
                return;
            }

            let conversation_text: String = sanitized
                .iter()
                .map(|m| format!("{}: {}", m.role, m.content))
                .collect::<Vec<_>>()
                .join("\n");

            let mut admitted_facts = Vec::new();
            for fact in &clean_facts {
                if is_compression_artifact(&fact.l0_abstract) {
                    debug!(fact = %fact.l0_abstract, "filtered as compression artifact");
                    continue;
                }

                let category = fact
                    .category
                    .parse::<Category>()
                    .unwrap_or(Category::Events);

                let result = admission
                    .evaluate(fact, &category, Some(&conversation_text))
                    .await;

                match result {
                    Ok(ar) if ar.admitted => {
                        debug!(
                            fact = %fact.l0_abstract,
                            score = ar.score,
                            hint = %ar.hint,
                            "admitted"
                        );
                        admitted_facts.push(fact.clone());
                    }
                    Ok(ar) => {
                        debug!(
                            fact = %fact.l0_abstract,
                            score = ar.score,
                            hint = %ar.hint,
                            "rejected by admission"
                        );
                    }
                    Err(e) => {
                        warn!(
                            error = %e,
                            fact = %fact.l0_abstract,
                            "admission error, rejecting by default"
                        );
                    }
                }
            }

            if admitted_facts.is_empty() {
                info!(task_id = %bg_task_id, "all facts rejected by admission control");
                return;
            }

            for fact in &mut admitted_facts {
                if fact.source_text.is_none() {
                    fact.source_text = Some(conversation_text.clone());
                }
            }

            for fact in &mut admitted_facts {
                let llm_tagged = fact.tags.contains(&"私密".to_string());
                let content_detected = detect_private_content(&fact.l2_content);
                if llm_tagged || content_detected {
                    fact.visibility = "private".to_string();
                    fact.owner_agent_id = agent_id.clone().unwrap_or_default();
                    if !llm_tagged && content_detected {
                        fact.tags.push("私密".to_string());
                    }
                }
            }

            match reconciler.reconcile(&admitted_facts, &tenant_id, agent_id.clone(), Some(session_id.clone())).await {
                Ok(memories) => {
                    info!(
                        task_id = %bg_task_id,
                        fact_count = admitted_facts.len(),
                        memory_count = memories.len(),
                        "reconciliation complete"
                    );
                    
                    for mem in &memories {
                        match cluster_assigner.assign(mem).await {
                            Ok(result) => {
                                match result.action {
                                    crate::cluster::assigner::AssignAction::AutoAssign => {
                                        if let Some(cid) = result.cluster_id {
                                            match cluster_manager.assign_to_cluster(&mem.id, &cid, store.clone()
                                            ).await {
                                                Ok(_) => info!(memory_id = %mem.id, cluster_id = %cid, "assigned to existing cluster"),
                                                Err(e) => warn!(error = %e, memory_id = %mem.id, "failed to assign memory to cluster"),
                                            }
                                        }
                                    }
                                    crate::cluster::assigner::AssignAction::CreateNew => {
                                        match embed.embed(&[mem.content.clone()]).await {
                                            Ok(vectors) => {
                                                if let Some(vector) = vectors.first() {
                                                    match cluster_manager.create_cluster(mem, vector, mem.tags.clone()).await {
                                                        Ok(cluster) => {
                                                            // 回写 memory.cluster_id，否则召回时聚合器找不到簇关系
                                                            if let Err(e) = cluster_manager.assign_to_cluster(&mem.id, &cluster.id, store.clone()).await {
                                                                warn!(error = %e, memory_id = %mem.id, "failed to link memory to new cluster");
                                                            } else {
                                                                info!(memory_id = %mem.id, cluster_id = %cluster.id, "created new cluster and linked memory");
                                                            }
                                                        }
                                                        Err(e) => warn!(error = %e, memory_id = %mem.id, "failed to create cluster"),
                                                    }
                                                }
                                            }
                                            Err(e) => warn!(error = %e, memory_id = %mem.id, "failed to embed for cluster creation"),
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            Err(e) => {
                                warn!(error = %e, memory_id = %mem.id, "cluster assignment failed");
                            }
                        }
                    }
                }
                Err(e) => {
                    error!(error = %e, task_id = %bg_task_id, "reconciliation failed");
                }
            }
        });

        Ok(IngestResponse {
            task_id,
            stored_count,
        })
    }
}

fn get_meta_regex_set() -> &'static RegexSet {
    static META_RE: OnceLock<RegexSet> = OnceLock::new();
    META_RE.get_or_init(|| {
        RegexSet::new([
            r"(?i)search returned",
            r"(?i)found \d+ matching",
            r"(?i)query\s*->\s*none",
            r"(?i)extracted \d+ facts",
            r"(?i)reconciliation complete",
            r"(?i)compression #\d+",
            r"(?i)dcp-message-id",
            r"(?i)<system-reminder>",
            r"(?i)background task",
            r"(?i)task_id.*bg_",
            r"(?i)session_id:\s*ses_",
            r"(?i)cargo (build|test|check|run)",
            r"(?i)npm (run|install|test)",
            r"(?i)HTTP \d{3}",
            r"(?i)status code \d+",
            r"(?i)docker (ps|run|exec|logs)",
            r"(?i)grep.*pattern",
            r"(?i)ast_grep",
            r"(?i)lsp_diagnostics",
            r"(?i)git (status|log|diff|add|commit|push|pull)",
        ])
        .expect("meta operation regex compilation failed")
    })
}

fn is_meta_operation(content: &str) -> bool {
    get_meta_regex_set().is_match(content)
}

fn should_skip_content(content: &str) -> bool {
    let content_lower = content.to_lowercase();

    let system_patterns = [
        "compression #",
        "compressed conversation section",
        "[system-reminder]",
        "[dcp-message-id]",
        "background task",
        "tool call message",
        "cargo check",
        "cargo build",
        "npm run build",
        "git commit",
        "git push",
        "task_id",
        "session_id",
        "background_output",
    ];

    let match_count = system_patterns
        .iter()
        .filter(|p| content_lower.contains(*p))
        .count();

    match_count >= 1
}

fn is_compression_artifact(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.contains("完成dcp压缩")
        || (lower.contains("移除") && lower.contains("数据"))
        || (lower.contains("新增") && lower.contains("摘要"))
        || (lower.contains("compressed") && lower.contains("messages"))
}

fn select_messages(messages: &[IngestMessage]) -> Vec<IngestMessage> {
    let mut selected = Vec::new();
    let mut total_bytes = 0;

    for msg in messages.iter().rev() {
        if selected.len() >= MESSAGE_BUDGET {
            break;
        }
        let msg_bytes = msg.content.len();
        if total_bytes + msg_bytes > BYTE_BUDGET && !selected.is_empty() {
            break;
        }
        total_bytes += msg_bytes;
        selected.push(msg.clone());
    }

    selected.reverse();
    selected
}

fn get_private_regex_set() -> &'static RegexSet {
    static PRIVATE_RE: OnceLock<RegexSet> = OnceLock::new();
    PRIVATE_RE.get_or_init(|| {
        RegexSet::new([
            r"\b(?:\d{1,3}\.){3}\d{1,3}\b",
            r"密码[是为：:]\s*\S+",
            r"password\s*[=:]\s*\S+",
            r"sk-[a-zA-Z0-9]{20,}",
            r"api[_-]?key\s*[=:]\s*\S+",
            r"token\s*[=:]\s*\S+",
            r"ssh-[a-z]{3,}\s+\S+",
            r"mysql://",
            r"postgres://",
            r"mongodb://",
            r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Z|a-z]{2,}\b",
            r"1[3-9]\d{9}",
            r"\b\d{16,19}\b",
            r"\b\d{17}[\dXx]\b",
        ])
        .expect("private content regex compilation failed")
    })
}

fn detect_private_content(text: &str) -> bool {
    get_private_regex_set().is_match(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingest::session::SessionStore;
    use crate::store::LanceStore;
    use std::sync::Mutex;
    use tempfile::TempDir;

    struct MockEmbed;

    #[async_trait::async_trait]
    impl EmbedService for MockEmbed {
        async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, OmemError> {
            Ok(texts.iter().map(|_| vec![0.0; 1024]).collect())
        }
        fn dimensions(&self) -> usize {
            1024
        }
    }

    struct TrackingLlm {
        call_count: Mutex<u32>,
        response: String,
    }

    impl TrackingLlm {
        fn new(response: &str) -> Self {
            Self {
                call_count: Mutex::new(0),
                response: response.to_string(),
            }
        }

        fn calls(&self) -> u32 {
            *self.call_count.lock().expect("lock")
        }
    }

    #[async_trait::async_trait]
    impl LlmService for TrackingLlm {
        async fn complete_text(&self, _system: &str, _user: &str) -> Result<String, OmemError> {
            *self.call_count.lock().expect("lock") += 1;
            Ok(self.response.clone())
        }
    }

    struct FailingLlm;

    #[async_trait::async_trait]
    impl LlmService for FailingLlm {
        async fn complete_text(&self, _system: &str, _user: &str) -> Result<String, OmemError> {
            Err(OmemError::Llm("service unavailable".to_string()))
        }
    }

    async fn setup_pipeline(
        llm: Arc<dyn LlmService>,
    ) -> (IngestPipeline, Arc<SessionStore>, TempDir) {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().to_str().expect("path");

        let store = Arc::new(LanceStore::new(path).await.expect("store"));
        store.init_table().await.expect("init memories");

        let session_store = Arc::new(SessionStore::new(path).await.expect("session store"));
        session_store.init_table().await.expect("init sessions");

        let embed: Arc<dyn EmbedService> = Arc::new(MockEmbed);
        let cluster_store = Arc::new(
            ClusterStore::new(store.db()).await.expect("cluster store")
        );
        let pipeline = IngestPipeline::new(
            store,
            session_store.clone(),
            embed,
            llm,
            cluster_store,
            "balanced",
            None,
            None,
        ).await.expect("pipeline");

        (pipeline, session_store, dir)
    }

    fn make_request(messages: Vec<(&str, &str)>, mode: IngestMode) -> IngestRequest {
        IngestRequest {
            messages: messages
                .into_iter()
                .map(|(role, content)| IngestMessage {
                    role: role.to_string(),
                    content: content.to_string(),
                })
                .collect(),
            tenant_id: "t-001".to_string(),
            agent_id: Some("agent-1".to_string()),
            session_id: Some("sess-test".to_string()),
            entity_context: None,
            mode,
        }
    }

    #[tokio::test]
    async fn test_fast_path_stores_sessions() {
        let llm = Arc::new(TrackingLlm::new(r#"{"memories":[]}"#));
        let (pipeline, session_store, _dir) = setup_pipeline(llm).await;

        let request = make_request(
            vec![
                ("user", "I prefer dark mode"),
                ("assistant", "Noted!"),
                ("user", "Also use Rust"),
            ],
            IngestMode::Smart,
        );

        let response = pipeline.ingest(request).await.expect("ingest");
        assert_eq!(response.stored_count, 3);
        assert!(!response.task_id.is_empty());

        let count = session_store
            .count_by_session("sess-test")
            .await
            .expect("count");
        assert_eq!(count, 3);
    }

    #[tokio::test]
    async fn test_content_hash_dedup_via_pipeline() {
        let llm = Arc::new(TrackingLlm::new(r#"{"memories":[]}"#));
        let (pipeline, session_store, _dir) = setup_pipeline(llm).await;

        let request = make_request(
            vec![("user", "hello"), ("assistant", "hi")],
            IngestMode::Raw,
        );

        let r1 = pipeline.ingest(request).await.expect("first ingest");
        assert_eq!(r1.stored_count, 2);

        let dup_request = make_request(
            vec![("user", "hello"), ("assistant", "hi")],
            IngestMode::Raw,
        );

        let r2 = pipeline.ingest(dup_request).await.expect("second ingest");
        assert_eq!(r2.stored_count, 0);

        let count = session_store
            .count_by_session("sess-test")
            .await
            .expect("count");
        assert_eq!(count, 2);
    }

    #[test]
    fn test_message_budget() {
        let messages: Vec<IngestMessage> = (0..25)
            .map(|i| IngestMessage {
                role: "user".to_string(),
                content: format!("message {i}"),
            })
            .collect();

        let selected = select_messages(&messages);
        assert_eq!(selected.len(), MESSAGE_BUDGET);

        assert_eq!(selected[0].content, "message 5");
        assert_eq!(selected[MESSAGE_BUDGET - 1].content, "message 24");
    }

    #[test]
    fn test_message_budget_byte_limit() {
        let big_content = "x".repeat(100_000);
        let messages: Vec<IngestMessage> = (0..5)
            .map(|_| IngestMessage {
                role: "user".to_string(),
                content: big_content.clone(),
            })
            .collect();

        let selected = select_messages(&messages);
        assert_eq!(selected.len(), 2);
    }

    #[test]
    fn test_message_budget_always_includes_one() {
        let huge = "x".repeat(500_000);
        let messages = vec![IngestMessage {
            role: "user".to_string(),
            content: huge,
        }];

        let selected = select_messages(&messages);
        assert_eq!(selected.len(), 1);
    }

    #[test]
    fn test_message_budget_empty() {
        let selected = select_messages(&[]);
        assert!(selected.is_empty());
    }

    #[test]
    fn test_message_budget_preserves_order() {
        let messages: Vec<IngestMessage> = (0..5)
            .map(|i| IngestMessage {
                role: "user".to_string(),
                content: format!("msg-{i}"),
            })
            .collect();

        let selected = select_messages(&messages);
        assert_eq!(selected.len(), 5);
        for (i, msg) in selected.iter().enumerate() {
            assert_eq!(msg.content, format!("msg-{i}"));
        }
    }

    #[tokio::test]
    async fn test_raw_mode() {
        let llm = Arc::new(TrackingLlm::new(r#"{"memories":[]}"#));
        let llm_ref = llm.clone();
        let (pipeline, session_store, _dir) = setup_pipeline(llm as Arc<dyn LlmService>).await;

        let request = make_request(
            vec![("user", "remember this"), ("assistant", "ok")],
            IngestMode::Raw,
        );

        let response = pipeline.ingest(request).await.expect("ingest");
        assert_eq!(response.stored_count, 2);

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        assert_eq!(llm_ref.calls(), 0, "LLM should not be called in raw mode");

        let count = session_store
            .count_by_session("sess-test")
            .await
            .expect("count");
        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn test_graceful_degradation() {
        let llm: Arc<dyn LlmService> = Arc::new(FailingLlm);
        let (pipeline, session_store, _dir) = setup_pipeline(llm).await;

        let request = make_request(
            vec![("user", "important data"), ("assistant", "received")],
            IngestMode::Smart,
        );

        let response = pipeline
            .ingest(request)
            .await
            .expect("ingest should succeed despite LLM failure");
        assert_eq!(response.stored_count, 2);

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let count = session_store
            .count_by_session("sess-test")
            .await
            .expect("count");
        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn test_empty_messages_rejected() {
        let llm: Arc<dyn LlmService> = Arc::new(TrackingLlm::new(r#"{"memories":[]}"#));
        let (pipeline, _session_store, _dir) = setup_pipeline(llm).await;

        let request = IngestRequest {
            messages: vec![],
            tenant_id: "t-001".to_string(),
            agent_id: None,
            session_id: None,
            entity_context: None,
            mode: IngestMode::Smart,
        };

        let result = pipeline.ingest(request).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_auto_generated_session_id() {
        let llm: Arc<dyn LlmService> = Arc::new(TrackingLlm::new(r#"{"memories":[]}"#));
        let (pipeline, _session_store, _dir) = setup_pipeline(llm).await;

        let request = IngestRequest {
            messages: vec![IngestMessage {
                role: "user".to_string(),
                content: "test".to_string(),
            }],
            tenant_id: "t-001".to_string(),
            agent_id: None,
            session_id: None,
            entity_context: None,
            mode: IngestMode::Raw,
        };

        let response = pipeline.ingest(request).await.expect("ingest");
        assert_eq!(response.stored_count, 1);
    }

    #[test]
    fn test_detect_private_content_ip() {
        assert!(detect_private_content("Server IP is 192.168.1.1"));
        assert!(detect_private_content("Connect to 47.93.199.242"));
        assert!(!detect_private_content("The year is 2024"));
    }

    #[test]
    fn test_detect_private_content_password() {
        assert!(detect_private_content("密码是 abc123"));
        assert!(detect_private_content("password=secret123"));
        assert!(detect_private_content("password: mypass"));
        assert!(!detect_private_content("talking about passwords in general"));
    }

    #[test]
    fn test_detect_private_content_api_key() {
        assert!(detect_private_content("API key: sk-abcdefghijklmnopqrstuvwxyz"));
        assert!(detect_private_content("api_key=xyz123"));
        assert!(detect_private_content("token: bearer_abc123"));
        assert!(detect_private_content("sk-live-51HxZ9l2eZvKYlo2C0XJqWn3"));
    }

    #[test]
    fn test_detect_private_content_ssh_and_db() {
        assert!(detect_private_content("ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABgQC..."));
        assert!(detect_private_content("mysql://user:pass@localhost/db"));
        assert!(detect_private_content("postgres://user:pass@host/db"));
        assert!(detect_private_content("mongodb://user:pass@host/db"));
    }

    #[test]
    fn test_detect_private_content_personal() {
        assert!(detect_private_content("Contact me at user@example.com"));
        assert!(detect_private_content("Phone: 13800138000"));
        assert!(detect_private_content("Card: 6222021234567890123"));
        assert!(detect_private_content("ID: 110101199001011234"));
        assert!(!detect_private_content("The number 12345"));
    }

    #[test]
    fn test_is_compression_artifact_dcp_compress() {
        assert!(is_compression_artifact("完成DCP压缩#10，移除-161.6K数据"));
        assert!(is_compression_artifact("用户完成DCP压缩操作"));
    }

    #[test]
    fn test_is_compression_artifact_remove_data() {
        assert!(is_compression_artifact("移除大量无用数据"));
        assert!(is_compression_artifact("本次操作移除了历史数据"));
    }

    #[test]
    fn test_is_compression_artifact_add_summary() {
        assert!(is_compression_artifact("新增多个摘要条目"));
        assert!(is_compression_artifact("系统新增会话摘要"));
    }

    #[test]
    fn test_is_compression_artifact_compressed_messages() {
        assert!(is_compression_artifact("Compressed 15 messages into summary"));
        assert!(is_compression_artifact("Successfully compressed all messages"));
    }

    #[test]
    fn test_is_compression_artifact_not_artifact() {
        assert!(!is_compression_artifact("User prefers dark mode"));
        assert!(!is_compression_artifact("I like to compress files with gzip"));
        assert!(!is_compression_artifact("新增用户偏好设置"));
        assert!(!is_compression_artifact("移除旧密码"));
    }

    #[test]
    fn test_is_compression_artifact_case_insensitive() {
        assert!(is_compression_artifact("COMPRESSED MESSAGES"));
        assert!(is_compression_artifact("完成DCP压缩"));
    }
}
