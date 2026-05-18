use std::sync::Arc;
use arrow_array::{
    Array, Float32Array, RecordBatch, RecordBatchIterator,
    RecordBatchReader, StringArray, UInt32Array,
};
use arrow_schema::{DataType, Field, Schema};
use dashmap::DashMap;
use futures::TryStreamExt;
use lancedb::query::{ExecutableQuery, QueryBase, Select};
use lancedb::table::OptimizeAction;
use lancedb::Table;
use tokio::sync::Mutex;
use tracing::info;

use crate::domain::category::Category;
use crate::domain::cluster::MemoryCluster;
use crate::domain::error::OmemError;

use crate::store::lancedb::{escape_sql, VECTOR_DIM};

const CLUSTER_TABLE_NAME: &str = "clusters";
const JOB_TABLE_NAME: &str = "clustering_jobs";

pub struct ClusterStore {
    table: Table,
    job_table: Table,
    locks: DashMap<String, Mutex<()>>,
}

impl ClusterStore {
    pub async fn new(db: &lancedb::Connection) -> Result<Self, OmemError> {
        let table = Self::init_table(db).await?;
        let job_table = Self::init_job_table(db).await?;
        Ok(Self { table, job_table, locks: DashMap::new() })
    }

    fn schema() -> Schema {
        Schema::new(vec![
            Field::new("id", DataType::Utf8, false),
            Field::new("tenant_id", DataType::Utf8, false),
            Field::new("space_id", DataType::Utf8, false),
            Field::new("title", DataType::Utf8, false),
            Field::new("summary", DataType::Utf8, false),
            Field::new("category", DataType::Utf8, false),
            Field::new("member_count", DataType::UInt32, false),
            Field::new("importance", DataType::Float32, false),
            Field::new("keywords", DataType::Utf8, false),
            Field::new("tags", DataType::Utf8, false),
            Field::new("anchor_memory_id", DataType::Utf8, false),
            Field::new(
                "anchor_vector",
                DataType::FixedSizeList(
                    Arc::new(Field::new("item", DataType::Float32, true)),
                    VECTOR_DIM,
                ),
                false,
            ),
            Field::new("created_at", DataType::Utf8, false),
            Field::new("updated_at", DataType::Utf8, false),
            Field::new("last_accessed_at", DataType::Utf8, true),
        ])
    }

    fn job_schema() -> Schema {
        Schema::new(vec![
            Field::new("id", DataType::Utf8, false),
            Field::new("tenant_id", DataType::Utf8, false),
            Field::new("space_id", DataType::Utf8, false),
            Field::new("status", DataType::Utf8, false),
            Field::new("total_memories", DataType::UInt64, false),
            Field::new("processed_memories", DataType::UInt64, false),
            Field::new("assigned_to_existing", DataType::UInt64, false),
            Field::new("created_new_clusters", DataType::UInt64, false),
            Field::new("errors", DataType::UInt64, false),
            Field::new("started_at", DataType::Utf8, true),
            Field::new("completed_at", DataType::Utf8, true),
            Field::new("error_message", DataType::Utf8, true),
            Field::new("created_at", DataType::Utf8, false),
        ])
    }

    async fn init_job_table(db: &lancedb::Connection) -> Result<Table, OmemError> {
        let schema = Arc::new(Self::job_schema());
        match db.open_table(JOB_TABLE_NAME).execute().await {
            Ok(table) => Ok(table),
            Err(_) => {
                db.create_empty_table(JOB_TABLE_NAME, schema)
                    .execute()
                    .await
                    .map_err(|e| OmemError::Storage(format!("failed to create job table: {e}")))
            }
        }
    }

    async fn init_table(db: &lancedb::Connection) -> Result<Table, OmemError> {
        let schema = Arc::new(Self::schema());

        match db.open_table(CLUSTER_TABLE_NAME).execute().await {
            Ok(table) => Ok(table),
            Err(_) => {
                db.create_empty_table(CLUSTER_TABLE_NAME, schema)
                    .execute()
                    .await
                    .map_err(|e| OmemError::Storage(format!("failed to create cluster table: {e}")))
            }
        }
    }

