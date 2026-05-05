use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use chrono::Utc;
use dashmap::DashMap;

use crate::domain::category::Category;
use crate::domain::error::OmemError;
use crate::domain::memory::Memory;
use crate::domain::profile::{
    FactType, ProfileFact, StaticFact, UserProfile,
    FACT_TYPE_DYNAMIC_TAG, FACT_TYPE_STATIC_TAG, PROFILE_FACT_TAG_PREFIX,
};
use crate::domain::types::MemoryType;
use crate::lifecycle::decay::parse_datetime;
use crate::llm::LlmService;
use crate::retrieve::SearchResult;
use crate::store::lancedb::{LanceStore, ListFilter};

const PROFILE_CACHE_TTL_SECS: u64 = 1800;

pub struct CachedProfile {
    pub profile: UserProfile,
    pub cached_at: Instant,
    pub refreshing: AtomicBool,
}

impl CachedProfile {
    pub fn new(profile: UserProfile) -> Self {
        Self {
            profile,
            cached_at: Instant::now(),
            refreshing: AtomicBool::new(false),
        }
    }
}

pub struct ProfileResponse {
    pub profile: UserProfile,
    pub search_results: Option<Vec<SearchResult>>,
}

pub struct ProfileService {
    store: Arc<LanceStore>,
    llm: Option<Arc<dyn LlmService>>,
    cache: Arc<DashMap<String, CachedProfile>>,
    tenant_id: String,
}

impl ProfileService {
    pub fn new(
        store: Arc<LanceStore>,
        cache: Arc<DashMap<String, CachedProfile>>,
        tenant_id: String,
    ) -> Self {
        Self {
            store,
            llm: None,
            cache,
            tenant_id,
        }
    }

    pub fn with_llm(mut self, llm: Arc<dyn LlmService>) -> Self {
        self.llm = Some(llm);
        self
    }

    pub async fn get_profile(&self, query: Option<&str>) -> Result<ProfileResponse, OmemError> {
        if query.is_none() {
            if let Some(cached) = self.cache.get(&self.tenant_id) {
                let age = cached.cached_at.elapsed().as_secs();
                if age > 7200 {
                    drop(cached);
                    self.cache.remove(&self.tenant_id);
                } else if age < PROFILE_CACHE_TTL_SECS {
                    let profile = cached.profile.clone();
                    drop(cached);
                    return Ok(ProfileResponse {
                        profile,
                        search_results: None,
                    });
                } else {
                    let profile = cached.profile.clone();
                    let should_refresh = cached
                        .refreshing
                        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
                        .is_ok();
                    drop(cached);
                    if should_refresh {
                        let store = Arc::clone(&self.store);
                        let llm = self.llm.clone();
                        let cache = Arc::clone(&self.cache);
                        let tenant_id = self.tenant_id.clone();
                        tokio::spawn(async move {
                            match build_profile(&store, llm.as_ref()).await {
                                Ok(profile) => {
                                    cache.insert(tenant_id, CachedProfile::new(profile));
                                }
                                Err(e) => {
                                    tracing::warn!("Background profile refresh failed: {}", e);
                                    if let Some(cached) = cache.get(&tenant_id) {
                                        cached.refreshing.store(false, Ordering::Release);
                                    }
                                }
                            }
                        });
                    }
                    return Ok(ProfileResponse {
                        profile,
                        search_results: None,
                    });
                }
            }
        }

        let profile = build_profile(&self.store, self.llm.as_ref()).await?;

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

        let response = ProfileResponse {
            profile,
            search_results,
        };

        if query.is_none() {
            self.cache
                .insert(self.tenant_id.clone(), CachedProfile::new(response.profile.clone()));
        }

        Ok(response)
    }

    pub async fn upsert_static_fact(
        &self,
        key: &str,
        value: &str,
        confidence: f32,
    ) -> Result<(), OmemError> {
        let existing = self.find_fact_by_key(key).await?;

        if let Some(mut mem) = existing {
            let days_old = days_since_created(&mem);
            let time_bonus = (days_old as f32 / 365.0).min(0.3) * 0.1;
            let merged_confidence = (confidence.max(mem.confidence) + time_bonus).min(1.0);

            if mem.content != value {
                mem.content = format!("{}; {}", mem.content, value);
            }
            mem.confidence = merged_confidence;
            mem.updated_at = Utc::now().to_rfc3339();
            self.store.update(&mem, None).await?;
        } else {
            self.create_fact_memory(key, value, confidence, FactType::Static)
                .await?;
        }
        self.invalidate_cache();
        Ok(())
    }

