# Phase 6b: Profile V2 Plugin 适配 + session_ingest 归纳触发

> 生成时间: 2026-05-22
> 前置条件: Phase 6a 已完成并合并到 main (commit bde0aa1)
> 更新时间: 2026-05-22 (灵犀REVISE + 玄机REVISE→GO，已修复2处P0)
> 审批状态: ✅ 灵犀+玄机 双审通过（修订后）
> 预计耗时: 30min

---

## 一、背景

Phase 6a 建立了独立画像系统 (Profile V2)。另一个窗口已完成 T2 (client.ts) 和 T3 (hooks.ts)，但 T1 和 T4 遗漏。

**已完成：**
- ✅ T2: client.ts — `getProfile()` (L284) 调 `/v2/profile`, `getInjection()` (L290) 调 `/v2/profile/inject`
- ✅ T3: hooks.ts — L379-393 已调用 `client.getInjection()` + TTL 缓存 (`profileInjectedSessions`)

**未完成：**
- ❌ T1: session_ingest handler 未添加归纳触发
- ❌ T4: tools.ts 未新增 V2 profile tools

---

## 二、未完成任务

### T1: session_ingest 添加归纳触发 [Rust 服务端]

**文件**: `omem-server/src/api/handlers/memory.rs`

**问题**: Plugin 端实际使用 `POST /v1/memories/session-ingest`（`session_ingest` handler），该 handler 有独立的 LLM topic 提取逻辑，**不经过 `IngestPipeline`**，导致 Phase 6a 在 `pipeline.rs` 中添加的归纳触发对 `session_ingest` 完全无效。

**插入位置**: L2080（cluster 循环 `}` 结束后）与 L2082（`tracing::info!` 统计日志前）之间，在 `tokio::spawn` 块内部。

**改动**:

```rust
// --- Profile V2 Induction Trigger ---
// state.induction_engine 类型是 Arc<InductionEngine>（非 Option），server.rs L45 确认
let engine = state.induction_engine.clone();
// created_memories: Vec<(Memory, Option<Vec<f32>>)>, L1575 声明
// 注意：EMOTIONAL/WORK 追加路径会 continue 跳过 created_memories.push，
// 所以 created_memories 只包含新建的 memory。追加的内容已在数据库中，
// 下次归纳时 InductionEngine 会从数据库查到。
let ind_texts: Vec<String> = created_memories.iter().map(|(m, _)| m.content.clone()).collect();
let ind_tenant = tenant_id.clone();
if !ind_texts.is_empty() {
    tracing::debug!(texts_count = ind_texts.len(), "triggering profile induction from session_ingest");
    tokio::spawn(async move {
        match engine.trigger_induction(&ind_tenant, "session_ingest", &ind_texts).await {
            Ok(Some(result)) => tracing::info!(run_id = %result.run_id, extracted = result.extracted_count, "session_ingest: profile_induction_triggered"),
            Ok(None) => tracing::debug!("session_ingest: profile_induction_skipped"),
            Err(e) => tracing::warn!(error = %e, "session_ingest: profile_induction_failed"),
        }
    });
}
```

**已确认事实**（灵犀+玄机审查）:
- `state.induction_engine` 类型: `Arc<InductionEngine>`（server.rs L45），**不是** `Option`，不能用 `if let Some`
- `created_memories: Vec<(Memory, Option<Vec<f32>>)>` 在 L1575 声明，需用 `(m, _)` 解构
- EMOTIONAL/WORK 追加路径会 `continue` 跳过 `created_memories.push`，追加内容不在该 Vec 中
- `trigger_induction` 签名: `(tenant_id: &str, _trigger_reason: &str, candidate_texts: &[String])`
- `induction_threshold` 默认 3，由 InductionEngine 内部控制，外部无需检查

**验证**: `cargo check` + `cargo test -p omem-server`

---

### T4: tools.ts 添加 Profile V2 工具 [TypeScript Plugin]

**文件**: `plugins/opencode/src/tools.ts`

**现状**: L166 有旧的 `memory_profile` tool 调用 `client.getProfile()`（已改为调 V2 API `/v2/profile`），但功能有限，只能查看偏好列表。

**新增 tools**:

在现有 `memory_profile` tool 之后添加:

```typescript
memory_profile_stats: tool({
  name: "memory_profile_stats",
  description: "查看用户偏好画像统计信息（偏好总数、slot分布、归纳运行次数等）",
  parameters: z.object({}),
  execute: async () => {
    const stats = await client.request("/v2/profile/stats");
    return JSON.stringify(stats, null, 2);
  },
}),
```

**不新增的 tools**: 创建/更新/删除偏好 — 不暴露给 AI 避免随意修改。归纳由系统自动触发。

**验证**: `npx tsc` 零错误 + 三端飞升部署流程

---

## 三、执行顺序

```
T1 (Rust) → cargo check + cargo test → commit
T4 (TS)   → npx tsc → 升版 → npm publish → 清缓存 → commit+push
```

## 四、Commit 策略

| Commit | 文件 | 说明 |
|--------|------|------|
| C1 | `memory.rs` | fix: session_ingest 添加 Profile V2 归纳触发 |
| C2 | `tools.ts` | feat(plugin): 新增 memory_profile_stats tool |

## 五、约束

- ❌ 不修改 profile_v2 模块（Phase 6a 代码不动）
- ❌ 不修改已完成 T2/T3 的代码（client.ts / hooks.ts）
- ❌ 不使用 console.log
- ✅ ESM imports 带 `.js` 扩展名
- ✅ 三端飞升部署流程

## 六、验证清单

- [ ] T1: `cargo check` 0e0w
- [ ] T1: `cargo test -p omem-server` profile_v2 24 测试通过
- [ ] T4: `npx tsc` 0 error
- [ ] Plugin 部署: npm publish + 清缓存
- [ ] 明镜评审 APPROVE