    fn cluster_to_batch(cluster: &MemoryCluster, anchor_vector: &[f32]) -> Result<RecordBatch, OmemError> {
        use arrow::array::FixedSizeListArray;
        use arrow::datatypes::Float32Type;

        let keywords_json = serde_json::to_string(&cluster.keywords)
            .map_err(|e| OmemError::Storage(format!("failed to serialize keywords: {e}")))?;

        let tags_json = serde_json::to_string(&cluster.tags)
            .map_err(|e| OmemError::Storage(format!("failed to serialize tags: {e}")))?;

        let vec_data: Vec<Option<f32>> = anchor_vector.iter().map(|&x| Some(x)).collect();
        let vector_array = FixedSizeListArray::from_iter_primitive::<Float32Type, _, _>(
            vec![Some(vec_data)],
            VECTOR_DIM,
        );

        RecordBatch::try_new(
            Arc::new(Self::schema()),
            vec![
                Arc::new(StringArray::from(vec![cluster.id.as_str()])),
                Arc::new(StringArray::from(vec![cluster.tenant_id.as_str()])),
                Arc::new(StringArray::from(vec![cluster.space_id.as_str()])),
                Arc::new(StringArray::from(vec![cluster.title.as_str()])),
                Arc::new(StringArray::from(vec![cluster.summary.as_str()])),
                Arc::new(StringArray::from(vec![cluster.category.to_string().as_str()])),
                Arc::new(UInt32Array::from(vec![cluster.member_count])),
                Arc::new(Float32Array::from(vec![cluster.importance])),
                Arc::new(StringArray::from(vec![keywords_json.as_str()])),
                Arc::new(StringArray::from(vec![tags_json.as_str()])),
                Arc::new(StringArray::from(vec![cluster.anchor_memory_id.as_str()])),
                Arc::new(vector_array),
                Arc::new(StringArray::from(vec![cluster.created_at.as_str()])),
                Arc::new(StringArray::from(vec![cluster.updated_at.as_str()])),
                Arc::new(StringArray::from(vec![cluster.last_accessed_at.as_deref()])),
            ],
        )
        .map_err(|e| OmemError::Storage(format!("failed to build cluster batch: {e}")))
    }

    #[allow(dead_code)]
    async fn with_cluster_lock<F, Fut, T>(
        &self,
        cluster_id: &str,
        op: F,
    ) -> Result<T, OmemError>
    where
        F: FnOnce(MemoryCluster) -> Fut,
        Fut: std::future::Future<Output = Result<(T, MemoryCluster), OmemError>>,
    {
        let lock = self.locks.entry(cluster_id.to_string()).or_insert_with(|| Mutex::new(()));
        let _guard = lock.lock().await;

        let safe_id = escape_sql(cluster_id);
        let batches: Vec<RecordBatch> = self.table
            .query()
            .only_if(format!("id = '{}'", safe_id))
            .limit(1)
            .execute()
            .await
            .map_err(|e| OmemError::Storage(format!("query failed: {e}")))?
            .try_collect()
            .await
            .map_err(|e| OmemError::Storage(format!("collect failed: {e}")))?;

        if batches.is_empty() || batches[0].num_rows() == 0 {
            return Err(OmemError::NotFound(format!("cluster {} not found", cluster_id)));
        }

        let cluster = Self::row_to_cluster(&batches[0], 0)?;
        let anchor_vector = Self::extract_anchor_vector(&batches[0], 0)?;
        let (result, updated_cluster) = op(cluster).await?;

        self.table
            .delete(&format!("id = '{}'", safe_id))
            .await
            .map_err(|e| OmemError::Storage(format!("delete for update failed: {e}")))?;

        let batch = Self::cluster_to_batch(&updated_cluster, &anchor_vector)?;
        let schema = batch.schema();
        let reader = RecordBatchIterator::new(vec![Ok(batch)], schema);

        self.table
            .add(Box::new(reader) as Box<dyn RecordBatchReader + Send>)
            .execute()
            .await
            .map_err(|e| OmemError::Storage(format!("re-insert failed: {e}")))?;

        Ok(result)
    }

    pub async fn create(
        &self,
        cluster: &MemoryCluster,
        anchor_vector: &[f32],
    ) -> Result<(), OmemError> {
        let batch = Self::cluster_to_batch(cluster, anchor_vector)?;
        let schema = batch.schema();
        let reader = RecordBatchIterator::new(vec![Ok(batch)], schema);
        
        self.table
            .add(Box::new(reader) as Box<dyn RecordBatchReader + Send>)
            .execute()
            .await
            .map_err(|e| OmemError::Storage(format!("failed to insert cluster: {e}")))?;
        
        info!(cluster_id = %cluster.id, "cluster persisted to LanceDB");
        Ok(())
    }

    pub async fn batch_create_clusters(
        &self,
        clusters: &[(MemoryCluster, Vec<f32>)],
    ) -> Result<(), OmemError> {
        if clusters.is_empty() {
            return Ok(());
        }

        let batches: Vec<RecordBatch> = clusters.iter()
            .map(|(c, v)| Self::cluster_to_batch(c, v))
            .collect::<Result<Vec<_>, OmemError>>()?;

        let schema = batches[0].schema();
        let combined = arrow::compute::concat_batches(&schema, &batches)
            .map_err(|e| OmemError::Storage(format!("concat_batches failed: {e}")))?;
        let reader = RecordBatchIterator::new(vec![Ok(combined)], schema);

        self.table
            .add(Box::new(reader) as Box<dyn RecordBatchReader + Send>)
            .execute()
            .await
            .map_err(|e| OmemError::Storage(format!("batch_create_clusters failed: {e}")))?;

        info!(count = clusters.len(), "batch created clusters");
        Ok(())
    }

