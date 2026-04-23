use std::sync::Arc;
use arrow_array::{
    Array, Float32Array, RecordBatch, RecordBatchIterator,
    RecordBatchReader, StringArray, UInt32Array,
};
use arrow_schema::{DataType, Field, Schema};
use dashmap::DashMap;
use futures::TryStreamExt;
use lancedb::query::{ExecutableQuery, QueryBase};
use lancedb::Table;
use tokio::sync::Mutex;
use tracing::{debug, info};

use crate::domain::cluster::MemoryCluster;
use crate::domain::error::OmemError;

use crate::store::lancedb::VECTOR_DIM;

const CLUSTER_TABLE_NAME: &str = "clusters";

pub struct ClusterStore {
    table: Table,
    locks: DashMap<String, Mutex<()>>,
}

impl ClusterStore {
    pub async fn new(db: &lancedb::Connection) -> Result<Self, OmemError> {
        let table = Self::init_table(db).await?;
        Ok(Self { table, locks: DashMap::new() })
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
                Arc::new(StringArray::from(vec![cluster.anchor_memory_id.as_str()])),
                Arc::new(vector_array),
                Arc::new(StringArray::from(vec![cluster.created_at.as_str()])),
                Arc::new(StringArray::from(vec![cluster.updated_at.as_str()])),
                Arc::new(StringArray::from(vec![cluster.last_accessed_at.as_deref()])),
            ],
        )
        .map_err(|e| OmemError::Storage(format!("failed to build cluster batch: {e}")))
    }

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

        let safe_id = Self::escape_sql(cluster_id);
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
            let safe_sid = Self::escape_sql(sid);
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
        
        Self::batch_to_clusters(&batches)
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

        Ok(MemoryCluster {
            id: get_str("id")?,
            tenant_id: get_str("tenant_id")?,
            space_id: get_str("space_id")?,
            title: get_str("title")?,
            summary: get_str("summary")?,
            category: get_str("category")?.parse()
                .map_err(|e: String| OmemError::Storage(e))?,
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
            anchor_memory_id: get_str("anchor_memory_id")?,
            created_at: get_str("created_at")?,
            updated_at: get_str("updated_at")?,
            last_accessed_at: get_opt_str("last_accessed_at")?,
        })
    }

    fn extract_anchor_vector(batch: &RecordBatch, row: usize) -> Result<Vec<f32>, OmemError> {
        use arrow::array::FixedSizeListArray;
        use arrow::datatypes::Float32Type;
        
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
        let safe_id = Self::escape_sql(cluster_id);
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

    fn escape_sql(input: &str) -> String {
        input.replace("'", "''")
    }

    pub async fn update_summary(
        &self,
        cluster_id: &str,
        summary: &str,
    ) -> Result<(), OmemError> {
        let summary = summary.to_string();
        self.with_cluster_lock(cluster_id, |mut cluster| async move {
            cluster.summary = summary;
            cluster.updated_at = chrono::Utc::now().to_rfc3339();
            Ok(((), cluster))
        }).await
    }

    pub async fn increment_member_count(
        &self,
        cluster_id: &str,
    ) -> Result<(), OmemError> {
        self.with_cluster_lock(cluster_id, |mut cluster| async move {
            cluster.member_count += 1;
            cluster.updated_at = chrono::Utc::now().to_rfc3339();
            Ok(((), cluster))
        }).await
    }

    pub async fn decrement_member_count(
        &self,
        cluster_id: &str,
    ) -> Result<(), OmemError> {
        self.with_cluster_lock(cluster_id, |mut cluster| async move {
            cluster.member_count = cluster.member_count.saturating_sub(1);
            cluster.updated_at = chrono::Utc::now().to_rfc3339();
            Ok(((), cluster))
        }).await
    }

    pub async fn delete_cluster(
        &self,
        cluster_id: &str,
    ) -> Result<(), OmemError> {
        let safe_id = Self::escape_sql(cluster_id);
        self.table
            .delete(&format!("id = '{}'", safe_id))
            .await
            .map_err(|e| OmemError::Storage(format!("delete cluster failed: {e}")))?;
        self.locks.remove(cluster_id);
        Ok(())
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

    pub async fn create_job(
        &self,
        job: &crate::domain::cluster::ClusteringJob,
    ) -> Result<(), OmemError> {
        use arrow::array::UInt64Array;
        
        let batch = RecordBatch::try_new(
            Arc::new(Schema::new(vec![
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
            ])),
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
        .map_err(|e| OmemError::Storage(format!("failed to build job batch: {e}")))?;

        let schema = batch.schema();
        let reader = RecordBatchIterator::new(vec![Ok(batch)], schema);
        self.table
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
        let safe_id = Self::escape_sql(job_id);
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
        
        Ok(Some(Self::row_to_job(&batches[0], 0)?))
    }

    pub async fn list_jobs(
        &self,
        tenant_id: &str,
        limit: usize,
    ) -> Result<Vec<crate::domain::cluster::ClusteringJob>, OmemError> {
        let safe_tenant = Self::escape_sql(tenant_id);
        let batches: Vec<RecordBatch> = self.table
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
}