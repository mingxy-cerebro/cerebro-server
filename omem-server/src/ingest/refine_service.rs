use crate::domain::error::OmemError;
use crate::domain::memory::Memory;
use crate::domain::relation::RelationType;
use crate::embed::EmbedService;
use crate::ingest::refine_prompt::{build_refine_prompt, RefineInput, RefineOutput};
use crate::llm::{complete_json, LlmService};
use crate::store::lancedb::LanceStore;
use std::collections::HashSet;
use std::sync::Arc;

/// 单条旧记忆最大字符数（超过截断或触发split）
pub const MAX_SINGLE_MEMORY_CHARS: usize = 3000;
/// BFS遍历Continues/ContinuedBy relation链，收集链上所有Memory实体（含root）
#[deprecated(note = "session_ingest REFINE path removed; chain collection no longer needed. Kept for reference.")]
pub async fn collect_chain_memories(
    store: &LanceStore,
    root_memory: &Memory,
) -> Result<Vec<Memory>, OmemError> {
    let mut result = Vec::new();
    let mut visited = HashSet::new();
    let mut queue = vec![(root_memory.clone(), 0usize)];

    while let Some((memory, depth)) = queue.pop() {
        if depth > 5 {
            continue;
        }
        if visited.contains(&memory.id) {
            continue;
        }
        visited.insert(memory.id.clone());

        for rel in &memory.relations {
            if matches!(
                rel.relation_type,
                RelationType::Continues | RelationType::ContinuedBy
            ) {
                if !visited.contains(&rel.target_id) {
                    if let Some(target) = store.get_by_id(&rel.target_id).await? {
                        queue.push((target, depth + 1));
                    }
                }
            }
        }

        result.push(memory);
    }

    Ok(result)
}

/// Walk ContinuedBy relations to find the tail of a memory chain.
/// If the memory has no ContinuedBy relations, returns it as-is.
/// Follows the most recent child if multiple ContinuedBy exist.
pub async fn walk_to_chain_tail(
    store: &LanceStore,
    start: &Memory,
) -> Memory {
    let mut current = start.clone();
    let mut visited = std::collections::HashSet::new();
    visited.insert(current.id.clone());

    loop {
        let continued_by_targets: Vec<String> = current
            .relations
            .iter()
            .filter(|r| r.relation_type == RelationType::ContinuedBy)
            .map(|r| r.target_id.clone())
            .collect();

        if continued_by_targets.is_empty() {
            return current;
        }

        let next_id = match continued_by_targets.into_iter().next() {
            Some(id) => id,
            None => return current,
        };

        if visited.contains(&next_id) {
            tracing::warn!(
                current_id = %current.id,
                next_id = %next_id,
                "walk_to_chain_tail: detected cycle, stopping"
            );
            return current;
        }
        visited.insert(next_id.clone());

        match store.get_by_id(&next_id).await {
            Ok(Some(next_mem)) => {
                tracing::debug!(
                    from_id = %current.id,
                    to_id = %next_id,
                    "walk_to_chain_tail: walking to child"
                );
                current = next_mem;
            }
            Ok(None) => {
                tracing::warn!(
                    parent_id = %current.id,
                    child_id = %next_id,
                    "walk_to_chain_tail: ContinuedBy target not found, using current"
                );
                return current;
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    parent_id = %current.id,
                    "walk_to_chain_tail: failed to load child, using current"
                );
                return current;
            }
        }
    }
}

/// 刀3(docs/memory-dedup SPEC):新建 WORK 记忆前的跨 session 向量防线。
/// 调用方同 session 的段头匹配已全部 miss 时,用 topic 向量在同 project_path
/// 的大事记记忆里做 cosine 检索,超阈值返回旧链链尾作为追加目标——段头匹配
/// 是字面级,LLM 给旧话题起新名时 miss;向量是语义级,补这层。
/// 仅 session_ingest 源、非 private;跨项目靠 project_path_filter 的双向
/// 前缀匹配天然隔离(农服的积分进不了 omem 的候选池)。
/// ponytail: limit=20 候选靠内存过滤缩到 WORK 大事记,池子大到漏召回再调。
pub async fn find_cross_session_work_tail(
    store: &LanceStore,
    query_vector: &[f32],
    project_path: &str,
    min_score: f32,
) -> Result<Option<(Memory, f32)>, OmemError> {
    // project_path_filter 自带 `OR visibility='private'` 旁路,内存侧必须重滤 private
    let hits = store
        .vector_search(
            query_vector,
            20,
            min_score,
            None,
            None,
            None,
            None,
            Some(project_path),
            None,
        )
        .await?;
    for (m, score) in hits {
        if m.scope != "private" && m.source.as_deref() == Some("session_ingest") {
            tracing::info!(
                matched_id = %m.id,
                score = score,
                "cross_session_dedup: embedding hit, walking to chain tail"
            );
            return Ok(Some((walk_to_chain_tail(store, &m).await, score)));
        }
    }
    Ok(None)
}