    pub async fn upsert_dynamic_fact(
        &self,
        key: &str,
        value: &str,
        confidence: f32,
    ) -> Result<(), OmemError> {
        let existing = self.find_fact_by_key(key).await?;

        if let Some(mut mem) = existing {
            mem.content = value.to_string();
            mem.confidence = confidence;
            mem.updated_at = Utc::now().to_rfc3339();
            self.store.update(&mem, None).await?;
        } else {
            self.create_fact_memory(key, value, confidence, FactType::Dynamic)
                .await?;
        }
        self.invalidate_cache();
        Ok(())
    }

    pub async fn update_profile_facts(
        &self,
        facts: Vec<ProfileFact>,
    ) -> Result<(), OmemError> {
        for fact in &facts {
            match fact.fact_type {
                FactType::Static => {
                    self.upsert_static_fact(&fact.key, &fact.value, fact.confidence)
                        .await?;
                }
                FactType::Dynamic => {
                    self.upsert_dynamic_fact(&fact.key, &fact.value, fact.confidence)
                        .await?;
                }
            }
        }
        Ok(())
    }

    pub async fn cleanup_low_confidence_facts(
        &self,
        threshold: f32,
    ) -> Result<usize, OmemError> {
        let filter = ListFilter {
            category: Some(Category::Profile.to_string()),
            state: Some("active".to_string()),
            sort: "created_at".to_string(),
            order: "asc".to_string(),
            ..Default::default()
        };
        let all = self.store.list_filtered(&filter, 500, 0).await?;

        let to_delete: Vec<String> = all
            .iter()
            .filter(|m| {
                m.tags.iter().any(|t| t.starts_with(PROFILE_FACT_TAG_PREFIX))
                    && m.confidence < threshold
            })
            .map(|m| m.id.clone())
            .collect();

        let count = to_delete.len();
        if count > 0 {
            self.store.batch_hard_delete_by_ids(&to_delete).await?;
            self.invalidate_cache();
        }
        Ok(count)
    }

    async fn find_fact_by_key(&self, key: &str) -> Result<Option<Memory>, OmemError> {
        let key_tag = format!("{}{}", PROFILE_FACT_TAG_PREFIX, key);
        let filter = ListFilter {
            category: Some(Category::Profile.to_string()),
            tags: Some(vec![key_tag]),
            state: Some("active".to_string()),
            sort: "created_at".to_string(),
            order: "desc".to_string(),
            ..Default::default()
        };
        let results = self.store.list_filtered(&filter, 10, 0).await?;
        Ok(results.into_iter().next())
    }

    async fn create_fact_memory(
        &self,
        key: &str,
        value: &str,
        confidence: f32,
        fact_type: FactType,
    ) -> Result<(), OmemError> {
        let key_tag = format!("{}{}", PROFILE_FACT_TAG_PREFIX, key);
        let type_tag = match fact_type {
            FactType::Static => FACT_TYPE_STATIC_TAG,
            FactType::Dynamic => FACT_TYPE_DYNAMIC_TAG,
        };
        let mut mem = Memory::new(value, Category::Profile, MemoryType::Insight, &self.tenant_id);
        mem.tags = vec![key_tag, type_tag.to_string()];
        mem.confidence = confidence;
        mem.visibility = "personal".to_string();
        self.store.create(&mem, None).await?;
        Ok(())
    }

    fn invalidate_cache(&self) {
        self.cache.remove(&self.tenant_id);
    }
}

async fn build_profile(
    store: &Arc<LanceStore>,
    _llm: Option<&Arc<dyn LlmService>>,
) -> Result<UserProfile, OmemError> {
    let filter = ListFilter {
        state: Some("active".to_string()),
        sort: "created_at".to_string(),
        order: "desc".to_string(),
        ..Default::default()
    };
    let all_memories = store.list_filtered(&filter, 1000, 0).await?;

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
    let static_facts: Vec<StaticFact> = static_memories
        .iter()
        .take(20)
        .map(|m| StaticFact {
            content: sanitize_profile_content(&m.content),
            tags: m.tags.clone(),
            visibility: m.visibility.clone(),
            l2_content: Some(m.l2_content.clone()),
        })
        .filter(|s| !s.content.is_empty())
        .collect();

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

    Ok(UserProfile {
        static_facts,
        dynamic_context,
    })
}

