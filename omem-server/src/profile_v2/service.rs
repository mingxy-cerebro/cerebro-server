use std::sync::Arc;

use crate::config::OmemConfig;
use crate::llm::LlmService;

use super::store::ProfileStore;

/// Profile V2 服务门面 — 组合 store + LLM
/// 归纳引擎(induction.rs)和注入协议(injection.rs)通过此门面访问底层资源
pub struct ProfileV2Service {
    store: Arc<ProfileStore>,
    llm: Option<Arc<dyn LlmService>>,
    config: ProfileConfig,
}

/// 从 OmemConfig 提取的 profile 相关配置子集
pub struct ProfileConfig {
    pub enabled: bool,
    pub cache_ttl_secs: u64,
    pub induction_cooldown_secs: u64,
    pub induction_threshold: usize,
    pub injection_budget_tokens: usize,
    pub max_global_preferences: usize,
    pub max_project_preferences: usize,
    pub dormant_days: u32,
}

impl From<&OmemConfig> for ProfileConfig {
    fn from(config: &OmemConfig) -> Self {
        Self {
            enabled: config.profile_enabled,
            cache_ttl_secs: config.profile_cache_ttl_secs,
            induction_cooldown_secs: config.profile_induction_cooldown_secs,
            induction_threshold: config.profile_induction_threshold,
            injection_budget_tokens: config.profile_injection_budget_tokens,
            max_global_preferences: config.profile_max_global_preferences,
            max_project_preferences: config.profile_max_project_preferences,
            dormant_days: config.profile_dormant_days,
        }
    }
}

impl ProfileV2Service {
    pub fn new(
        store: Arc<ProfileStore>,
        llm: Option<Arc<dyn LlmService>>,
        config: &OmemConfig,
    ) -> Self {
        Self {
            store,
            llm,
            config: ProfileConfig::from(config),
        }
    }

    /// 给 handler/injection 访问 store
    pub fn store(&self) -> &Arc<ProfileStore> {
        &self.store
    }

    /// 给归纳引擎访问 LLM
    pub fn llm(&self) -> Option<&Arc<dyn LlmService>> {
        self.llm.as_ref()
    }

    /// 访问 profile 配置
    pub fn config(&self) -> &ProfileConfig {
        &self.config
    }

    /// profile 系统是否启用
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::OmemConfig;

    #[test]
    fn profile_config_from_omem_config() {
        let config = OmemConfig::default();
        let pc = ProfileConfig::from(&config);
        assert!(pc.enabled);
        assert_eq!(pc.cache_ttl_secs, 1800);
        assert_eq!(pc.induction_cooldown_secs, 600);
        assert_eq!(pc.induction_threshold, 5);
        assert_eq!(pc.injection_budget_tokens, 3000);
        assert_eq!(pc.max_global_preferences, 20);
        assert_eq!(pc.max_project_preferences, 10);
        assert_eq!(pc.dormant_days, 90);
    }

    #[test]
    fn service_assembly_with_no_llm() {
        let config = OmemConfig::default();
        // 创建 in-memory ProfileStore 需要先创建 SqliteStore
        // 这里只测试 config 提取逻辑
        let pc = ProfileConfig::from(&config);
        assert!(pc.enabled);
    }
}