    pub async fn search_by_vector(
        &self,
        vector: &[f32],
        top_k: usize,
        space_id: Option<&str>,
    ) -> Result<Vec<(MemoryCluster, f32)>, OmemError> {
        let query_vec: Vec<f32> = vector.to_vec();
        
        let mut query = self.table
            .query()
            .nearest_to(query_vec)
            .map_err(|e| OmemError::Storage(format!("cluster vector search failed: {e}")))?;
        
        if let Some(sid) = space_id {
            let safe_sid = escape_sql(sid);
            query = query.only_if(format!("space_id = '{}'", safe_sid));
        }
        
        query = query.limit(top_k);
        
        let results = query
            .execute()
            .await
            .map_err(|e| OmemError::Storage(format!("cluster vector search failed: {e}")))?;
        
        let batches: Vec<RecordBatch> = results
            .try_collect()
            .await
            .map_err(|e| OmemError::Storage(format!("collect failed: {e}")))?;

        let clusters = tokio::task::spawn_blocking(move || {
            Self::batch_to_clusters(&batches)
        }).await.map_err(|e| OmemError::Internal(format!("spawn_blocking: {e}")))??;
        Ok(clusters)
    }

    fn batch_to_clusters(batches: &[RecordBatch]) -> Result<Vec<(MemoryCluster, f32)>, OmemError> {
        let mut results = Vec::new();
        
        for batch in batches {
            for row in 0..batch.num_rows() {
                let cluster = Self::row_to_cluster(batch, row)?;
                let score = Self::extract_score(batch, row);
                results.push((cluster, score));
            }
        }
        
        Ok(results)
    }

    fn row_to_cluster(batch: &RecordBatch, row: usize) -> Result<MemoryCluster, OmemError> {
        use arrow::array::StringArray;
        use arrow::array::UInt32Array;
        use arrow::array::Float32Array;

        let get_str = |name: &str| -> Result<String, OmemError> {
            let col = batch
                .column_by_name(name)
                .ok_or_else(|| OmemError::Storage(format!("missing column: {name}")))?;
            let arr = col
                .as_any()
                .downcast_ref::<StringArray>()
                .ok_or_else(|| OmemError::Storage(format!("column {name} is not Utf8")))?;
            Ok(arr.value(row).to_string())
        };

        let get_opt_str = |name: &str| -> Result<Option<String>, OmemError> {
            let col = batch
                .column_by_name(name)
                .ok_or_else(|| OmemError::Storage(format!("missing column: {name}")))?;
            let arr = col
                .as_any()
                .downcast_ref::<StringArray>()
                .ok_or_else(|| OmemError::Storage(format!("column {name} is not Utf8")))?;
            Ok(if arr.is_null(row) { None } else { Some(arr.value(row).to_string()) })
        };

        let keywords_json = get_str("keywords")?;
        let keywords: Vec<String> = serde_json::from_str(&keywords_json)
            .map_err(|e| OmemError::Storage(format!("failed to parse keywords: {e}")))?;

        let tags_json = get_str("tags")?;
        let tags: Vec<String> = serde_json::from_str(&tags_json)
            .map_err(|e| OmemError::Storage(format!("failed to parse tags: {e}")))?;

        Ok(MemoryCluster {
            id: get_str("id")?,
            tenant_id: get_str("tenant_id")?,
            space_id: get_str("space_id")?,
            title: get_str("title")?,
            summary: get_str("summary")?,
            category: get_str("category")?.parse().unwrap_or_else(|_| Category::new("profile")),
            member_count: batch
                .column_by_name("member_count")
                .and_then(|col| col.as_any().downcast_ref::<UInt32Array>())
                .map(|arr| arr.value(row))
                .unwrap_or(0),
            importance: batch
                .column_by_name("importance")
                .and_then(|col| col.as_any().downcast_ref::<Float32Array>())
                .map(|arr| arr.value(row))
                .unwrap_or(0.5),
            keywords,
            tags,
            anchor_memory_id: get_str("anchor_memory_id")?,
            created_at: get_str("created_at")?,
            updated_at: get_str("updated_at")?,
            last_accessed_at: get_opt_str("last_accessed_at")?,
        })
    }