fn sanitize_profile_content(content: &str) -> String {
    content.replace("用户", "").trim().to_string()
}

fn days_since_created(mem: &Memory) -> i64 {
    let created = parse_datetime(&mem.created_at).unwrap_or_else(|| Utc::now());
    (Utc::now() - created).num_days().max(0)
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
        let svc = ProfileService::new(
            Arc::clone(&store),
            Arc::new(dashmap::DashMap::new()),
            "t-001".to_string(),
        );

        let mut m1 = make_memory_with(
            "t-001",
            "speaks mandarin",
            Category::Profile,
            &days_ago_str(30),
        );
        m1.tags = vec!["language".to_string()];
        m1.visibility = "team".to_string();
        let mut m2 = make_memory_with(
            "t-001",
            "prefers dark mode",
            Category::Preferences,
            &days_ago_str(15),
        );
        m2.tags = vec!["ui".to_string()];
        m2.visibility = "personal".to_string();
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
        assert!(resp.profile.static_facts[0]
            .content
            .contains("speaks mandarin"));
        assert!(resp.profile.static_facts[1]
            .content
            .contains("prefers dark mode"));
        assert!(!resp.profile.static_facts[0].tags.is_empty());
        assert!(!resp.profile.static_facts[0].visibility.is_empty());
        assert!(!resp.profile.static_facts[1].tags.is_empty());
        assert!(!resp.profile.static_facts[1].visibility.is_empty());
        assert!(resp.search_results.is_none());
    }

    #[tokio::test]
    async fn test_dynamic_from_recent_events() {
        let (store, _dir) = setup().await;
        let svc = ProfileService::new(
            Arc::clone(&store),
            Arc::new(dashmap::DashMap::new()),
            "t-001".to_string(),
        );

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
        let svc = ProfileService::new(
            Arc::clone(&store),
            Arc::new(dashmap::DashMap::new()),
            "t-001".to_string(),
        );

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
        let svc = ProfileService::new(
            Arc::clone(&store),
            Arc::new(dashmap::DashMap::new()),
            "t-001".to_string(),
        );

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

    #[tokio::test]
    async fn test_upsert_static_fact_creates_new() {
        let (store, _dir) = setup().await;
        let svc = ProfileService::new(
            Arc::clone(&store),
            Arc::new(dashmap::DashMap::new()),
            "t-001".to_string(),
        );

        svc.upsert_static_fact("language", "mandarin", 0.7)
            .await
            .expect("upsert_static_fact");

        let found = svc.find_fact_by_key("language").await.expect("find");
        assert!(found.is_some());
        let mem = found.unwrap();
        assert_eq!(mem.content, "mandarin");
        assert!(mem.tags.iter().any(|t| t == "pfact:language"));
        assert!(mem.tags.iter().any(|t| t == "pfact_type:static"));
        assert!((mem.confidence - 0.7).abs() < 0.01);
    }

    #[tokio::test]
    async fn test_upsert_static_fact_merges_existing() {
        let (store, _dir) = setup().await;
        let svc = ProfileService::new(
            Arc::clone(&store),
            Arc::new(dashmap::DashMap::new()),
            "t-001".to_string(),
        );

        let mut m = Memory::new("introvert", Category::Profile, MemoryType::Insight, "t-001");
        m.tags = vec!["pfact:personality".to_string(), "pfact_type:static".to_string()];
        m.confidence = 0.6;
        m.created_at = days_ago_str(60);
        m.updated_at = days_ago_str(60);
        store.create(&m, None).await.expect("create existing");

        svc.upsert_static_fact("personality", "analytical thinker", 0.5)
            .await
            .expect("upsert_static_fact merge");

        let found = svc.find_fact_by_key("personality").await.expect("find");
        let mem = found.expect("should exist after merge");
        assert!(mem.content.contains("introvert"));
        assert!(mem.content.contains("analytical thinker"));
        assert!(mem.confidence > 0.6);
    }

    #[tokio::test]
    async fn test_upsert_dynamic_fact_creates_new() {
        let (store, _dir) = setup().await;
        let svc = ProfileService::new(
            Arc::clone(&store),
            Arc::new(dashmap::DashMap::new()),
            "t-001".to_string(),
        );

        svc.upsert_dynamic_fact("current_task", "refactoring auth", 0.8)
            .await
            .expect("upsert_dynamic_fact");

        let found = svc.find_fact_by_key("current_task").await.expect("find");
        assert!(found.is_some());
        let mem = found.unwrap();
        assert_eq!(mem.content, "refactoring auth");
        assert!(mem.tags.iter().any(|t| t == "pfact_type:dynamic"));
    }

    #[tokio::test]
    async fn test_upsert_dynamic_fact_replaces_existing() {
        let (store, _dir) = setup().await;
        let svc = ProfileService::new(
            Arc::clone(&store),
            Arc::new(dashmap::DashMap::new()),
            "t-001".to_string(),
        );

        let mut m = Memory::new("old task", Category::Profile, MemoryType::Insight, "t-001");
        m.tags = vec!["pfact:current_task".to_string(), "pfact_type:dynamic".to_string()];
        m.confidence = 0.9;
        store.create(&m, None).await.expect("create existing");

        svc.upsert_dynamic_fact("current_task", "new task", 0.5)
            .await
            .expect("upsert_dynamic_fact replace");

        let found = svc.find_fact_by_key("current_task").await.expect("find");
        let mem = found.expect("should exist");
        assert_eq!(mem.content, "new task");
        assert!((mem.confidence - 0.5).abs() < 0.01);
    }

    #[tokio::test]
    async fn test_update_profile_facts_batch() {
        let (store, _dir) = setup().await;
        let svc = ProfileService::new(
            Arc::clone(&store),
            Arc::new(dashmap::DashMap::new()),
            "t-001".to_string(),
        );

        let now = Utc::now();
        let facts = vec![
            ProfileFact {
                key: "language".to_string(),
                value: "python".to_string(),
                confidence: 0.9,
                fact_type: FactType::Static,
                created_at: now,
                updated_at: now,
            },
            ProfileFact {
                key: "mood".to_string(),
                value: "focused".to_string(),
                confidence: 0.6,
                fact_type: FactType::Dynamic,
                created_at: now,
                updated_at: now,
            },
        ];

        svc.update_profile_facts(facts).await.expect("batch update");

        let lang = svc.find_fact_by_key("language").await.expect("find");
        assert!(lang.is_some());
        assert!(lang.unwrap().tags.iter().any(|t| t == "pfact_type:static"));

        let mood = svc.find_fact_by_key("mood").await.expect("find");
        assert!(mood.is_some());
        assert!(mood.unwrap().tags.iter().any(|t| t == "pfact_type:dynamic"));
    }

    #[tokio::test]
    async fn test_cleanup_removes_low_confidence() {
        let (store, _dir) = setup().await;
        let svc = ProfileService::new(
            Arc::clone(&store),
            Arc::new(dashmap::DashMap::new()),
            "t-001".to_string(),
        );

        let mut m_low = Memory::new("weak fact", Category::Profile, MemoryType::Insight, "t-001");
        m_low.tags = vec!["pfact:weak".to_string(), "pfact_type:static".to_string()];
        m_low.confidence = 0.1;
        store.create(&m_low, None).await.expect("create low");

        let mut m_high = Memory::new("strong fact", Category::Profile, MemoryType::Insight, "t-001");
        m_high.tags = vec!["pfact:strong".to_string(), "pfact_type:static".to_string()];
        m_high.confidence = 0.9;
        store.create(&m_high, None).await.expect("create high");

        let deleted = svc.cleanup_low_confidence_facts(0.5).await.expect("cleanup");
        assert_eq!(deleted, 1);

        assert!(svc.find_fact_by_key("strong").await.expect("find").is_some());
        assert!(svc.find_fact_by_key("weak").await.expect("find").is_none());
    }

    #[tokio::test]
    async fn test_cleanup_skips_non_fact_memories() {
        let (store, _dir) = setup().await;
        let svc = ProfileService::new(
            Arc::clone(&store),
            Arc::new(dashmap::DashMap::new()),
            "t-001".to_string(),
        );

        let mut m = Memory::new("regular profile", Category::Profile, MemoryType::Insight, "t-001");
        m.confidence = 0.1;
        store.create(&m, None).await.expect("create regular");

        let deleted = svc.cleanup_low_confidence_facts(0.5).await.expect("cleanup");
        assert_eq!(deleted, 0);
    }
}
