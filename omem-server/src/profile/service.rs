use std::sync::Arc;

use chrono::Utc;
use serde::Deserialize;

use crate::domain::category::Category;
use crate::domain::error::OmemError;
use crate::domain::profile::UserProfile;
use crate::ingest::prompts::PROFILE_FILTER_SYSTEM_PROMPT;
use crate::lifecycle::decay::parse_datetime;
use crate::llm::LlmService;
use crate::retrieve::SearchResult;
use crate::store::lancedb::{LanceStore, ListFilter};

#[derive(Deserialize)]
struct ProfileFilterResponse {
    facts: Vec<String>,
}

pub struct ProfileResponse {
    pub profile: UserProfile,
    pub search_results: Option<Vec<SearchResult>>,
}

pub struct ProfileService {
    store: Arc<LanceStore>,
    llm: Option<Arc<dyn LlmService>>,
}

impl ProfileService {
    pub fn new(store: Arc<LanceStore>) -> Self {
        Self { store, llm: None }
    }

    pub fn with_llm(mut self, llm: Arc<dyn LlmService>) -> Self {
        self.llm = Some(llm);
        self
    }

    pub async fn get_profile(&self, query: Option<&str>) -> Result<ProfileResponse, OmemError> {
        let filter = ListFilter {
            state: Some("active".to_string()),
            sort: "created_at".to_string(),
            order: "desc".to_string(),
            ..Default::default()
        };
        let all_memories = self.store.list_filtered(&filter, 1000, 0).await?;

        // Exclude private memories from profile generation
        let all_memories: Vec<_> = all_memories
            .into_iter()
            .filter(|m| m.visibility != "private")
            .collect();

        let mut static_memories: Vec<_> = all_memories
            .iter()
            .filter(|m| m.category == Category::Profile || m.category == Category::Preferences)
            .collect();
        static_memories.sort_by(|a, b| {
            b.importance
                .partial_cmp(&a.importance)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let static_facts: Vec<String> = static_memories
            .iter()
            .take(20)
            .map(|m| sanitize_profile_content(&m.content))
            .filter(|s| !s.is_empty())
            .collect();

        // LLM过滤：去除非画像内容
        let static_facts = if !static_facts.is_empty() {
            if let Some(ref llm) = self.llm {
                match crate::llm::complete_json::<ProfileFilterResponse>(
                    &**llm,
                    PROFILE_FILTER_SYSTEM_PROMPT,
                    &format!(
                        "请从以下记忆条目中筛选出真正的用户画像信息：\n{}",
                        static_facts
                            .iter()
                            .enumerate()
                            .map(|(i, f)| format!("{}. {}", i + 1, f))
                            .collect::<Vec<_>>()
                            .join("\n")
                    ),
                )
                .await
                {
                    Ok(resp) => resp.facts,
                    Err(e) => {
                        tracing::warn!("Profile LLM filter failed, using raw facts: {}", e);
                        static_facts
                    }
                }
            } else {
                static_facts
            }
        } else {
            static_facts
        };

        let cutoff = Utc::now() - chrono::TimeDelta::try_days(7).unwrap_or_default();
        let dynamic_context: Vec<String> = all_memories
            .iter()
            .filter(|m| {
                matches!(
                    m.category,
                    Category::Events | Category::Cases | Category::Patterns
                )
                    && parse_datetime(&m.created_at)
                        .map(|dt| dt >= cutoff)
                        .unwrap_or(false)
            })
            .take(10)
            .map(|m| sanitize_profile_content(&m.content))
            .filter(|s| !s.is_empty())
            .collect();

        let profile = UserProfile {
            static_facts,
            dynamic_context,
        };

        let search_results = match query {
            Some(q) => {
                let results = self
                    .store
                    .fts_search(q, 10, None, None, None)
                    .await
                    .unwrap_or_default();
                Some(
                    results
                        .into_iter()
                        .map(|(memory, score)| SearchResult { memory, score })
                        .collect(),
                )
            }
            None => None,
        };

        Ok(ProfileResponse {
            profile,
            search_results,
        })
    }
}

fn sanitize_profile_content(content: &str) -> String {
    content.replace("用户", "").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::category::Category;
    use crate::domain::memory::Memory;
    use crate::domain::types::MemoryType;
    use tempfile::TempDir;

    fn days_ago_str(n: i64) -> String {
        let delta = chrono::TimeDelta::try_days(n).unwrap_or_default();
        (Utc::now() - delta).to_rfc3339()
    }

    async fn setup() -> (Arc<LanceStore>, TempDir) {
        let dir = TempDir::new().expect("failed to create temp dir");
        let store = LanceStore::new(dir.path().to_str().expect("invalid path"))
            .await
            .expect("failed to create store");
        store.init_table().await.expect("failed to init table");
        (Arc::new(store), dir)
    }

    fn make_memory_with(
        tenant: &str,
        content: &str,
        category: Category,
        created_at: &str,
    ) -> Memory {
        let mut mem = Memory::new(content, category, MemoryType::Insight, tenant);
        mem.created_at = created_at.to_string();
        mem.updated_at = created_at.to_string();
        mem
    }

    #[tokio::test]
    async fn test_static_facts_from_profile_category() {
        let (store, _dir) = setup().await;
        let svc = ProfileService::new(Arc::clone(&store));

        let m1 = make_memory_with(
            "t-001",
            "speaks mandarin",
            Category::Profile,
            &days_ago_str(30),
        );
        let m2 = make_memory_with(
            "t-001",
            "prefers dark mode",
            Category::Preferences,
            &days_ago_str(15),
        );
        let m3 = make_memory_with(
            "t-001",
            "meeting yesterday",
            Category::Events,
            &days_ago_str(1),
        );

        store.create(&m1, None).await.expect("create m1");
        store.create(&m2, None).await.expect("create m2");
        store.create(&m3, None).await.expect("create m3");

        let resp = svc.get_profile(None).await.expect("get_profile");
        assert_eq!(resp.profile.static_facts.len(), 2);
        assert!(resp
            .profile
            .static_facts
            .contains(&"speaks mandarin".to_string()));
        assert!(resp
            .profile
            .static_facts
            .contains(&"prefers dark mode".to_string()));
        assert!(resp.search_results.is_none());
    }

    #[tokio::test]
    async fn test_dynamic_from_recent_events() {
        let (store, _dir) = setup().await;
        let svc = ProfileService::new(Arc::clone(&store));

        let m1 = make_memory_with(
            "t-001",
            "debugging OOM issue",
            Category::Events,
            &days_ago_str(2),
        );
        let m2 = make_memory_with(
            "t-001",
            "auth refactor pattern",
            Category::Patterns,
            &days_ago_str(3),
        );

        store.create(&m1, None).await.expect("create m1");
        store.create(&m2, None).await.expect("create m2");

        let resp = svc.get_profile(None).await.expect("get_profile");
        assert_eq!(resp.profile.dynamic_context.len(), 2);
        assert!(resp
            .profile
            .dynamic_context
            .contains(&"debugging OOM issue".to_string()));
        assert!(resp
            .profile
            .dynamic_context
            .contains(&"auth refactor pattern".to_string()));
    }

    #[tokio::test]
    async fn test_old_events_excluded() {
        let (store, _dir) = setup().await;
        let svc = ProfileService::new(Arc::clone(&store));

        let old_event = make_memory_with(
            "t-001",
            "old event from 2 weeks ago",
            Category::Events,
            &days_ago_str(14),
        );
        let recent_event =
            make_memory_with("t-001", "recent event", Category::Events, &days_ago_str(1));

        store.create(&old_event, None).await.expect("create old");
        store
            .create(&recent_event, None)
            .await
            .expect("create recent");

        let resp = svc.get_profile(None).await.expect("get_profile");
        assert_eq!(resp.profile.dynamic_context.len(), 1);
        assert_eq!(resp.profile.dynamic_context[0], "recent event");
    }

    #[tokio::test]
    async fn test_profile_with_search() {
        let (store, _dir) = setup().await;
        let svc = ProfileService::new(Arc::clone(&store));

        let m1 = make_memory_with(
            "t-001",
            "rust programming tips",
            Category::Preferences,
            &days_ago_str(5),
        );
        let m2 = make_memory_with(
            "t-001",
            "python scripting notes",
            Category::Preferences,
            &days_ago_str(3),
        );

        store.create(&m1, None).await.expect("create m1");
        store.create(&m2, None).await.expect("create m2");

        store.create_fts_index().await.expect("create fts index");

        let resp = svc
            .get_profile(Some("rust programming"))
            .await
            .expect("get_profile with search");

        assert!(resp.search_results.is_some());
        let results = resp.search_results.expect("should have search results");
        assert!(
            !results.is_empty(),
            "search for 'rust programming' should return results"
        );
        let contents: Vec<&str> = results.iter().map(|r| r.memory.content.as_str()).collect();
        assert!(contents.contains(&"rust programming tips"));
    }
}
