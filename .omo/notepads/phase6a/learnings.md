# Phase 6a Learnings

## 2026-05-21 Phase 6a 状态初查

### Phase 2 前置条件已满足
- AppState 有 sqlite_store: Arc<SqliteStore> 和 category_registry: Arc<CategoryRegistry>
- SqliteStore 在 admission.rs, pipeline.rs, reconciler.rs 中被引用
- CategoryRegistry 在 admission.rs, reconciler.rs 中使用 category_prior/category_importance

### Phase 6a 当前实施状态
- T1 types.rs + slots.rs: 文件已存在，含测试（167行 + 78行）
- T2 migration.rs + store.rs: 文件已存在（107行 + 630行），5张表DDL
- T3 config.rs + llm/: 未实施（OmemConfig 无 OMEM_PROFILE_* 字段）
- T4 service.rs: 文件不存在
- T5-T10: 全部未实施
- main.rs 未注册 profile_v2 模块 → 不在编译图中

### Plugin 端适配（Phase 6b）
- 只做 opencode plugin（师尊明确）
- 现有 buildProfileBlock() 输出 <cerebro-profile> 格式
- Phase 6a injection.rs 输出格式完全兼容
- Phase 6b 切换改动量约 30 行

## 2026-05-21 执行进度

### Wave 1 执行计划
- T1 ✅ 已存在：types.rs(167行) + slots.rs(78行)，含完整测试
- T2 ✅ 已存在：migration.rs(107行) + store.rs(630行)，5张表DDL+完整CRUD
- T3 ❌ 未实施：需要修改 config.rs（12个字段）+ llm/mod.rs（工厂函数）+ llm/openai_compat.rs（构造器）
- T4 ❌ 未实施：service.rs 不存在，mod.rs 中注释掉了

### T3 具体修改清单
#### config.rs 新增字段（在 recall_llm_refine_timeout_secs 后）
12个字段: profile_enabled(bool), profile_llm_provider(String), profile_llm_api_key(String), profile_llm_model(String), profile_llm_base_url(String), profile_cache_ttl_secs(u64), profile_induction_cooldown_secs(u64), profile_induction_threshold(usize), profile_injection_budget_tokens(usize), profile_max_global_preferences(usize), profile_max_project_preferences(usize), profile_dormant_days(u32)

默认值: enabled=true, provider="openai-compatible", model="deepseek-v4-flash", base_url="https://opencode.ai/zen/v1", cache_ttl=1800, cooldown=600, threshold=5, budget=500, max_global=20, max_project=10, dormant=90

#### llm/mod.rs 新增函数
create_profile_llm_service(config) — 照搬 create_recall_llm_service 模式
先检查 profile_enabled && profile_llm_api_key 非空
匹配 provider "openai-compatible" → OpenAICompatLlm::new_profile(config)

#### llm/openai_compat.rs 新增构造器
new_profile(config) — 照搬 new_cluster 模式
使用 config.profile_llm_api_key, profile_llm_model, profile_llm_base_url

### task() 工具故障
- 错误: "The 'path' property must be of type string, got object"
- 原因: workspace root 路径异常 (WSL+Windows路径拼接)
- 解决: 重启 opencode session
