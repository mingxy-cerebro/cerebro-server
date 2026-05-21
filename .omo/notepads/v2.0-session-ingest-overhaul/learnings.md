# Learnings

## 2026-05-05 Wave 1-3 Execution

### Critical Patterns
- `cargo check` 是快速验证工具，`cargo test` 在无LLM/embedding环境下部分test会skip
- LanceDB的SQL expression (如 `col + 1`) 不工作，必须用 read-modify-write 模式
- assigner.rs (line 78-102) 已有session_id优先逻辑：通过find_memories_by_session_id查同session记忆取cluster_id
- background_clustering.rs 全量K-Means会删除所有簇再重建，这是破坏性的
- LifecycleScheduler只做孤儿清理和decay，不触发归簇
- anchor记忆的cluster_id从未被设置 → 导致memory_count全为0

### Code Conventions
- 所有store查询必须有limit参数
- ProfileService需要store+profile_cache+tenant_id三件套
- 追加逻辑用 `## {YYYY-MM-DD}` 日期分隔
- PREFERENCE用continue跳过store.create()
- Confidence默认0.7，cleanup阈值0.3

### Key Files Modified
- `prompts.rs`: +149/-10 (SESSION_EXTRACT三分类+新签名)
- `profile/service.rs`: +322/-1 (写入方法+静动态画像)
- `store/lancedb.rs`: +116 (OOM安全session查询)
- `domain/profile.rs`: +103 (ProfileFact结构体)
- `domain/memory.rs`: +20 (MemoryDigest+SessionMemorySummary)
- `handlers/memory.rs`: +185/-29 (handler全改造)
- `cluster/cluster_store.rs`: +82/-21 (read-modify-write)
- `cluster/background_clustering.rs`: +6 (anchor cluster_id设置)

### OOM Guards
- list_memories_by_session带limit
- get_memory_summary不返回完整content
- count_memories_by_session用count_rows不加载到内存
- build_merged_summary截断2000字符