/// 刀3 灰度开关:默认关,`OMEM_DEDUP_MERGE=1|true` 启用。先关后开,生产观察
/// 误合并率满意后再考虑翻默认(可回滚铁律)。
pub fn dedup_merge_enabled() -> bool {
    std::env::var("OMEM_DEDUP_MERGE")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

/// 刀3 cosine 阈值:默认 0.72(SPEC 定),`OMEM_DEDUP_COSINE` 可调。
pub fn dedup_cosine_threshold() -> f32 {
    clamp_threshold(std::env::var("OMEM_DEDUP_COSINE").ok().as_deref())
}

/// 纯函数:阈值解析+夹取 [0.5, 0.99],防手滑写个 0.1 把整库缝成一条。
fn clamp_threshold(raw: Option<&str>) -> f32 {
    raw.and_then(|s| s.parse::<f32>().ok())
        .unwrap_or(0.72)
        .clamp(0.5, 0.99)
}

/// 调LLM精炼，原地update目标记忆（id不变，保留relations）
/// 
/// 只精炼链尾（root_memory自身），不合并整条链。
/// 精炼后直接update原记忆，不删旧建新，保留所有关联。
#[deprecated(note = "session_ingest REFINE path removed; in-place refine no longer needed. Kept for reference.")]
pub async fn refine_and_replace(
    store: &LanceStore,
    llm: &Arc<dyn LlmService>,
    embed: &Arc<dyn EmbedService>,
    root_memory: &Memory,
    chain_memories: &[Memory],
    new_fact: &str,
    topic: &str,
) -> Result<Memory, OmemError> {
    tracing::info!(
        topic = %topic,
        chain_len = chain_memories.len(),
        new_fact_len = new_fact.chars().count(),
        "session_ingest: starting WORK refine (in-place update)"
    );

    let existing_content = if root_memory.content.chars().count() > MAX_SINGLE_MEMORY_CHARS {
        let truncated: String = root_memory.content.chars().take(MAX_SINGLE_MEMORY_CHARS).collect();
        format!("{truncated}...")
    } else {
        root_memory.content.clone()
    };

    let input = RefineInput {
        existing_contents: vec![existing_content],
        new_fact: new_fact.to_string(),
        topic: topic.to_string(),
    };

    let (system, user) = build_refine_prompt(&input);

    let refined: RefineOutput = complete_json(&**llm, &system, &user).await?;

    let refined_content = refined.refined_content;
    let l1_overview = truncate_at_sentence_boundary(&refined.l1_overview, 150);
    let l2_content = truncate_at_sentence_boundary(&refined.l2_content, 300);

    let best_tier_str = chain_memories
        .iter()
        .map(|m| m.tier.to_string())
        .max_by_key(|t| tier_priority(t))
        .unwrap_or_else(|| root_memory.tier.to_string());
    let inherited_tier = best_tier_str
        .parse()
        .unwrap_or(root_memory.tier.clone());

    let inherited_importance = chain_memories
        .iter()
        .map(|m| m.importance)
        .fold(root_memory.importance, f32::max);

    let mut inherited_tags: Vec<String> = chain_memories
        .iter()
        .flat_map(|m| m.tags.clone())
        .collect();
    inherited_tags.sort();
    inherited_tags.dedup();

    let mut updated = root_memory.clone();
    updated.content = refined_content;
    updated.l0_abstract = refined.l0_abstract;
    updated.l1_overview = l1_overview;
    updated.l2_content = l2_content;
    updated.tier = inherited_tier;
    updated.importance = inherited_importance;
    updated.tags = inherited_tags;
    updated.updated_at = chrono::Utc::now().to_rfc3339();

    // If content exceeds 3000 chars, do NOT embed+update here —
    // the caller will handle split, which embeds and writes each part.
    // This avoids a wasted embed+update that split_memory would immediately overwrite.
    if updated.content.chars().count() > MAX_SINGLE_MEMORY_CHARS {
        tracing::info!(
            memory_id = %updated.id,
            content_len = updated.content.chars().count(),
            "session_ingest: WORK refined content exceeds 3000, deferring to split"
        );
        return Ok(updated);
    }

    let vectors = embed
        .embed(&[updated.content.clone()])
        .await
        .map_err(|e| OmemError::Embedding(format!("embed failed: {e}")))?;
    let vector = vectors.into_iter().next();

    store.update(&updated, vector.as_deref()).await?;

    tracing::info!(
        memory_id = %updated.id,
        new_content_len = updated.content.chars().count(),
        "session_ingest: WORK refine completed (in-place update, relations preserved)"
    );

    Ok(updated)
}

/// 按句子边界截断字符串，超限强制chars().take(max_chars)
fn truncate_at_sentence_boundary(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }

    let sentence_boundaries = ['。', '！', '？', '\n'];

    let mut best_pos = None;
    for (i, ch) in s.char_indices() {
        if sentence_boundaries.contains(&ch) {
            let char_count = s[..i + ch.len_utf8()].chars().count();
            if char_count <= max_chars {
                best_pos = Some(i + ch.len_utf8());
            } else {
                break;
            }
        }
    }

    match best_pos {
        Some(pos) => format!("{}...", &s[..pos]),
        None => {
            let truncated: String = s.chars().take(max_chars).collect();
            format!("{truncated}...")
        }
    }
}

