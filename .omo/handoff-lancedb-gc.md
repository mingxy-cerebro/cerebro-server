# Handoff: LanceDB精准Orphan GC

> 写于 2026-05-08, 月儿交班文档

## 一句话总结
把 `maybe_rebuild_on_write()` 从核弹级(drop全部索引)改为精准外科手术(只删orphan UUID目录，保留活跃索引)。

## 当前问题
`maybe_rebuild_on_write()` 每次 GC 会 drop **全部**索引→重建期间 FTS 缺失→查询退化全表扫描→超时 3.6s。
师尊要求：只删 orphan 碎片 UUID 目录，保留活跃索引，零空窗。

## 需要改的文件
**仅一个文件**: `omem-server/src/store/lancedb.rs`

### 改动点 1: 获取活跃索引 UUID (2138-2150行)

**当前代码(编译错误)**:
```rust
// 2138-2146: list_indices() 获取 active_indices
let active_indices = table.list_indices().await.unwrap_or_default();
// 2147-2150: 用 idx.uuid — 编译错误！IndexConfig 无 uuid 字段
let active_uuids: HashSet<String> = active_indices
    .iter()
    .map(|idx| idx.uuid.to_string())  // ❌ IndexConfig 没有 uuid
    .collect();
```

**改为(两种方案选一)**:

#### 方案A: 用 table.dataset() → load_indices() (推荐，不加依赖)
```rust
// 在 spawn 内，table 已 clone 进来
let wrapper = table.dataset().expect("native table")
    .clone();  // DatasetConsistencyWrapper 实现了 Clone
let dataset = wrapper.get().await
    .map_err(|e| { /* log error */ })?;
let indices = dataset.load_indices().await
    .map_err(|e| { /* log error */ })?;
let active_uuids: HashSet<String> = indices
    .iter()
    .map(|idx| idx.uuid.to_string())
    .collect();
```

**坑**: `load_indices()` 是 trait method，需要 trait 在 scope 内。检查 `lance::Dataset` 是否已导入或需要加 `use lance::Dataset;`。

#### 方案B: 用 read_manifest_indexes (玄机推荐，需加依赖)
```rust
// Cargo.toml 加: lance-table = "=3.0.1"
// 文件头加: use lance_table::io::manifest::read_manifest_indexes;
let wrapper = table.dataset().expect("native table").clone();
let dataset = wrapper.get().await.map_err(|e| { /* log */ })?;
let indices = read_manifest_indexes(
    dataset.object_store(),
    dataset.manifest_location(),
    dataset.manifest(),
).await.map_err(|e| { /* log */ })?;
let active_uuids: HashSet<String> = indices
    .iter()
    .map(|idx| idx.uuid.to_string())
    .collect();
```

### 改动点 2: 删除重复 reset (2198-2201行)
当前有两处重复的 reset，删掉其中一对：
```rust
// 2198-2199 是重复的，删掉
// 2200-2201 是正确的，保留
wc.store(0, Ordering::Relaxed);
rb.store(false, Ordering::Release);
```

### 改动点 3: 删掉核弹级 drop 所有索引的代码
当前 spawn 里有 `drop_index(name)` 全删的逻辑，**全部删掉**，替换为上面的精准清理：
- 删掉 Step1(list_indices + drop_all)
- 删掉 Step2(prune) — 精准清理不需要 prune
- **保留 Step3 的磁盘级 orphan 清理逻辑**，但改用 `active_uuids` 差集

### 改动点 4: 磁盘 orphan 清理逻辑(2164-2190行)改为差集
```rust
// 原来是全部删除，改为差集：只删不在 active_uuids 里的
let indices_dir = format!("{}/memories.lance/_indices", uri);
if let Ok(mut entries) = tokio::fs::read_dir(&indices_dir).await {
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().to_string_lossy().to_string();
        if !active_uuids.contains(&name) {
            if let Err(e) = tokio::fs::remove_dir_all(entry.path()).await {
                // log warn, 不阻塞
            }
        }
    }
}
```

## 完整的 maybe_rebuild_on_write() 改后伪代码

