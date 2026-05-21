# Phase 6b Decisions

## Plan Decisions
- 砍掉 session_ingest 的 PREFERENCE 路径
- /v1/profile 路由保留做兼容代理（openclaw/mcp 仍依赖）
- Plugin 端直接用 V2 inject 返回的 content
- 仅改 opencode plugin，其他 3 个后续按需
- memory.rs 中 LLM 输出的非法 PREFERENCE fallback 到 WORK/EMOTIONAL