    #[allow(dead_code)]
    fn extract_anchor_vector(batch: &RecordBatch, row: usize) -> Result<Vec<f32>, OmemError> {
        use arrow::array::FixedSizeListArray;

        let col = batch
            .column_by_name("anchor_vector")
            .ok_or_else(|| OmemError::Storage("missing column: anchor_vector".to_string()))?;
        let arr = col
            .as_any()
            .downcast_ref::<FixedSizeListArray>()
            .ok_or_else(|| OmemError::Storage("column anchor_vector is not FixedSizeList".to_string()))?;
        
        let list = arr.value(row);
        let float_arr = list
            .as_any()
            .downcast_ref::<Float32Array>()
            .ok_or_else(|| OmemError::Storage("anchor_vector items are not Float32".to_string()))?;
        
        let mut vector = Vec::with_capacity(float_arr.len());
        for i in 0..float_arr.len() {
            vector.push(float_arr.value(i));
        }
        Ok(vector)
    }

    fn extract_score(batch: &RecordBatch, row: usize) -> f32 {
        if let Some(col) = batch.column_by_name("_distance") {
            if let Some(arr) = col.as_any().downcast_ref::<Float32Array>() {
                let distance = arr.value(row);
                let score = 1.0 - distance;
                return score.clamp(-1.0, 1.0);
            }
        }
        if let Some(col) = batch.column_by_name("_score") {
            if let Some(arr) = col.as_any().downcast_ref::<Float32Array>() {
                return arr.value(row).clamp(0.0, 1.0);
            }
        }
        0.0
    }

    pub async fn get_by_id(
        &self,
        cluster_id: &str,
    ) -> Result<Option<MemoryCluster>, OmemError> {
        let safe_id = escape_sql(cluster_id);
        let batches: Vec<RecordBatch> = self.table
            .query()
            .only_if(format!("id = '{}'", safe_id))
            .limit(1)
            .execute()
            .await
            .map_err(|e| OmemError::Storage(format!("query failed: {e}")))?
            .try_collect()
            .await
            .map_err(|e| OmemError::Storage(format!("collect failed: {e}")))?;
        
        if batches.is_empty() || batches[0].num_rows() == 0 {
            return Ok(None);
        }
        
        Ok(Some(Self::row_to_cluster(&batches[0], 0)?))
    }

    pub async fn update_summary(
        &self,
        cluster_id: &str,
        summary: &str,
    ) -> Result<(), OmemError> {
        let safe_id = escape_sql(cluster_id);
        let safe_summary = escape_sql(summary);
        let now = chrono::Utc::now().to_rfc3339();
        self.table
            .update()
            .only_if(format!("id = '{safe_id}'"))
            .column("summary", format!("'{safe_summary}'"))
            .column("updated_at", format!("'{now}'"))
            .execute()
            .await
            .map_err(|e| OmemError::Storage(format!("update_summary failed: {e}")))?;
        Ok(())
    }

    pub async fn update_title(
        &self,
        cluster_id: &str,
        title: &str,
    ) -> Result<(), OmemError> {
        let safe_id = escape_sql(cluster_id);
        let safe_title = escape_sql(title);
        let now = chrono::Utc::now().to_rfc3339();
        self.table
            .update()
            .only_if(format!("id = '{safe_id}'"))
            .column("title", format!("'{safe_title}'"))
            .column("updated_at", format!("'{now}'"))
            .execute()
            .await
            .map_err(|e| OmemError::Storage(format!("update_title failed: {e}")))?;
        Ok(())
    }

    pub async fn update_cluster_fields(
        &self,
        cluster_id: &str,
        title: &str,
        summary: &str,
    ) -> Result<(), OmemError> {
        let safe_id = escape_sql(cluster_id);
        let safe_title = escape_sql(title);
        let safe_summary = escape_sql(summary);
        let now = chrono::Utc::now().to_rfc3339();
        self.table
            .update()
            .only_if(format!("id = '{safe_id}'"))
            .column("title", format!("'{safe_title}'"))
            .column("summary", format!("'{safe_summary}'"))
            .column("updated_at", format!("'{now}'"))
            .execute()
            .await
            .map_err(|e| OmemError::Storage(format!("update_cluster_fields failed: {e}")))?;
        Ok(())
    }

    pub async fn increment_member_count(
        &self,
        cluster_id: &str,
    ) -> Result<u32, OmemError> {
        let safe_id = escape_sql(cluster_id);

        let batches: Vec<RecordBatch> = self.table
            .query()
            .only_if(format!("id = '{safe_id}'"))
            .select(lancedb::query::Select::columns(&["member_count"]))
            .limit(1)
            .execute()
            .await
            .map_err(|e| OmemError::Storage(format!("increment read failed: {e}")))?
            .try_collect()
            .await
            .map_err(|e| OmemError::Storage(format!("increment collect failed: {e}")))?;

        if batches.is_empty() || batches[0].num_rows() == 0 {
            return Err(OmemError::NotFound(format!("cluster {} not found", cluster_id)));
        }

        let current: u32 = batches[0]
            .column_by_name("member_count")
            .and_then(|col| col.as_any().downcast_ref::<UInt32Array>())
            .map(|arr| arr.value(0))
            .unwrap_or(0);

        let new_count = current.saturating_add(1);
        self.set_member_count(cluster_id, new_count).await?;
        Ok(new_count)
    }