```rust
fn maybe_rebuild_on_write(&self) {
    let wc = self.write_count.clone();
    let rb = self.rebuilding.clone();
    
    // 原子检查+重置
    let count = wc.fetch_add(1, Ordering::Relaxed) + 1;
    if count < 200 { return; }
    if rb.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err() { return; }
    wc.store(0, Ordering::Relaxed);
    
    let table = self.table.clone();
    let uri = self.uri.clone();
    
    tokio::spawn(async move {
        // 终于 reset guard，无论成功失败
        let _guard = scope_guard(|| { rb.store(false, Ordering::Release); });
        
        // Step 1: 获取活跃索引 UUID
        let wrapper = match table.dataset() {
            Some(w) => w.clone(),
            None => { log::warn!("no dataset"); return; }
        };
        let dataset = match wrapper.get().await {
            Ok(d) => d,
            Err(e) => { log::warn!("dataset get failed: {}", e); return; }
        };
        let active_uuids: HashSet<String> = match dataset.load_indices().await {
            Ok(indices) => indices.iter().map(|i| i.uuid.to_string()).collect(),
            Err(e) => { log::warn!("load_indices failed: {}", e); return; }
        };
        
        log::info!("orphan GC: {} active indices", active_uuids.len());
        
        // Step 2: 磁盘级差集清理
        let indices_dir = format!("{}/memories.lance/_indices", uri);
        if let Ok(mut entries) = tokio::fs::read_dir(&indices_dir).await {
            let mut cleaned = 0u32;
            while let Ok(Some(entry)) = entries.next_entry().await {
                let name = entry.file_name().to_string_lossy().to_string();
                if !active_uuids.contains(&name) {
                    match tokio::fs::remove_dir_all(entry.path()).await {
                        Ok(()) => cleaned += 1,
                        Err(e) => log::warn!("remove_dir failed: {}", e),
                    }
                }
            }
            log::info!("orphan GC cleaned {} uuid dirs", cleaned);
        }
        // _guard drop → rb.store(false)
    });
}
```

**注意**: 需要确认 `IndexMetadata.uuid` 的类型。如果是 `Uuid` 类型，`.to_string()` 会生成标准 UUID 字符串。磁盘上 `_indices/` 下的 UUID 目录名也是标准格式，应该能匹配。

## 编译相关
- `cargo check` 验证
- `cargo build --release` 构建 (~3分钟)
- 如果方案B需要在 `omem-server/Cargo.toml` 加 `lance-table = "=3.0.1"`

## 部署
```bash
# 本地
cargo build --release
md5sum target/release/omem-server
scp target/release/omem-server root@47.93.199.242:/opt/omem/omem-server-new

# 服务器
ssh root@47.93.199.242
md5sum /opt/omem/omem-server-new
mv /opt/omem/omem-server-new /opt/omem/omem-server
systemctl restart omem
curl localhost:8080/health
ls /opt/omem/omem-data/personal/c60beb98-7aab-4985-8c1d-29ffd6aff75a/memories.lance/_indices/ | wc -l
```

## 服务器信息
- SSH: `ssh root@47.93.199.242`
- 数据: `/opt/omem/omem-data/`
- 二进制: `/opt/omem/omem-server`
- 备份: `/opt/omem/omem-data.bak.202605081545` (144M)
- 旧binary: `/opt/omem/omem-server.bak.gc`
- 服务: `systemctl restart omem`
- API Key: `c60beb98-7aab-4985-8c1d-29ffd6aff75a`
- 当前PID: 493890

## Git 历史
- 生命周期配置化: `4fcf952`
- 插件 agent_id: `8f5aee1`, `@mingxy/cerebro@1.8.3`
- LanceDB 根治(L1+L2+L3): `0200332`
- GC 计数器(核弹版): `10f6a62` + `d79e0e0`
- 当前未提交: 精准orphan GC改动(待实现)
- 偏好提取遗漏P2备忘OMEM ID: `32dbc182`

## 已验证的 API 路径
```
table.dataset() → Option<&DatasetConsistencyWrapper>
  .clone() → DatasetConsistencyWrapper (#[derive(Debug, Clone)])
  .get().await → Result<Arc<Dataset>>
  Dataset.load_indices().await → Result<Arc<Vec<IndexMetadata>>>
  IndexMetadata.uuid → Uuid (lance-table crate)
```

## 已踩过的坑(别再踩！)
1. `IndexConfig`(lancedb) 无 uuid → 必须走 `Dataset.load_indices()` 或 `read_manifest_indexes()`
2. `table.dataset()` 返回引用 → 需要 `.clone()` DatasetConsistencyWrapper
3. `load_indices()` 是 async → 需要 `.await` 在 spawn 内
4. `as_native()` deprecated → 直接用 `table.dataset()` 公开API
5. 2198-2201重复reset → 删一对

## 核心原则(师尊原话)
- "先调研再动手！看你这么删代码，吓得我肝颤啊~~"
- "要稳，切记"
- "你要保证根治呀~"
- **精准外科手术**：只删orphan，不碰活跃索引
