# Session Ingest v2.0 - Decisions

## 2026-05-05 Plan Approval
- 师尊批准计划，由玄机(oracle)做技术review
- 师尊不看Rust代码，只要结果
- OOM防护是最高优先级（每次改版必出OOM）
- 情感私密暂不改核心逻辑，只加标签分类
- PREFERENCE不创建独立记忆，走profile注入
- 归簇先按session_id再按语义
- 簇断裂(memory_count=0)本次一起修
