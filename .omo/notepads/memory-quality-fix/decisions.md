# Decisions - memory-quality-fix

## [2026-05-10] Execution Strategy
- Wave 1: T1+T2+T3 in parallel (reconciler.rs, memory.rs, prompts.rs - no file conflicts)
- Wave 2: T4 sequential (build+test+deploy after wave 1)
- Final: F1-F4 parallel reviews