/// Tier优先级辅助（兼容现有枚举值和未来扩展）
fn tier_priority(tier: &str) -> u8 {
    match tier {
        "l3" | "core" => 4,
        "l2" | "working" => 3,
        "l1" => 2,
        "l0" => 1,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_at_sentence_boundary_short() {
        let input = "这是一段短文本";
        let result = truncate_at_sentence_boundary(input, 150);
        assert_eq!(result, input, "短文本不应被截断");
    }

    #[test]
    fn test_truncate_at_sentence_boundary_long_with_boundary() {
        let part1 = "这是一个很长的句子用于测试截断功能的边界情况".repeat(5); // 100 chars
        let part2 = "。这是句号后面的内容需要足够多的字来填充空间范围".repeat(3); // 63 chars
        let input = format!("{part1}{part2}");
        assert!(
            input.chars().count() > 150,
            "输入应超过150字: {}",
            input.chars().count()
        );

        let result = truncate_at_sentence_boundary(&input, 150);
        // 应在句号处截断并带"..."
        assert!(
            result.ends_with("..."),
            "截断结果应以...结尾: {result}"
        );
        assert!(
            result.chars().count() <= 153,
            "截断结果不应超过max_chars+3: got {}",
            result.chars().count()
        );
        // 截断处应包含句号
        let without_ellipsis = &result[..result.len() - 3];
        assert!(
            without_ellipsis.ends_with('。'),
            "截断应在句号处: {without_ellipsis}"
        );
    }

    #[test]
    fn test_truncate_at_sentence_boundary_long_no_boundary() {
        // 构造超过150字无句子边界（无。！？\n）的纯文字
        let input: String = "纯文字无标点内容填充".repeat(20); // 200 chars
        assert!(
            input.chars().count() > 150,
            "输入应超过150字: {}",
            input.chars().count()
        );
        assert!(
            !input.contains('。') && !input.contains('！') && !input.contains('？') && !input.contains('\n'),
            "输入不应包含句子边界"
        );

        let result = truncate_at_sentence_boundary(&input, 150);
        assert!(
            result.ends_with("..."),
            "截断结果应以...结尾: {result}"
        );
        // 强制截断：前150字符 + "..."
        let without_ellipsis = &result[..result.len() - 3];
        assert_eq!(
            without_ellipsis.chars().count(),
            150,
            "无边界时应精确截取150字符"
        );
    }

    #[test]
    fn test_tier_priority() {
        assert_eq!(tier_priority("core"), 4, "core 应为最高优先级 4");
        assert_eq!(tier_priority("l3"), 4, "l3 应等同于 core = 4");
        assert_eq!(tier_priority("working"), 3);
        assert_eq!(tier_priority("l2"), 3);
        assert_eq!(tier_priority("l1"), 2);
        assert_eq!(tier_priority("l0"), 1);
        assert_eq!(tier_priority("peripheral"), 0, "peripheral 应为 0");
        assert_eq!(tier_priority("unknown"), 0, "未知值应为 0");

        // 验证优先级顺序: core > working > peripheral > unknown
        assert!(tier_priority("core") > tier_priority("working"));
        assert!(tier_priority("working") > tier_priority("peripheral"));
        assert_eq!(tier_priority("peripheral"), tier_priority("unknown"));
    }

    #[test]
    fn test_clamp_threshold() {
        assert!((clamp_threshold(None) - 0.72).abs() < 1e-6, "缺省 0.72");
        assert!((clamp_threshold(Some("0.8")) - 0.8).abs() < 1e-6, "合法值原样过");
        assert!((clamp_threshold(Some("0.1")) - 0.5).abs() < 1e-6, "低于 0.5 夹到 0.5");
        assert!((clamp_threshold(Some("1.5")) - 0.99).abs() < 1e-6, "高于 0.99 夹到 0.99");
        assert!((clamp_threshold(Some("garbage")) - 0.72).abs() < 1e-6, "垃圾输入回默认");
    }

    mod cross_session {
        use super::super::*;
        use crate::domain::memory::Memory;
        use crate::domain::relation::{MemoryRelation, RelationType};
        use crate::domain::category::Category;
        use crate::domain::types::MemoryType;
        use tempfile::TempDir;

        async fn setup() -> (LanceStore, TempDir) {
            let dir = TempDir::new().expect("temp dir");
            let store = LanceStore::new(dir.path().to_str().expect("path"))
                .await
                .expect("store");
            store.init_table().await.expect("init");
            (store, dir)
        }

        fn work_memory(content: &str, project_path: &str, session_id: &str) -> Memory {
            let mut m = Memory::new(content, Category::new("events"), MemoryType::Pinned, "t");
            m.source = Some("session_ingest".to_string());
            m.session_id = Some(session_id.to_string());
            m.project_path = Some(project_path.to_string());
            m
        }

        /// e1=基准轴,query 同向 → cosine 1.0;正交记忆 → 0.0,过不了任何阈值
        fn unit_vec(first: f32) -> Vec<f32> {
            let mut v = vec![0.0f32; 1024];
            v[0] = first;
            v
        }

        #[tokio::test]
        async fn hits_same_project_work_and_walks_to_tail() {
            let (store, _dir) = setup().await;
            // 旧链:head ← ContinuedBy ← tail(query 同向,语义同话题)
            let head = work_memory("old head", "/proj", "session-old-1");
            let mut tail = work_memory("old tail", "/proj", "session-old-2");
            tail.relations = vec![MemoryRelation {
                relation_type: RelationType::Continues,
                target_id: head.id.clone(),
                context_label: Some("auto-split on overflow".to_string()),
            }];
            store.create(&head, Some(&unit_vec(0.0))).await.expect("create head");
            store.create(&tail, Some(&unit_vec(1.0))).await.expect("create tail");

            let got = find_cross_session_work_tail(&store, &unit_vec(1.0), "/proj", 0.72)
                .await
                .expect("lookup");
            let (m, score) = got.expect("should hit");
            assert_eq!(m.id, tail.id, "应返回链尾而非链头");
            assert!(score >= 0.72, "cosine 应过阈值: {score}");
        }

        #[tokio::test]
        async fn misses_other_project() {
            let (store, _dir) = setup().await;
            let other = work_memory("other project memory", "/nf", "session-old-1");
            store.create(&other, Some(&unit_vec(1.0))).await.expect("create");

            let got = find_cross_session_work_tail(&store, &unit_vec(1.0), "/omem", 0.72)
                .await
                .expect("lookup");
            assert!(got.is_none(), "跨项目不应命中");
        }

        #[tokio::test]
        async fn skips_private_and_non_ingest_sources() {
            let (store, _dir) = setup().await;
            // private:visibility 旁路能进候选,内存过滤必须拦下
            let mut private = work_memory("private memory", "/proj", "session-old-1");
            private.scope = "private".to_string();
            private.visibility = "private".to_string();
            store.create(&private, Some(&unit_vec(1.0))).await.expect("create private");
            // 非 session_ingest 源:手动建的记忆不参与大事记缝合
            let mut manual = work_memory("manual memory", "/proj", "session-old-2");
            manual.source = Some("web-create".to_string());
            store.create(&manual, Some(&unit_vec(1.0))).await.expect("create manual");

            let got = find_cross_session_work_tail(&store, &unit_vec(1.0), "/proj", 0.5)
                .await
                .expect("lookup");
            assert!(got.is_none(), "private 与非 ingest 源都不该命中");
        }
    }
}
