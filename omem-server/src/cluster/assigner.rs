use std::sync::Arc;
use tracing::warn;

use crate::cluster::cluster_store::ClusterStore;
use crate::domain::error::OmemError;
use crate::domain::memory::Memory;
use crate::embed::EmbedService;
use crate::llm::{complete_json, LlmService};

const DEFAULT_SIMILARITY_THRESHOLD: f32 = 0.55;
const DEFAULT_AUTO_MERGE_THRESHOLD: f32 = 0.90;
const DEFAULT_CANDIDATE_COUNT: usize = 15;

pub struct ClusterAssigner {
    cluster_store: Arc<ClusterStore>,
    embed: Arc<dyn EmbedService>,
    llm: Option<Arc<dyn LlmService>>,
    lance_store: Option<Arc<crate::store::lancedb::LanceStore>>,
    similarity_threshold: f32,
    auto_merge_threshold: f32,
    candidate_count: usize,
    llm_judge_enabled: bool,
}

#[derive(Debug, Clone)]
pub struct AssignResult {
    pub cluster_id: Option<String>,
    pub confidence: f32,
    pub action: AssignAction,
}

#[derive(Debug, Clone)]
pub enum AssignAction {
    AutoAssign,
    LlmJudge,
    CreateNew,
}

impl ClusterAssigner {
    pub fn new(
        cluster_store: Arc<ClusterStore>,
        embed: Arc<dyn EmbedService>,
    ) -> Self {
        Self {
            cluster_store,
            embed,
            llm: None,
            lance_store: None,
            similarity_threshold: Self::get_env_threshold("OMEM_CLUSTER_SIMILARITY_THRESHOLD", DEFAULT_SIMILARITY_THRESHOLD),
            auto_merge_threshold: Self::get_env_threshold("OMEM_CLUSTER_AUTO_MERGE_THRESHOLD", DEFAULT_AUTO_MERGE_THRESHOLD),
            candidate_count: std::env::var("OMEM_CLUSTER_CANDIDATE_COUNT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(DEFAULT_CANDIDATE_COUNT),
            llm_judge_enabled: std::env::var("OMEM_CLUSTER_LLM_JUDGE")
                .map(|v| v != "false")
                .unwrap_or(true),
        }
    }

    fn get_env_threshold(name: &str, default: f32) -> f32 {
        std::env::var(name)
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(default)
    }

    pub fn with_llm(mut self, llm: Arc<dyn LlmService>) -> Self {
        self.llm = Some(llm);
        self
    }

    pub fn with_lance_store(mut self, store: Option<Arc<crate::store::lancedb::LanceStore>>) -> Self {
        self.lance_store = store;
        self
    }

    pub async fn assign(&self, memory: &Memory) -> Result<AssignResult, OmemError> {
        if let Some(ref session_id) = memory.session_id {
            if !session_id.is_empty() {
                if let Some(ref lance_store) = self.lance_store {
                    match lance_store.find_memories_by_session_id(session_id, 5).await {
                        Ok(session_memories) => {
                            if let Some(first_memory) = session_memories.first() {
                                if let Some(ref cluster_id) = first_memory.cluster_id {
                                    if !cluster_id.is_empty() {
                                        return Ok(AssignResult {
                                            action: AssignAction::AutoAssign,
                                            cluster_id: Some(cluster_id.clone()),
                                            confidence: 1.0,
                                        });
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, session_id, "session_id优先匹配失败，继续向量搜索");
                        }
                    }
                }
            }
        }

        if memory.cluster_id.is_some() {
            return Ok(AssignResult {
                cluster_id: memory.cluster_id.clone(),
                confidence: 1.0,
                action: AssignAction::AutoAssign,
            });
        }

        let candidates = self.find_candidates(memory).await?;

        if candidates.is_empty() {
            return Ok(AssignResult {
                cluster_id: None,
                confidence: 0.0,
                action: AssignAction::CreateNew,
            });
        }

        let best = &candidates[0];

        if best.similarity >= self.auto_merge_threshold {
            return Ok(AssignResult {
                cluster_id: Some(best.cluster_id.clone()),
                confidence: best.similarity,
                action: AssignAction::AutoAssign,
            });
        }

        if best.similarity >= self.similarity_threshold {
            if self.llm_judge_enabled && self.llm.is_some() {
                return self.llm_judge(memory, &candidates).await;
            }
            return Ok(AssignResult {
                cluster_id: Some(best.cluster_id.clone()),
                confidence: best.similarity,
                action: AssignAction::AutoAssign,
            });
        }

        Ok(AssignResult {
            cluster_id: None,
            confidence: best.similarity,
            action: AssignAction::CreateNew,
        })
    }

    async fn find_candidates(
        &self,
        memory: &Memory,
    ) -> Result<Vec<ClusterCandidate>, OmemError> {
        let embedding = self.embed.embed(&[memory.content.clone()]).await?;
        let vector = embedding.first().ok_or_else(|| {
            OmemError::Storage("failed to generate embedding".to_string())
        })?;

        let space_id = Some(memory.space_id.as_str());
        let clusters = self
            .cluster_store
            .search_by_vector(vector, self.candidate_count, space_id)
            .await?;

        let mut candidates: Vec<ClusterCandidate> = clusters
            .into_iter()
            .map(|(cluster, score)| ClusterCandidate {
                cluster_id: cluster.id,
                title: cluster.title,
                summary: cluster.summary,
                keywords: cluster.keywords,
                similarity: score,
            })
            .collect();

        candidates.sort_by(|a, b| b.similarity.partial_cmp(&a.similarity).unwrap_or(std::cmp::Ordering::Equal));
        Ok(candidates)
    }

    async fn llm_judge(
        &self,
        memory: &Memory,
        candidates: &[ClusterCandidate],
    ) -> Result<AssignResult, OmemError> {
        let llm = self.llm.as_ref().ok_or_else(|| {
            OmemError::Internal("LLM not configured for cluster judge".to_string())
        })?;

        let prompt = format!(
            r#"判断新记忆是否属于以下某个已有记忆簇。

新记忆内容: {}
新记忆标签: {:?}

候选簇:
{}

请返回JSON: {{"match": "cluster_id或null", "reason": "判断理由", "confidence": 0.0-1.0}}"#,
            memory.content,
            memory.tags,
            candidates
                .iter()
                .enumerate()
                .map(|(i, c)| format!(
                    "簇{}[{}]：{}\n  摘要：{}\n  关键词：{:?}",
                    i + 1,
                    c.cluster_id,
                    c.title,
                    c.summary,
                    c.keywords
                ))
                .collect::<Vec<_>>()
                .join("\n")
        );

        let system = "你是一个记忆聚类助手，判断新记忆是否属于已有记忆簇。只返回JSON格式。";

        match complete_json::<LlmJudgeResult>(llm.as_ref(), system, &prompt).await {
            Ok(result) => {
                let has_match = result.match_id.is_some();
                if result.confidence >= self.similarity_threshold {
                    Ok(AssignResult {
                        cluster_id: result.match_id,
                        confidence: result.confidence,
                        action: if has_match {
                            AssignAction::AutoAssign
                        } else {
                            AssignAction::CreateNew
                        },
                    })
                } else {
                    Ok(AssignResult {
                        cluster_id: None,
                        confidence: result.confidence,
                        action: AssignAction::CreateNew,
                    })
                }
            }
            Err(e) => {
                warn!("LLM judge failed, fallback to best candidate: {}", e);
                Ok(AssignResult {
                    cluster_id: Some(candidates[0].cluster_id.clone()),
                    confidence: candidates[0].similarity,
                    action: AssignAction::AutoAssign,
                })
            }
        }
    }
}

#[derive(Debug, Clone)]
struct ClusterCandidate {
    cluster_id: String,
    title: String,
    summary: String,
    keywords: Vec<String>,
    similarity: f32,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct LlmJudgeResult {
    #[serde(rename = "match")]
    match_id: Option<String>,
    #[allow(dead_code)]
    reason: String,
    confidence: f32,
}
