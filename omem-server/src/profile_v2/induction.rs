use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tokio::time::timeout;

use crate::domain::error::OmemError;
use crate::llm::complete_json;
use crate::profile_v2::slots::is_valid_slot_name;
use crate::profile_v2::types::*;

use super::service::ProfileV2Service;
use super::store::ProfileStore;

const INDUCTION_SYSTEM_PROMPT: &str = "\
你是偏好归纳引擎。从用户的行为记忆中提取偏好。每条偏好对应一个slot和一个具体值。仅从提供的记忆中提取，不编造。输出JSON数组。

## Slot定义
- communication_style: 沟通方式偏好（简洁/详细/正式/随意等）
- tone: 语言风格偏好（冷淡/热情/幽默/严肃等）
- code_style: 编码风格偏好（卫语句/早返回/命名规范等）
- error_handling: 错误处理偏好（日志策略/重试策略/降级方案等）
- naming_convention: 命名规范偏好（驼峰/下划线/前缀规则等）
- testing_strategy: 测试策略偏好（TDD/BDD/覆盖率要求等）
- workflow_preference: 工作流程偏好（先规划后执行/先原型后优化等）
- commit_style: 提交风格偏好（语义化版本/conventional commits等）
- emoji_preference: emoji使用偏好（是否使用/风格/频率等）
- self_reference: 自称偏好（我/本座/月儿等）
- address_style: 称呼他人方式（你/您/师尊等）
- language: 语言偏好（中文/英文/双语等）
- framework_preference: 框架/技术栈偏好（React vs Vue等）
- preferred_tools: 工具偏好（编辑器/终端/构建工具等）
- custom:* 自定义slot（格式：custom:描述，如custom:deploy_strategy）