    pub async fn decrement_member_count(
        &self,
        cluster_id: &str,
    ) -> Result<(), OmemError> {
        let safe_id = escape_sql(cluster_id);

        let batches: Vec<RecordBatch> = self.table
            .query()
            .only_if(format!("id = '{safe_id}'"))
            .select(lancedb::query::Select::columns(&["member_count"]))
            .limit(1)
            .execute()
            .await
            .map_err(|e| OmemError::Storage(format!("decrement read failed: {e}")))?
            .try_collect()
            .await
            .map_err(|e| OmemError::Storage(format!("decrement collect failed: {e}")))?;

        if batches.is_empty() || batches[0].num_rows() == 0 {
            return Ok(());
        }

        let current: u32 = batches[0]
            .column_by_name("member_count")
            .and_then(|col| col.as_any().downcast_ref::<UInt32Array>())
            .map(|arr| arr.value(0))
            .unwrap_or(0);

        let new_count = current.saturating_sub(1);
        self.set_member_count(cluster_id, new_count).await?;
        Ok(())
    }

    pub async fn set_member_count(
        &self,
        cluster_id: &str,
        count: u32,
    ) -> Result<(), OmemError> {
        let safe_id = escape_sql(cluster_id);
        let now = chrono::Utc::now().to_rfc3339();
        let count_str = count.to_string();
        self.table
            .update()
            .only_if(format!("id = '{safe_id}'"))
            .column("member_count", count_str)
            .column("updated_at", format!("'{now}'"))
            .execute()
            .await
            .map_err(|e| {
                OmemError::Storage(format!("set_member_count failed: {e}"))
            })?;
        Ok(())
    }

    pub async fn batch_set_member_counts(&self, counts: &[(&str, u32)]) -> Result<(), OmemError> {
        if counts.is_empty() {
            return Ok(());
        }
        let now = chrono::Utc::now().to_rfc3339();
        for (cluster_id, count) in counts {
            self.table.update()
                .only_if(format!("id = '{}'", escape_sql(cluster_id)))
                .column("member_count", count.to_string())
                .column("updated_at", format!("'{now}'"))
                .execute().await
                .map_err(|e| OmemError::Storage(format!("batch_set_member_counts failed for {cluster_id}: {e}")))?;
        }
        Ok(())
    }

    pub async fn recalculate_cluster_counts(
        &self,
        lance_store: &crate::store::lancedb::LanceStore,
        tenant_id: &str,
    ) -> Result<usize, OmemError> {
        let clusters = self.list_clusters_by_tenant(tenant_id, 1000, 0).await?;
        if clusters.is_empty() {
            return Ok(0);
        }

        let memories = lance_store.list_all_active(Some(10000)).await?;

        let mut counts: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
        for m in &memories {
            if let Some(ref cid) = m.cluster_id {
                *counts.entry(cid.clone()).or_insert(0) += 1;
            }
        }

        let mut updated = 0usize;
        for cluster in &clusters {
            let actual = counts.get(&cluster.id).copied().unwrap_or(0);
            if actual != cluster.member_count {
                self.set_member_count(&cluster.id, actual).await?;
                updated += 1;
                info!(
                    cluster_id = %cluster.id,
                    old = cluster.member_count,
                    new = actual,
                    "recalculated cluster member_count"
                );
            }
        }

        Ok(updated)
    }

    pub async fn delete_cluster(
        &self,
        cluster_id: &str,
    ) -> Result<(), OmemError> {
        let safe_id = escape_sql(cluster_id);
        self.table
            .delete(&format!("id = '{}'", safe_id))
            .await
            .map_err(|e| OmemError::Storage(format!("delete cluster failed: {e}")))?;
        self.locks.remove(cluster_id);
        Ok(())
    }

    pub async fn batch_delete_clusters(
        &self,
        cluster_ids: &[String],
    ) -> Result<usize, OmemError> {
        let mut deleted = 0usize;
        for id in cluster_ids {
            if let Ok(()) = self.delete_cluster(id).await {
                deleted += 1;
            }
        }
        Ok(deleted)
    }

