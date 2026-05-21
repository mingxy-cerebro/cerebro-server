## Decisions

### 2026-05-21
- D1: 先同步 src/index.ts 到 dist 版本（T0），再实施异步缓存（T1）
- D2: 不回退 system.transform，保留 chat.message 注入路径但让它不阻塞
- D3: 异步缓存设计：同步读缓存注入 output.parts + fire-and-forget 后台获取写缓存
