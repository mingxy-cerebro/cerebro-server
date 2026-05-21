# Lifecycle 修复 + Prompt 分类优化

## TODOs

- [ ] T1: LanceStore 新增 `batch_update_tiers()` 方法
- [ ] T2: evaluate_tiers 改用批量更新
- [ ] T3: forgetting.rs 接入 DecayEngine.is_stale() + list() 降 limit
- [ ] T4: scheduler 调用 cleanup_stale
- [ ] T5: SESSION_EXTRACT prompt 修复 EMOTIONAL category
- [ ] T6: SESSION_COMPRESS prompt 增加分类拆分规则
- [ ] T7: 编译验证 + 测试
- [ ] T8: 部署 + 验证

## Final Verification Wave

- [ ] F1: Oracle 审查 lifecycle 改动（batch update + is_stale 接入）的 OOM 和 version 风险
- [ ] F2: Oracle 审查 prompt 改动不会导致 session_ingest 回归
