# Phase 4: Cluster 模块 project_path 感知

## 背景
Phase 3 数据治理已完成：477条记忆中，NF 184条、OMEM 269条、全局 24条。
Retrieve 模块已完整支持 project_path 过滤（handler → pipeline → lancedb 全链路已打通）。

**Cluster 模块完全不知道 project_path 的存在**——归簇时 NF 和 OMEM 记忆混在一起。

## 设计决策

### ❌ 错误思路：按 project_path 分组归簇
如果按 project_path 分3组（NF/OMEM/全局）各自做 K-Means，只有3个簇，丧失了语义细分的意义。全局24条记忆更聚不出来。

### ✅ 正确思路：project_path 作为簇的标签 + 匹配过滤维度
- **归簇时**：仍然按语义（向量相似度）聚出几十个簇，不做分组。但每个簇**继承 anchor memory 的 project_path 作为标签**
- **匹配时**：`ClusterAssigner` 搜索候选簇时，**只匹配同 project_path 的簇**（不同项目的记忆不会分到对方的簇）
- **召回时**：不需要改（retrieve 已经按 project_path 过滤了）

这样每个项目内部仍然有语义细分的多个簇（如 NF 里有"Maven编译簇"、"WSL配置簇"等），只是跨项目的记忆不会混簇。

## 诊断结果

### Retrieve 模块 ✅ 已完整支持（无需改动）
- `memory.rs` search_memories → `project_path_filter` 已传给 pipeline ✅
- `session_recalls.rs` should_recall → `project_path_filter` 已传给 pipeline ✅
- `pipeline.rs` → `vector_search` + `fts_search` 都正确传参 ✅
- `lancedb.rs` WHERE 子句正确过滤 ✅

### Cluster 模块 ❌ 需要改动
1. `domain/cluster.rs` — MemoryCluster 无 `project_path` 字段
2. `cluster/manager.rs` — create_cluster() 不记录 project_path
3. `cluster/assigner.rs` — find_candidates() 搜索候选簇无 project_path 过滤
4. `cluster/cluster_store.rs` — LanceDB Schema + 搜索需要加 project_path 支持
5. `cluster/background_clustering.rs` — 现有簇数据需要 backfill project_path

## TODOs

- [x] T1: MemoryCluster 加 `project_path: Option<String>` 字段
  - `domain/cluster.rs` — struct + serde(default) + new() project_path: None
  - `cluster/cluster_store.rs` — LanceDB Schema nullable列 + cluster_to_batch + row_to_cluster
  - search_by_vector 加 project_path 参数 + LanceDB only_if 组合式过滤

- [x] T2: ClusterManager::create_cluster() 从 anchor memory 继承 project_path
  - `cluster/manager.rs` — cluster.project_path = memory.project_path.clone()

- [x] T3: ClusterAssigner 候选搜索过滤 + session匹配校验 + K-Means按pp分组
  - `cluster/assigner.rs` — find_candidates() 传 project_path + session优先匹配加pp校验（3种组合全覆盖）
  - `cluster/background_clustering.rs` — 按project_path分组后分别K-Means + label_offset防冲突 + 簇继承anchor的pp

- [x] T4: 现有簇数据 backfill project_path（通过全局 K-Means 重建替代 backfill）
  - Web 端触发全局 K-Means 重建 → 旧簇被替换，新簇按 project_path 分组
  - 结果：35 簇（pp=omem-server-source 17个, pp=nf 7个, pp=NULL 11个），50 members

- [x] T5: 编译验证 + 部署
  - `cargo build --release -p omem-server` ✅ 零 error
  - 部署到 47.93.199.242 + health check ✅
  - Schema migration 自动执行（Adding missing columns: project_path）✅
  - 增量归簇验证 ✅ (processed=10, assigned=9, created=1, errors=0)
  - 全局 K-Means 重建验证 ✅ (35 clusters, pp 分布正确)

## 不改的文件
- `retrieve/` 模块所有文件（已支持）
- `kmeans.rs`（纯数学算法，不受影响）
- `background_clustering.rs` 的归簇分组逻辑（仍然全量归簇，不按 project_path 分组）
- `api/handlers/` 所有文件（归簇不经过 HTTP handler）
- `aggregator.rs`（聚合结果自然按簇的 project_path 分属，无需特殊处理）

## 关键约束
- **project_path=NULL 的记忆**只能匹配 project_path=NULL 的簇（全局偏好聚在一起）
- **跨项目不混簇**：NF 记忆永远不进 OMEM 的簇，反之亦然
- **向后兼容**：新增 `Option<String>` 字段，现有簇 project_path=NULL，不影响存量