    pub async fn batch_delete_clusters_by_ids(&self, ids: &[String]) -> Result<(), OmemError> {
        if ids.is_empty() {
            return Ok(());
        }
        let id_list: Vec<String> = ids.iter().map(|id| format!("'{}'", escape_sql(id))).collect();
        let filter = format!("id IN ({})", id_list.join(","));
        self.table.delete(&filter).await
            .map_err(|e| OmemError::Storage(format!("batch_delete_clusters_by_ids failed: {e}")))?;
        Ok(())
    }

    pub async fn delete_all_clusters_by_tenant(
        &self,
        tenant_id: &str,
    ) -> Result<usize, OmemError> {
        let safe_tid = escape_sql(tenant_id);
        let batches: Vec<RecordBatch> = self.table
            .query()
            .only_if(format!("tenant_id = '{}'", safe_tid))
            .select(Select::columns(&["id"]))
            .execute()
            .await
            .map_err(|e| OmemError::Storage(format!("query clusters for delete: {e}")))?
            .try_collect()
            .await
            .map_err(|e| OmemError::Storage(format!("collect: {e}")))?;

        let mut ids = Vec::new();
        for batch in &batches {
            if let Some(arr) = batch.column_by_name("id") {
                if let Some(str_arr) = arr.as_any().downcast_ref::<StringArray>() {
                    for i in 0..str_arr.len() {
                        ids.push(str_arr.value(i).to_string());
                    }
                }
            }
        }

        let count = ids.len();
        self.batch_delete_clusters(&ids).await?;
        Ok(count)
    }

    pub async fn list_empty_clusters(
        &self,
    ) -> Result<Vec<MemoryCluster>, OmemError> {
        let batches: Vec<RecordBatch> = self.table
            .query()
            .only_if("member_count = 0")
            .execute()
            .await
            .map_err(|e| OmemError::Storage(format!("query empty clusters failed: {e}")))?
            .try_collect()
            .await
            .map_err(|e| OmemError::Storage(format!("collect failed: {e}")))?;

        let mut results = Vec::new();
        for batch in &batches {
            for row in 0..batch.num_rows() {
                results.push(Self::row_to_cluster(batch, row)?);
            }
        }
        Ok(results)
    }

