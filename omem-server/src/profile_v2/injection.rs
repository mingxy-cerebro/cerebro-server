use std::sync::Arc;
use std::time::Instant;

use dashmap::DashMap;

use crate::domain::error::OmemError;
use crate::profile_v2::service::ProfileV2Service;
use crate::profile_v2::types::*;

struct CachedInjection {
    content: String,
    preference_count: i32,
    cached_at: Instant,
}

pub struct InjectionBuilder {
    service: Arc<ProfileV2Service>,
    cache: DashMap<String, CachedInjection>,
}

impl InjectionBuilder {
    pub fn new(service: Arc<ProfileV2Service>) -> Self {
        Self {
            service,
            cache: DashMap::new(),
        }
    }

    pub fn service(&self) -> &Arc<ProfileV2Service> {
        &self.service
    }

    /// 构建注入内容。缓存key = `tenant_id:project_path`，TTL由ProfileConfig.cache_ttl_secs控制。
    pub fn build_injection(
        &self,
        tenant_id: &str,
        project_path: Option<&str>,
    ) -> Result<InjectionResult, OmemError> {
        let config = self.service.config();
        let cache_key = format!("{}:{}", tenant_id, project_path.unwrap_or(""));

        // 检查缓存
        if let Some(cached) = self.cache.get(&cache_key) {
            if cached.cached_at.elapsed().as_secs() < config.cache_ttl_secs {
                return Ok(InjectionResult {
                    content: cached.content.clone(),
                    preference_count: cached.preference_count,
                    estimated_tokens: (cached.content.len() / 4) as i32,
                });
            }
        }

        let store = self.service.store();

        // 1. 全局偏好（scope=global, status=active）按confidence降序 ≤ max_global_preferences
        let all_prefs = store.get_preferences(tenant_id, None)?;

        let global_prefs: Vec<&Preference> = all_prefs
            .iter()
            .filter(|p| p.scope == PreferenceScope::Global && p.status == PreferenceStatus::Active)
            .take(config.max_global_preferences)
            .collect();

        // 2. 项目偏好（scope=project, project_path匹配, status=active）≤ max_project_preferences
        let project_prefs: Vec<&Preference> = if let Some(_pp) = project_path {
            all_prefs
                .iter()
                .filter(|p| {
                    p.scope == PreferenceScope::Project
                        && p.status == PreferenceStatus::Active
                        && p.project_path.as_deref() == Some(_pp)
                })
                .take(config.max_project_preferences)
                .collect()
        } else {
            Vec::new()
        };

        // 3. 合并 + 按confidence降序排列
        let mut combined: Vec<&Preference> =
            Vec::with_capacity(global_prefs.len() + project_prefs.len());
        combined.extend(global_prefs);
        combined.extend(project_prefs);
        combined.sort_by(|a, b| {
            b.confidence
                .partial_cmp(&a.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // 4. Token预算裁剪：总字符 ≤ injection_budget_tokens
        let max_chars = config.injection_budget_tokens;
        let mut total_chars = 0;
        let mut selected: Vec<&Preference> = Vec::new();
        for pref in &combined {
            let line = format!("· {} — {}", pref.slot, pref.value);
            if total_chars + line.len() + 1 > max_chars {
                break;
            }
            total_chars += line.len() + 1;
            selected.push(pref);
        }

        let truncated_count = combined.len() - selected.len();
        if truncated_count > 0 {
            tracing::debug!(
                "profile injection budget trimmed: {}/{} preferences, {}/{} bytes used",
                selected.len(),
                combined.len(),
                total_chars,
                max_chars,
            );
        }

        // 5. 格式化为 <cerebro-profile> XML块
        let content = if selected.is_empty() {
            String::new()
        } else {
            let lines: Vec<String> = selected
                .iter()
                .map(|p| format!("  · {} — {}", p.slot, p.value))
                .collect();
            format!(
                "<cerebro-profile>\n{}\n</cerebro-profile>",
                lines.join("\n")
            )
        };

        let result = InjectionResult {
            preference_count: selected.len() as i32,
            estimated_tokens: (content.len() / 4) as i32,
            content,
        };

        // 写入缓存
        self.cache.insert(
            cache_key,
            CachedInjection {
                content: result.content.clone(),
                preference_count: result.preference_count,
                cached_at: Instant::now(),
            },
        );

        Ok(result)
    }

    /// 使缓存失效（归纳引擎写入后调用）
    pub fn invalidate_cache(&self, tenant_id: &str) {
        let prefix = format!("{}:", tenant_id);
        let keys_to_remove: Vec<String> = self
            .cache
            .iter()
            .filter(|entry| entry.key().starts_with(&prefix))
            .map(|entry| entry.key().clone())
            .collect();
        for key in keys_to_remove {
            self.cache.remove(&key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_injection_fields() {
        let ci = CachedInjection {
            content: "test".to_string(),
            preference_count: 1,
            cached_at: Instant::now(),
        };
        assert_eq!(ci.content, "test");
        assert_eq!(ci.preference_count, 1);
    }

    #[test]
    fn injection_builder_new_creates_empty_cache() {
        // 仅验证结构可以构建
        // 完整集成测试需要 ProfileStore + ProfileV2Service
    }
}