## 输出格式
[{\"slot\":\"slot_name\",\"value\":\"偏好描述\",\"confidence\":0.0到1.0,\"scope\":\"project或global\"}]

## 规则
1. confidence: 0.5-0.9（从单条记忆推断0.5-0.6，多条一致0.7-0.9）
2. scope: 涉及特定项目用project，跨项目通用用global
3. 每条记忆最多提取3条偏好
4. 没有明确偏好的记忆跳过
5. 偏好边界：只提取反复出现的、稳定的行为模式。以下不属于偏好，必须跳过：
   - 一次性决策（这次用方案A vs 每次都用方案A）
   - 具体bug修复步骤
   - 项目特定配置值（URL、端口、密钥）
   - 临时workaround
   - 单次任务执行记录
   判断标准：如果这个行为在未来新项目中也会重复出现，才是偏好。
6. value长度硬限制：value必须≤150个字符。超过150字符的value将被系统丢弃。优先保留操作性信息（命令模板、工具名），删除修饰性描述。宁可分拆为多条偏好也不要合并成一条超长的。
7. 好的value示例：「卫语句优先：先处理错误/边界case→return，正常逻辑放最后」
8. 差的value示例：「使用卫语句处理错误情况，先检查边界条件然后返回，正常逻辑放在最后面」——啰嗦，无操作价值
9. 去重：多条记忆指向同一偏好→合并为一条，取信息最完整的描述，confidence取最高值";

pub struct InductionResult {
    pub run_id: String,
    pub extracted_count: usize,
}

pub struct InductionEngine {
    service: Arc<ProfileV2Service>,
}

impl InductionEngine {
    pub fn new(service: Arc<ProfileV2Service>) -> Self {
        Self { service }
    }

    pub fn service(&self) -> &Arc<ProfileV2Service> {
        &self.service
    }

    /// `candidate_texts` 由调用方从 LanceStore 查询后传入，归纳引擎不直接依赖 LanceStore。
    /// 返回 `Ok(None)` 表示跳过（未启用/锁冲突/冷却期/候选不足）。
    pub async fn trigger_induction(
        &self,
        tenant_id: &str,
        _trigger_reason: &str,
        candidate_texts: &[String],
        project_path: Option<&str>,
    ) -> Result<Option<InductionResult>, OmemError> {
        let store = self.service.store();
        let config = self.service.config();

        // ── Step 1: 检查启用 + 归纳锁 ──
        if !self.service.is_enabled() {
            return Ok(None);
        }

        if let Some(lock) = store.get_induction_lock(tenant_id)? {
            tracing::debug!(
                tenant_id,
                lock_id = %lock.id,
                "induction lock exists, skipping"
            );
            return Ok(None);
        }

        // ── Step 2: 检查冷却期 ──
        let recent_runs = store.get_induction_runs(tenant_id, 1)?;
        if let Some(last_run) = recent_runs.first() {
            let elapsed = Utc::now()
                .signed_duration_since(last_run.started_at)
                .num_seconds();
            if elapsed >= 0 && (elapsed as u64) < config.induction_cooldown_secs {
                tracing::debug!(
                    tenant_id,
                    elapsed_secs = elapsed,
                    "induction cooldown, skipping"
                );
                return Ok(None);
            }
        }

        // ── Step 3: 获取锁 + 创建 run ──
        let acquired = store.acquire_induction_lock(tenant_id, 600)?;
        if !acquired {
            tracing::debug!(tenant_id, "failed to acquire induction lock");
            return Ok(None);
        }

        let run_id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now();
        store.create_induction_run(&InductionRun {
            id: run_id.clone(),
            tenant_id: tenant_id.to_string(),
            status: "running".to_string(),
            candidate_count: candidate_texts.len() as i32,
            extracted_count: 0,
            error: None,
            started_at: now,
            completed_at: None,
        })?;

        // ── Steps 4-10: 核心归纳逻辑（match 保证锁释放） ──
        let result = self
            .run_induction_inner(tenant_id, &run_id, candidate_texts, project_path)
            .await;

        match result {
            Ok(extracted_count) => {
                store.invalidate_cache(tenant_id);
                Ok(Some(InductionResult {
                    run_id,
                    extracted_count,
                }))
            }
            Err(e) => {
                tracing::error!(tenant_id, error = %e, "induction failed");
                let _ = store.update_induction_run(&run_id, "failed", 0, Some(&e.to_string()));
                if let Err(release_err) = Self::release_lock_by_tenant(store, tenant_id) {
                    tracing::warn!(tenant_id, error = %release_err, "failed to release induction lock after error");
                }
                Err(e)
            }
        }
    }

    /// 核心归纳逻辑（步骤 4-10），调用方负责锁管理。
    async fn run_induction_inner(
        &self,
        tenant_id: &str,
        run_id: &str,
        candidate_texts: &[String],
        project_path: Option<&str>,
    ) -> Result<usize, OmemError> {
        let store = self.service.store();
        let config = self.service.config();

        // ── Step 4: 候选不足则跳过 ──
        if candidate_texts.len() < config.induction_threshold {
            tracing::debug!(
                tenant_id,
                count = candidate_texts.len(),
                threshold = config.induction_threshold,
                "not enough candidates for induction"
            );
            store.update_induction_run(run_id, "skipped", 0, Some("not enough candidates"))?;
            Self::release_lock_by_tenant(store, tenant_id)?;
            return Ok(0);
        }

        // ── Step 5: 调用 LLM 归纳 ──
        let llm = match self.service.llm() {
            Some(llm) => llm,
            None => {
                store.update_induction_run(run_id, "failed", 0, Some("no LLM available"))?;
                Self::release_lock_by_tenant(store, tenant_id)?;
                return Ok(0);
            }
        };

        let user_prompt = format!(
            "以下是从用户行为中提取的{}条记忆：\n\n{}\n\n请从中提取用户偏好，输出JSON数组。",
            candidate_texts.len(),
            candidate_texts
                .iter()
                .enumerate()
                .map(|(i, t)| format!("{}. {}", i + 1, t))
                .collect::<Vec<_>>()
                .join("\n")
        );

        let llm_future =
            complete_json::<Vec<InductedPreference>>(llm.as_ref(), INDUCTION_SYSTEM_PROMPT, &user_prompt);

        let inducted = match timeout(Duration::from_secs(60), llm_future).await {
            Ok(Ok(result)) => result,
            Ok(Err(e)) => {
                store.update_induction_run(
                    run_id,
                    "failed",
                    0,
                    Some(&format!("LLM error: {e}")),
                )?;
                Self::release_lock_by_tenant(store, tenant_id)?;
                return Ok(0);
            }
            Err(_) => {
                store.update_induction_run(run_id, "failed", 0, Some("LLM timeout 60s"))?;
                Self::release_lock_by_tenant(store, tenant_id)?;
                return Ok(0);
            }
        };

        // ── Step 6-8: 验证 + 冲突解决 + 写入 ──
        let existing_prefs = store.get_preferences(tenant_id, None)?;
        let mut extracted_count = 0usize;

        for item in inducted {
            if !is_valid_slot_name(&item.slot) {
                tracing::warn!(slot = %item.slot, "invalid slot name from induction, skipping");
                continue;
            }
            if item.confidence < 0.0 || item.confidence > 1.0 {
                tracing::warn!(
                    slot = %item.slot,
                    confidence = item.confidence,
                    "invalid confidence from induction, skipping"
                );
                continue;
            }
            if item.scope != "project" && item.scope != "global" {
                tracing::warn!(slot = %item.slot, scope = %item.scope, "invalid scope from induction, skipping");
                continue;
            }
            if item.value.trim().is_empty() {
                continue;
            }

            // value超150字符 → 丢弃，让LLM下次生成更短的
            if item.value.chars().count() > 150 {
                tracing::warn!(
                    slot = %item.slot,
                    char_count = item.value.chars().count(),
                    "induction value exceeds 150 chars, discarding (LLM should enforce this)"
                );
                continue;
            }

            let matching = existing_prefs.iter().find(|p| {
                if p.slot != item.slot || p.status == PreferenceStatus::Deleted {
                    return false;
                }
                // Exact match
                if p.value == item.value {
                    return true;
                }
                // Keyword overlap: same slot + 40%+ keyword overlap = duplicate
                let kw_existing = extract_keywords(&p.value);
                let kw_new = extract_keywords(&item.value);
                let union_count = kw_existing.union(&kw_new).count();
                if union_count == 0 {
                    return false;
                }
                let overlap_count = kw_existing.intersection(&kw_new).count();
                overlap_count as f32 / union_count as f32 > 0.6
            });

            if let Some(existing) = matching {
                let new_confidence = (existing.confidence + 0.15).min(0.95);
                store.update_confidence(&existing.id, 0.15)?;

                store.record_changelog(&ProfileChangelog {
                    id: uuid::Uuid::new_v4().to_string(),
                    tenant_id: tenant_id.to_string(),
                    preference_id: existing.id.clone(),
                    action: "reinforced".to_string(),
                    old_value: None,
                    new_value: Some(format!(
                        "confidence: {:.2}→{:.2}",
                        existing.confidence, new_confidence
                    )),
                    source: "induction".to_string(),
                    created_at: Utc::now(),
                })?;
            } else {
                let now = Utc::now();
                let pref = Preference {
                    id: uuid::Uuid::new_v4().to_string(),
                    tenant_id: tenant_id.to_string(),
                    slot: item.slot.clone(),
                    value: item.value.clone(),
                    confidence: item.confidence,
                    scope: if item.scope == "global" {
                        PreferenceScope::Global
                    } else {
                        PreferenceScope::Project
                    },
                    project_path: project_path.map(|s| s.to_string()),
                    source: "observed".to_string(),
                    status: PreferenceStatus::Active,
                    last_reinforced_at: now,
                    created_at: now,
                    updated_at: now,
                };
                store.upsert_preference(&pref)?;

                store.record_changelog(&ProfileChangelog {
                    id: uuid::Uuid::new_v4().to_string(),
                    tenant_id: tenant_id.to_string(),
                    preference_id: pref.id,
                    action: "created".to_string(),
                    old_value: None,
                    new_value: Some(item.value.clone()),
                    source: "induction".to_string(),
                    created_at: Utc::now(),
                })?;
            }
            extracted_count += 1;
        }

        // ── Step 9: 保存 version 快照 ──
        let all_prefs = store.get_preferences(tenant_id, None)?;
        let snapshot = serde_json::to_string(&all_prefs).unwrap_or_default();
        store.save_version(&ProfileVersion {
            id: uuid::Uuid::new_v4().to_string(),
            tenant_id: tenant_id.to_string(),
            snapshot,
            preference_count: all_prefs.len() as i32,
            created_at: Utc::now(),
        })?;

        // ── Step 10: 释放锁 + 更新 run ──
        Self::release_lock_by_tenant(store, tenant_id)?;
        store.update_induction_run(run_id, "completed", extracted_count as i32, None)?;
        store.invalidate_cache(tenant_id);

        tracing::info!(
            tenant_id,
            run_id,
            extracted_count,
            candidate_count = candidate_texts.len(),
            "induction completed"
        );

        Ok(extracted_count)
    }

    fn release_lock_by_tenant(store: &ProfileStore, tenant_id: &str) -> Result<(), OmemError> {
        if let Some(lock) = store.get_induction_lock(tenant_id)? {
            store.release_induction_lock(&lock.id)?;
        }
        Ok(())
    }
}

fn extract_keywords(text: &str) -> std::collections::HashSet<String> {
    let mut keywords = std::collections::HashSet::new();
    let chars: Vec<char> = text.chars().collect();
    for i in 0..chars.len().saturating_sub(1) {
        let a = chars[i];
        let b = chars[i + 1];
        if ('\u{4e00}'..='\u{9fff}').contains(&a) && ('\u{4e00}'..='\u{9fff}').contains(&b) {
            keywords.insert(format!("{a}{b}"));
        }
    }
    for m in regex::Regex::new(r"[a-zA-Z_]{3,}").unwrap().find_iter(text) {
        keywords.insert(m.as_str().to_lowercase());
    }
    keywords
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn induction_result_fields() {
        let result = InductionResult {
            run_id: "test-run".to_string(),
            extracted_count: 5,
        };
        assert_eq!(result.run_id, "test-run");
        assert_eq!(result.extracted_count, 5);
    }
}
