use crate::domain::error::OmemError;
use crate::domain::memory::Memory;
use crate::domain::relation::RelationType;
use crate::embed::EmbedService;
use crate::ingest::noise::cosine_similarity;
use crate::ingest::refine_prompt::{build_refine_prompt, RefineInput, RefineOutput};
use crate::llm::{complete_json, LlmService};
use crate::store::lancedb::LanceStore;
use std::collections::HashSet;
use std::sync::Arc;

/// LLM精炼输入最大字符数
const MAX_INPUT_CHARS: usize = 8000;
/// 精炼时最多取的旧记忆条数
const MAX_CHAIN_FOR_REFINE: usize = 3;
/// 单条旧记忆最大字符数（超过截断）
const MAX_SINGLE_MEMORY_CHARS: usize = 3000;
/// 精炼后全文最大字符数（超限截断）
const MAX_REFINED_CONTENT_CHARS: usize = 3000;

/// BFS遍历Continues/ContinuedBy relation链，收集链上所有Memory实体（含root）
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

/// 用topic的l0_abstract做embedding，搜索同session_id的WORK记忆
/// cosine > 0.72 且 session_id匹配 → 返回最相似的
pub async fn find_similar_work_memory(
    store: &LanceStore,
    embed: &Arc<dyn EmbedService>,
    topic_l0: &str,
    session_id: &str,
    _tenant_id: &str,
) -> Result<Option<Memory>, OmemError> {
    let vectors = embed
        .embed(&[topic_l0.to_string()])
        .await
        .map_err(|e| OmemError::Embedding(format!("embed failed: {e}")))?;
    let query_vector = match vectors.into_iter().next() {
        Some(v) => v,
        None => return Ok(None),
    };

    let session_memories = store.find_memories_by_session_id(session_id, 100).await?;

    // MemoryType has no Work variant; session_ingest distinguishes via scope
    let work_memories: Vec<Memory> = session_memories
        .into_iter()
        .filter(|m| m.scope != "private")
        .collect();

    let mut best: Option<(Memory, f32)> = None;
    for m in &work_memories {
        let mem_vector = match store.get_vector_by_id(&m.id).await? {
            Some(v) => v,
            None => continue,
        };
        let score = cosine_similarity(&query_vector, &mem_vector);
        if score > 0.72 {
            if best.as_ref().map_or(true, |(_, prev)| score > *prev) {
                best = Some((m.clone(), score));
            }
        }
    }

    Ok(best.map(|(m, _)| m))
}

/// 调LLM精炼，存结果，物理删除旧记忆
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
        "session_ingest: starting WORK refine"
    );

    let mut contents: Vec<String> = chain_memories
        .iter()
        .rev()
        .take(MAX_CHAIN_FOR_REFINE)
        .map(|m| {
            if m.content.chars().count() > MAX_SINGLE_MEMORY_CHARS {
                let truncated: String = m.content.chars().take(MAX_SINGLE_MEMORY_CHARS).collect();
                format!("{truncated}...")
            } else {
                m.content.clone()
            }
        })
        .collect();
    contents.reverse();

    let total_chars: usize = contents.iter().map(|c| c.chars().count()).sum();
    if total_chars > MAX_INPUT_CHARS {
        if let Some(latest) = chain_memories.last() {
            contents = vec![if latest.content.chars().count() > MAX_SINGLE_MEMORY_CHARS {
                let truncated: String = latest.content.chars().take(MAX_SINGLE_MEMORY_CHARS).collect();
                format!("{truncated}...")
            } else {
                latest.content.clone()
            }];
        }
    }

    let input = RefineInput {
        existing_contents: contents,
        new_fact: new_fact.to_string(),
        topic: topic.to_string(),
    };

    let (system, user) = build_refine_prompt(&input);

    let refined: RefineOutput = complete_json(&**llm, &system, &user).await?;

    let refined_content = truncate_at_sentence_boundary(&refined.refined_content, MAX_REFINED_CONTENT_CHARS);
    let l1_overview = truncate_at_sentence_boundary(&refined.l1_overview, 150);
    let l2_content = truncate_at_sentence_boundary(&refined.l2_content, 300);
    let best_tier_str = chain_memories
        .iter()
        .map(|m| m.tier.to_string())
        .max_by_key(|t| tier_priority(t))
        .unwrap_or_else(|| "peripheral".to_string());
    let inherited_tier = best_tier_str
        .parse()
        .unwrap_or_else(|_| crate::domain::types::Tier::Peripheral);

    let inherited_importance = chain_memories
        .iter()
        .map(|m| m.importance)
        .fold(0.5f32, f32::max);

    let mut inherited_tags: Vec<String> = chain_memories
        .iter()
        .flat_map(|m| m.tags.clone())
        .collect();
    inherited_tags.sort();
    inherited_tags.dedup();

    let mut new_memory = root_memory.clone();
    new_memory.id = uuid::Uuid::new_v4().to_string();
    new_memory.content = refined_content;
    new_memory.l0_abstract = refined.l0_abstract;
    new_memory.l1_overview = l1_overview;
    new_memory.l2_content = l2_content;
    new_memory.tier = inherited_tier;
    new_memory.importance = inherited_importance;
    new_memory.tags = inherited_tags;
    new_memory.relations = Vec::new();
    new_memory.superseded_by = None;
    new_memory.updated_at = chrono::Utc::now().to_rfc3339();

    let vectors = embed
        .embed(&[new_memory.content.clone()])
        .await
        .map_err(|e| OmemError::Embedding(format!("embed failed: {e}")))?;
    let vector = vectors.into_iter().next();

    if let Some(ref v) = vector {
        store.create(&new_memory, Some(v)).await?;
    } else {
        store.create(&new_memory, None).await?;
    }

    tracing::info!(
        new_content_len = new_memory.content.chars().count(),
        deleted_count = chain_memories.len(),
        "session_ingest: WORK refine completed, replacing old memories"
    );

    let old_ids: Vec<String> = chain_memories.iter().map(|m| m.id.clone()).collect();

    if !old_ids.is_empty() {
        store.batch_hard_delete_by_ids(&old_ids).await?;
    }

    Ok(new_memory)
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
}