    pub async fn list_clusters(
        &self,
        space_id: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<MemoryCluster>, OmemError> {
        let mut query = self.table.query();

        if let Some(sid) = space_id {
            let safe_sid = escape_sql(sid);
            query = query.only_if(format!("space_id = '{}'", safe_sid));
        }

        let batches: Vec<RecordBatch> = query
            .limit(limit + offset)
            .execute()
            .await
            .map_err(|e| OmemError::Storage(format!("query failed: {e}")))?
            .try_collect()
            .await
            .map_err(|e| OmemError::Storage(format!("collect failed: {e}")))?;

        let mut results = Vec::new();
        for batch in &batches {
            for row in offset..batch.num_rows() {
                results.push(Self::row_to_cluster(batch, row)?);
            }
        }
        Ok(results)
    }

    pub async fn list_clusters_by_tenant(
        &self,
        tenant_id: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<MemoryCluster>, OmemError> {
        let safe_tid = escape_sql(tenant_id);
        let query = self.table.query()
            .only_if(format!("tenant_id = '{}'", safe_tid));

        let batches: Vec<RecordBatch> = query
            .limit(limit + offset)
            .execute()
            .await
            .map_err(|e| OmemError::Storage(format!("query failed: {e}")))?
            .try_collect()
            .await
            .map_err(|e| OmemError::Storage(format!("collect failed: {e}")))?;

        let mut results = Vec::new();
        for batch in &batches {
            for row in offset..batch.num_rows() {
                results.push(Self::row_to_cluster(batch, row)?);
            }
        }
        Ok(results)
    }

    pub async fn count_clusters_by_tenant(
        &self,
        tenant_id: &str,
    ) -> Result<usize, OmemError> {
        let safe_tid = escape_sql(tenant_id);
        let batches: Vec<RecordBatch> = self.table.query()
            .only_if(format!("tenant_id = '{}'", safe_tid))
            .execute()
            .await
            .map_err(|e| OmemError::Storage(format!("count query failed: {e}")))?
            .try_collect()
            .await
            .map_err(|e| OmemError::Storage(format!("count collect failed: {e}")))?;

        let total: usize = batches.iter().map(|b| b.num_rows()).sum();
        Ok(total)
    }

    fn job_to_batch(
        job: &crate::domain::cluster::ClusteringJob,
    ) -> Result<RecordBatch, OmemError> {
        use arrow::array::UInt64Array;

        RecordBatch::try_new(
            Arc::new(Self::job_schema()),
            vec![
                Arc::new(StringArray::from(vec![job.id.as_str()])),
                Arc::new(StringArray::from(vec![job.tenant_id.as_str()])),
                Arc::new(StringArray::from(vec![job.space_id.as_str()])),
                Arc::new(StringArray::from(vec![format!("{:?}", job.status)])),
                Arc::new(UInt64Array::from(vec![job.total_memories])),
                Arc::new(UInt64Array::from(vec![job.processed_memories])),
                Arc::new(UInt64Array::from(vec![job.assigned_to_existing])),
                Arc::new(UInt64Array::from(vec![job.created_new_clusters])),
                Arc::new(UInt64Array::from(vec![job.errors])),
                Arc::new(StringArray::from(vec![job.started_at.as_deref()])),
                Arc::new(StringArray::from(vec![job.completed_at.as_deref()])),
                Arc::new(StringArray::from(vec![job.error_message.as_deref()])),
                Arc::new(StringArray::from(vec![job.created_at.as_str()])),
            ],
        )
        .map_err(|e| OmemError::Storage(format!("failed to build job batch: {e}")))
    }

    pub async fn create_job(
        &self,
        job: &crate::domain::cluster::ClusteringJob,
    ) -> Result<(), OmemError> {
        let batch = Self::job_to_batch(job)?;
        let schema = batch.schema();
        let reader = RecordBatchIterator::new(vec![Ok(batch)], schema);
        self.job_table
            .add(Box::new(reader) as Box<dyn RecordBatchReader + Send>)
            .execute()
            .await
            .map_err(|e| OmemError::Storage(format!("failed to insert job: {e}")))?;
        
        Ok(())
    }

    pub async fn get_job(
        &self,
        job_id: &str,
    ) -> Result<Option<crate::domain::cluster::ClusteringJob>, OmemError> {
        let safe_id = escape_sql(job_id);
        let batches: Vec<RecordBatch> = self.job_table
            .query()
            .only_if(format!("id = '{}'", safe_id))
            .limit(1)
            .execute()
            .await
            .map_err(|e| OmemError::Storage(format!("query failed: {e}")))?
            .try_collect()
            .await
            .map_err(|e| OmemError::Storage(format!("collect failed: {e}")))?;
        
        if batches.is_empty() || batches[0].num_rows() == 0 {
            return Ok(None);
        }
        
        Ok(Some(Self::row_to_job(&batches[0], 0)?))
    }

    pub async fn list_jobs(
        &self,
        tenant_id: &str,
        limit: usize,
    ) -> Result<Vec<crate::domain::cluster::ClusteringJob>, OmemError> {
        let safe_tenant = escape_sql(tenant_id);
        let batches: Vec<RecordBatch> = self.job_table
            .query()
            .only_if(format!("tenant_id = '{}'", safe_tenant))
            .limit(limit)
            .execute()
            .await
            .map_err(|e| OmemError::Storage(format!("query failed: {e}")))?
            .try_collect()
            .await
            .map_err(|e| OmemError::Storage(format!("collect failed: {e}")))?;
        
        let mut results = Vec::new();
        for batch in &batches {
            for row in 0..batch.num_rows() {
                results.push(Self::row_to_job(batch, row)?);
            }
        }
        Ok(results)
    }

    pub async fn list_running_jobs(
        &self,
    ) -> Result<Vec<crate::domain::cluster::ClusteringJob>, OmemError> {
        let batches: Vec<RecordBatch> = self.job_table
            .query()
            .only_if("status = 'Running'")
            .execute()
            .await
            .map_err(|e| OmemError::Storage(format!("query failed: {e}")))?
            .try_collect()
            .await
            .map_err(|e| OmemError::Storage(format!("collect failed: {e}")))?;
        
        let mut results = Vec::new();
        for batch in &batches {
            for row in 0..batch.num_rows() {
                results.push(Self::row_to_job(batch, row)?);
            }
        }
        Ok(results)
    }

    pub async fn update_job_status(
        &self,
        job_id: &str,
        status: &str,
        processed_memories: Option<u64>,
        assigned_to_existing: Option<u64>,
        created_new_clusters: Option<u64>,
        error_message: Option<&str>,
    ) -> Result<(), OmemError> {
        let job = self.get_job(job_id).await?;
        let mut job = match job {
            Some(j) => j,
            None => return Err(OmemError::NotFound(format!("Job {} not found", job_id))),
        };

        job.status = match status {
            "completed" => crate::domain::cluster::ClusteringJobStatus::Completed,
            "failed" => crate::domain::cluster::ClusteringJobStatus::Failed,
            "running" => crate::domain::cluster::ClusteringJobStatus::Running,
            _ => crate::domain::cluster::ClusteringJobStatus::Pending,
        };

        if let Some(processed) = processed_memories {
            job.processed_memories = processed;
        }
        if let Some(assigned) = assigned_to_existing {
            job.assigned_to_existing = assigned;
        }
        if let Some(created) = created_new_clusters {
            job.created_new_clusters = created;
        }
        if let Some(err) = error_message {
            job.error_message = Some(err.to_string());
        }

        if status == "completed" || status == "failed" {
            job.completed_at = Some(chrono::Utc::now().to_rfc3339());
        }

        self.save_job(&job).await
    }

    pub async fn save_job(
        &self,
        job: &crate::domain::cluster::ClusteringJob,
    ) -> Result<(), OmemError> {
        let safe_id = escape_sql(&job.id);
        self.job_table
            .delete(&format!("id = '{}'", safe_id))
            .await
            .map_err(|e| OmemError::Storage(format!("delete job for update failed: {e}")))?;

        let batch = Self::job_to_batch(job)?;
        let schema = batch.schema();
        let reader = RecordBatchIterator::new(vec![Ok(batch)], schema);
        self.job_table
            .add(Box::new(reader) as Box<dyn RecordBatchReader + Send>)
            .execute()
            .await
            .map_err(|e| OmemError::Storage(format!("re-insert job failed: {e}")))?;

        Ok(())
    }

    pub async fn delete_job(
        &self,
        job_id: &str,
    ) -> Result<(), OmemError> {
        let safe_id = escape_sql(job_id);
        self.job_table
            .delete(&format!("id = '{}'", safe_id))
            .await
            .map_err(|e| OmemError::Storage(format!("delete job failed: {e}")))?;
        Ok(())
    }

    fn row_to_job(batch: &RecordBatch, row: usize) -> Result<crate::domain::cluster::ClusteringJob, OmemError> {
        use arrow::array::UInt64Array;
        
        let get_str = |name: &str| -> Result<String, OmemError> {
            let col = batch
                .column_by_name(name)
                .ok_or_else(|| OmemError::Storage(format!("missing column: {name}")))?;
            let arr = col
                .as_any()
                .downcast_ref::<StringArray>()
                .ok_or_else(|| OmemError::Storage(format!("column {name} is not Utf8")))?;
            Ok(arr.value(row).to_string())
        };

        let get_opt_str = |name: &str| -> Result<Option<String>, OmemError> {
            let col = batch
                .column_by_name(name)
                .ok_or_else(|| OmemError::Storage(format!("missing column: {name}")))?;
            let arr = col
                .as_any()
                .downcast_ref::<StringArray>()
                .ok_or_else(|| OmemError::Storage(format!("column {name} is not Utf8")))?;
            Ok(if arr.is_null(row) { None } else { Some(arr.value(row).to_string()) })
        };

        let get_u64 = |name: &str| -> u64 {
            batch
                .column_by_name(name)
                .and_then(|col| col.as_any().downcast_ref::<UInt64Array>())
                .map(|arr| arr.value(row))
                .unwrap_or(0)
        };

        let status_str = get_str("status")?;
        let status = match status_str.as_str() {
            "Pending" => crate::domain::cluster::ClusteringJobStatus::Pending,
            "Running" => crate::domain::cluster::ClusteringJobStatus::Running,
            "Completed" => crate::domain::cluster::ClusteringJobStatus::Completed,
            "Failed" => crate::domain::cluster::ClusteringJobStatus::Failed,
            _ => crate::domain::cluster::ClusteringJobStatus::Pending,
        };

        Ok(crate::domain::cluster::ClusteringJob {
            id: get_str("id")?,
            tenant_id: get_str("tenant_id")?,
            space_id: get_str("space_id")?,
            status,
            total_memories: get_u64("total_memories"),
            processed_memories: get_u64("processed_memories"),
            assigned_to_existing: get_u64("assigned_to_existing"),
            created_new_clusters: get_u64("created_new_clusters"),
            errors: get_u64("errors"),
            started_at: get_opt_str("started_at")?,
            completed_at: get_opt_str("completed_at")?,
            error_message: get_opt_str("error_message")?,
            created_at: get_str("created_at")?,
        })
    }

    pub async fn optimize(&self) -> Result<(), OmemError> {
        self.table
            .optimize(OptimizeAction::Prune {
                older_than: Some(chrono::Duration::try_days(1).unwrap_or_else(|| chrono::Duration::days(1))),
                delete_unverified: Some(true),
                error_if_tagged_old_versions: None,
            })
            .await
            .map_err(|e| OmemError::Storage(format!("optimize clusters prune failed: {e}")))?;

        let _ = self.job_table
            .optimize(OptimizeAction::Prune {
                older_than: Some(chrono::Duration::try_days(1).unwrap_or_else(|| chrono::Duration::days(1))),
                delete_unverified: Some(true),
                error_if_tagged_old_versions: None,
            })
            .await;

        Ok(())
    }
}