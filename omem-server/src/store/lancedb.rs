use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use arrow_array::types::Float32Type;
use arrow_array::{
    Array, FixedSizeListArray, Float32Array, Int32Array, RecordBatch, RecordBatchIterator,
    StringArray, UInt64Array,
};
use arrow_schema::{DataType, Field, Schema};
use futures::TryStreamExt;
use lancedb::index::scalar::{BTreeIndexBuilder, BitmapIndexBuilder, FtsIndexBuilder};
use lancedb::index::Index;
use lancedb::query::{ExecutableQuery, QueryBase, Select};
use lancedb::table::{CompactionOptions, NewColumnTransform, OptimizeAction, Table};
use lancedb::Connection;

use crate::domain::category::Category;
use crate::domain::error::OmemError;
use crate::domain::memory::Memory;
use crate::domain::relation::MemoryRelation;
use crate::domain::space::Provenance;
use crate::domain::types::{MemoryState, MemoryType, Tier};

pub const VECTOR_DIM: i32 = 1024;
const TABLE_NAME: &str = "memories";

pub struct ListFilter {
    pub q: Option<String>,
    pub category: Option<String>,
    pub tier: Option<String>,
    pub tags: Option<Vec<String>>,
    pub memory_type: Option<String>,
    pub state: Option<String>,
    pub visibility: Option<String>,
    pub sort: String,
    pub order: String,
}

impl Default for ListFilter {
    fn default() -> Self {
        Self {
            q: None,
            category: None,
            tier: None,
            tags: None,
            memory_type: None,
            state: None,
            visibility: None,
            sort: "created_at".to_string(),
            order: "desc".to_string(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SessionRecall {
    pub id: String,
    pub session_id: String,
    pub memory_id: String,
    pub recall_type: String,
    pub query_text: String,
    pub similarity_score: f32,
    pub llm_confidence: f32,
    pub tenant_id: String,
    pub created_at: String,
}

pub struct LanceStore {
    db: Connection,
    table_name: String,
    session_recalls_table_name: String,
    fts_indexed: AtomicBool,
}

impl LanceStore {
    pub fn db(&self) -> &Connection {
        &self.db
    }
}

impl LanceStore {
    pub async fn new(uri: &str) -> Result<Self, OmemError> {
        let mut builder = lancedb::connect(uri);

        // For S3-compatible stores (e.g., Alibaba Cloud OSS), pass through
        // virtual-hosted style and endpoint configuration.
        if uri.starts_with("s3://") {
            if let Ok(val) = std::env::var("AWS_VIRTUAL_HOSTED_STYLE_REQUEST") {
                builder = builder.storage_option("aws_virtual_hosted_style_request", val);
            }
            if let Ok(val) = std::env::var("AWS_ENDPOINT_URL") {
                builder = builder.storage_option("aws_endpoint_url", val);
            } else if let Ok(val) = std::env::var("AWS_ENDPOINT") {
                builder = builder.storage_option("aws_endpoint_url", val);
            }
        }

        let db = builder
            .execute()
            .await
            .map_err(|e| OmemError::Storage(format!("failed to connect to LanceDB: {e}")))?;
        Ok(Self {
            db,
            table_name: TABLE_NAME.to_string(),
            session_recalls_table_name: "session_recalls".to_string(),
            fts_indexed: AtomicBool::new(false),
        })
    }

    pub async fn init_table(&self) -> Result<(), OmemError> {
        let existing = self
            .db
            .table_names()
            .execute()
            .await
            .map_err(|e| OmemError::Storage(format!("failed to list tables: {e}")))?;

        if !existing.contains(&self.table_name) {
            self.db
                .create_empty_table(&self.table_name, Self::schema())
                .execute()
                .await
                .map_err(|e| OmemError::Storage(format!("failed to create table: {e}")))?;
        } else {
            // Schema evolution: detect and add missing columns
            let table = self.open_table().await?;
            let current_schema = table
                .schema()
                .await
                .map_err(|e| OmemError::Storage(format!("failed to get table schema: {e}")))?;
            let expected_schema = Self::schema();

            let missing_fields: Vec<Field> = expected_schema
                .fields()
                .iter()
                .filter(|f| current_schema.field_with_name(f.name()).is_err())
                .map(|f| f.as_ref().clone())
                .collect();

            if !missing_fields.is_empty() {
                let missing_schema = Arc::new(Schema::new(missing_fields));
                table
                    .add_columns(NewColumnTransform::AllNulls(missing_schema), None)
                    .await
                    .map_err(|e| OmemError::Storage(format!("failed to add missing columns: {e}")))?;
            }
        }

        if !existing.contains(&self.session_recalls_table_name) {
            self.db
                .create_empty_table(&self.session_recalls_table_name, Self::session_recalls_schema())
                .execute()
                .await
                .map_err(|e| {
                    OmemError::Storage(format!("failed to create session_recalls table: {e}"))
                })?;
        } else {
            let table = self.open_session_recalls_table().await?;
            let current_schema = table
                .schema()
                .await
                .map_err(|e| {
                    OmemError::Storage(format!("failed to get session_recalls schema: {e}"))
                })?;
            let expected_schema = Self::session_recalls_schema();

            let missing_fields: Vec<Field> = expected_schema
                .fields()
                .iter()
                .filter(|f| current_schema.field_with_name(f.name()).is_err())
                .map(|f| f.as_ref().clone())
                .collect();

            if !missing_fields.is_empty() {
                let missing_schema = Arc::new(Schema::new(missing_fields));
                table
                    .add_columns(NewColumnTransform::AllNulls(missing_schema), None)
                    .await
                    .map_err(|e| {
                        OmemError::Storage(format!(
                            "failed to add missing columns to session_recalls: {e}"
                        ))
                    })?;
            }
        }

        self.ensure_scalar_indexes().await?;

        // One-time purge of previously soft-deleted data
        let table = self.open_table().await?;
        match table.delete("state = 'deleted'").await {
            Ok(_) => tracing::info!("Purged soft-deleted rows"),
            Err(e) => tracing::warn!("Failed to purge deleted rows (non-critical): {e}"),
        }

        // Compact + prune + index optimize on startup to recover from version bloat
        let start = std::time::Instant::now();
        if let Err(e) = self.optimize().await {
            tracing::warn!(error = %e, "startup_optimize_failed");
        } else {
            tracing::info!(duration_ms = start.elapsed().as_millis() as u64, "startup_optimize_completed");
        }

        Ok(())
    }

    pub async fn ensure_scalar_indexes(&self) -> Result<(), OmemError> {
        let table = self.open_table().await?;

        let existing = table.list_indices().await
            .map_err(|e| OmemError::Storage(format!("list_indices failed: {e}")))?;
        let indexed_columns: std::collections::HashSet<String> = existing.iter()
            .flat_map(|idx| idx.columns.clone())
            .collect();

        let btree_cols = ["id", "cluster_id", "created_at", "updated_at"];
        for col in btree_cols {
            if !indexed_columns.contains(col) {
                table.create_index(&[col], Index::BTree(BTreeIndexBuilder::default()))
                    .execute()
                    .await
                    .map_err(|e| OmemError::Storage(format!("create {col} btree index failed: {e}")))?;
            }
        }

        let bitmap_cols = ["state", "category", "tier"];
        for col in bitmap_cols {
            if !indexed_columns.contains(col) {
                table.create_index(&[col], Index::Bitmap(BitmapIndexBuilder::default()))
                    .execute()
                    .await
                    .map_err(|e| OmemError::Storage(format!("create {col} bitmap index failed: {e}")))?;
            }
        }

        Ok(())
    }

    fn schema() -> Arc<Schema> {
        Arc::new(Schema::new(vec![
            Field::new("id", DataType::Utf8, false),
            Field::new("content", DataType::Utf8, false),
            Field::new("l0_abstract", DataType::Utf8, false),
            Field::new("l1_overview", DataType::Utf8, false),
            Field::new("l2_content", DataType::Utf8, false),
            Field::new(
                "vector",
                DataType::FixedSizeList(
                    Arc::new(Field::new("item", DataType::Float32, true)),
                    VECTOR_DIM,
                ),
                true,
            ),
            Field::new("category", DataType::Utf8, false),
            Field::new("memory_type", DataType::Utf8, false),
            Field::new("state", DataType::Utf8, false),
            Field::new("tier", DataType::Utf8, false),
            Field::new("importance", DataType::Float32, false),
            Field::new("confidence", DataType::Float32, false),
            Field::new("access_count", DataType::Int32, false),
            Field::new("tags", DataType::Utf8, false),
            Field::new("scope", DataType::Utf8, false),
            Field::new("agent_id", DataType::Utf8, true),
            Field::new("session_id", DataType::Utf8, true),
            Field::new("tenant_id", DataType::Utf8, false),
            Field::new("source", DataType::Utf8, true),
            Field::new("relations", DataType::Utf8, false),
            Field::new("superseded_by", DataType::Utf8, true),
            Field::new("invalidated_at", DataType::Utf8, true),
            Field::new("created_at", DataType::Utf8, false),
            Field::new("updated_at", DataType::Utf8, false),
            Field::new("last_accessed_at", DataType::Utf8, true),
            Field::new("space_id", DataType::Utf8, false),
            Field::new("visibility", DataType::Utf8, false),
            Field::new("owner_agent_id", DataType::Utf8, false),
            Field::new("provenance", DataType::Utf8, true),
            Field::new("version", DataType::UInt64, true),
            Field::new("provenance_source_id", DataType::Utf8, true),
            Field::new("tier_history", DataType::Utf8, true),
            Field::new("cluster_id", DataType::Utf8, true),
            Field::new("is_cluster_anchor", DataType::Boolean, true),
            Field::new("metadata", DataType::Utf8, true),
        ]))
    }

    async fn open_table(&self) -> Result<Table, OmemError> {
        self.db
            .open_table(&self.table_name)
            .execute()
            .await
            .map_err(|e| OmemError::Storage(format!("failed to open table: {e}")))
    }

    async fn open_session_recalls_table(&self) -> Result<Table, OmemError> {
        self.db
            .open_table(&self.session_recalls_table_name)
            .execute()
            .await
            .map_err(|e| OmemError::Storage(format!("failed to open session_recalls table: {e}")))
    }

    fn session_recalls_schema() -> Arc<Schema> {
        Arc::new(Schema::new(vec![
            Field::new("id", DataType::Utf8, false),
            Field::new("session_id", DataType::Utf8, false),
            Field::new("memory_id", DataType::Utf8, false),
            Field::new("recall_type", DataType::Utf8, false),
            Field::new("query_text", DataType::Utf8, false),
            Field::new("similarity_score", DataType::Float32, false),
            Field::new("llm_confidence", DataType::Float32, false),
            Field::new("tenant_id", DataType::Utf8, false),
            Field::new("created_at", DataType::Utf8, false),
        ]))
    }

    fn session_recall_to_batch(recall: &SessionRecall) -> Result<RecordBatch, OmemError> {
        RecordBatch::try_new(
            Self::session_recalls_schema(),
            vec![
                Arc::new(StringArray::from(vec![recall.id.as_str()])),
                Arc::new(StringArray::from(vec![recall.session_id.as_str()])),
                Arc::new(StringArray::from(vec![recall.memory_id.as_str()])),
                Arc::new(StringArray::from(vec![recall.recall_type.as_str()])),
                Arc::new(StringArray::from(vec![recall.query_text.as_str()])),
                Arc::new(Float32Array::from(vec![recall.similarity_score])),
                Arc::new(Float32Array::from(vec![recall.llm_confidence])),
                Arc::new(StringArray::from(vec![recall.tenant_id.as_str()])),
                Arc::new(StringArray::from(vec![recall.created_at.as_str()])),
            ],
        )
        .map_err(|e| OmemError::Storage(format!("failed to build session_recalls batch: {e}")))
    }

    fn batch_to_session_recalls(batches: &[RecordBatch]) -> Result<Vec<SessionRecall>, OmemError> {
        let mut recalls = Vec::new();
        for batch in batches {
            for i in 0..batch.num_rows() {
                recalls.push(Self::row_to_session_recall(batch, i)?);
            }
        }
        Ok(recalls)
    }

    fn row_to_session_recall(batch: &RecordBatch, row: usize) -> Result<SessionRecall, OmemError> {
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

        let get_f32 = |name: &str| -> Result<f32, OmemError> {
            let col = batch
                .column_by_name(name)
                .ok_or_else(|| OmemError::Storage(format!("missing column: {name}")))?;
            let arr = col
                .as_any()
                .downcast_ref::<Float32Array>()
                .ok_or_else(|| OmemError::Storage(format!("column {name} is not Float32")))?;
            Ok(arr.value(row))
        };

        Ok(SessionRecall {
            id: get_str("id")?,
            session_id: get_str("session_id")?,
            memory_id: get_str("memory_id")?,
            recall_type: get_str("recall_type")?,
            query_text: get_str("query_text")?,
            similarity_score: get_f32("similarity_score")?,
            llm_confidence: get_f32("llm_confidence")?,
            tenant_id: get_str("tenant_id")?,
            created_at: get_str("created_at")?,
        })
    }

    pub async fn create_session_recall(&self, recall: &SessionRecall) -> Result<(), OmemError> {
        let batch = Self::session_recall_to_batch(recall)?;
        let table = self.open_session_recalls_table().await?;
        let reader = RecordBatchIterator::new(vec![Ok(batch)], Self::session_recalls_schema());
        table
            .add(Box::new(reader) as Box<dyn arrow_array::RecordBatchReader + Send>)
            .execute()
            .await
            .map_err(|e| OmemError::Storage(format!("failed to insert session_recall: {e}")))?;
        Ok(())
    }

    pub async fn get_session_recall_by_id(
        &self,
        id: &str,
    ) -> Result<Option<SessionRecall>, OmemError> {
        let table = self.open_session_recalls_table().await?;
        let batches: Vec<RecordBatch> = table
            .query()
            .only_if(format!("id = '{}'", escape_sql(id)))
            .limit(1)
            .execute()
            .await
            .map_err(|e| OmemError::Storage(format!("session_recall query failed: {e}")))?
            .try_collect()
            .await
            .map_err(|e| OmemError::Storage(format!("collect failed: {e}")))?;

        let recalls = Self::batch_to_session_recalls(&batches)?;
        Ok(recalls.into_iter().next())
    }

    pub async fn list_session_recalls(
        &self,
        tenant_id: &str,
        session_id: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SessionRecall>, OmemError> {
        let table = self.open_session_recalls_table().await?;
        
        let mut filter = format!("tenant_id = '{}'", escape_sql(tenant_id));
        if let Some(sid) = session_id {
            filter.push_str(&format!(" AND session_id = '{}'", escape_sql(sid)));
        }
        
        let batches: Vec<RecordBatch> = table
            .query()
            .only_if(&filter)
            .execute()
            .await
            .map_err(|e| OmemError::Storage(format!("list session_recalls query failed: {e}")))?
            .try_collect()
            .await
            .map_err(|e| OmemError::Storage(format!("collect failed: {e}")))?;

        let mut recalls = Self::batch_to_session_recalls(&batches)?;
        recalls.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(recalls.into_iter().skip(offset).take(limit).collect())
    }

    pub async fn delete_session_recall(
        &self,
        id: &str,
    ) -> Result<(), OmemError> {
        let table = self.open_session_recalls_table().await?;
        table
            .delete(format!("id = '{}'", escape_sql(id)).as_str())
            .await
            .map_err(|e| OmemError::Storage(format!("failed to delete session_recall: {e}")))?;
        Ok(())
    }

    fn memory_to_batch(memory: &Memory, vector: Option<&[f32]>) -> Result<RecordBatch, OmemError> {
        let tags_json = serde_json::to_string(&memory.tags)
            .map_err(|e| OmemError::Storage(format!("failed to serialize tags: {e}")))?;
        let relations_json = serde_json::to_string(&memory.relations)
            .map_err(|e| OmemError::Storage(format!("failed to serialize relations: {e}")))?;
        let provenance_json: Option<String> = memory
            .provenance
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|e| OmemError::Storage(format!("failed to serialize provenance: {e}")))?;

        let vec_data: Vec<f32> = match vector {
            Some(v) => v.to_vec(),
            None => vec![0.0; VECTOR_DIM as usize],
        };

        let vector_array = FixedSizeListArray::from_iter_primitive::<Float32Type, _, _>(
            vec![Some(vec_data.into_iter().map(Some).collect::<Vec<_>>())],
            VECTOR_DIM,
        );

        let provenance_source_id: Option<&str> = memory
            .provenance
            .as_ref()
            .map(|p| p.shared_from_memory.as_str());

        RecordBatch::try_new(
            Self::schema(),
            vec![
                Arc::new(StringArray::from(vec![memory.id.as_str()])),
                Arc::new(StringArray::from(vec![memory.content.as_str()])),
                Arc::new(StringArray::from(vec![memory.l0_abstract.as_str()])),
                Arc::new(StringArray::from(vec![memory.l1_overview.as_str()])),
                Arc::new(StringArray::from(vec![memory.l2_content.as_str()])),
                Arc::new(vector_array),
                Arc::new(StringArray::from(vec![memory
                    .category
                    .to_string()
                    .as_str()])),
                Arc::new(StringArray::from(vec![memory
                    .memory_type
                    .to_string()
                    .as_str()])),
                Arc::new(StringArray::from(vec![memory.state.to_string().as_str()])),
                Arc::new(StringArray::from(vec![memory.tier.to_string().as_str()])),
                Arc::new(Float32Array::from(vec![memory.importance])),
                Arc::new(Float32Array::from(vec![memory.confidence])),
                Arc::new(Int32Array::from(vec![memory.access_count as i32])),
                Arc::new(StringArray::from(vec![tags_json.as_str()])),
                Arc::new(StringArray::from(vec![memory.scope.as_str()])),
                Arc::new(StringArray::from(vec![option_str(&memory.agent_id)])),
                Arc::new(StringArray::from(vec![option_str(&memory.session_id)])),
                Arc::new(StringArray::from(vec![memory.tenant_id.as_str()])),
                Arc::new(StringArray::from(vec![option_str(&memory.source)])),
                Arc::new(StringArray::from(vec![relations_json.as_str()])),
                Arc::new(StringArray::from(vec![option_str(&memory.superseded_by)])),
                Arc::new(StringArray::from(vec![option_str(&memory.invalidated_at)])),
                Arc::new(StringArray::from(vec![memory.created_at.as_str()])),
                Arc::new(StringArray::from(vec![memory.updated_at.as_str()])),
                Arc::new(StringArray::from(vec![option_str(
                    &memory.last_accessed_at,
                )])),
                Arc::new(StringArray::from(vec![memory.space_id.as_str()])),
                Arc::new(StringArray::from(vec![memory.visibility.as_str()])),
                Arc::new(StringArray::from(vec![memory.owner_agent_id.as_str()])),
                Arc::new(StringArray::from(vec![option_str(&provenance_json)])),
                Arc::new(UInt64Array::from(vec![memory.version])),
                Arc::new(StringArray::from(vec![provenance_source_id])),
                Arc::new(StringArray::from(vec![option_str(&memory.tier_history)])),
                Arc::new(StringArray::from(vec![option_str(&memory.cluster_id)])),
                Arc::new(arrow_array::BooleanArray::from(vec![memory.is_cluster_anchor])),
                Arc::new(StringArray::from(vec![memory.metadata.as_ref().and_then(|m| serde_json::to_string(m).ok())])),
            ],
        )
        .map_err(|e| OmemError::Storage(format!("failed to build RecordBatch: {e}")))
    }

    fn batch_to_memories(batches: &[RecordBatch]) -> Result<Vec<Memory>, OmemError> {
        let mut memories = Vec::new();
        for batch in batches {
            for i in 0..batch.num_rows() {
                memories.push(Self::row_to_memory(batch, i)?);
            }
        }
        Ok(memories)
    }

    fn row_to_memory(batch: &RecordBatch, row: usize) -> Result<Memory, OmemError> {
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
            if arr.is_null(row) {
                return Ok(None);
            }
            let val = arr.value(row);
            if val.is_empty() {
                Ok(None)
            } else {
                Ok(Some(val.to_string()))
            }
        };

        let get_str_or = |name: &str, default: &str| -> String {
            batch
                .column_by_name(name)
                .and_then(|col| {
                    col.as_any()
                        .downcast_ref::<StringArray>()
                        .map(|a| a.value(row).to_string())
                })
                .unwrap_or_else(|| default.to_string())
        };

        let get_f32 = |name: &str| -> Result<f32, OmemError> {
            let col = batch
                .column_by_name(name)
                .ok_or_else(|| OmemError::Storage(format!("missing column: {name}")))?;
            let arr = col
                .as_any()
                .downcast_ref::<Float32Array>()
                .ok_or_else(|| OmemError::Storage(format!("column {name} is not Float32")))?;
            Ok(arr.value(row))
        };

        let get_i32 = |name: &str| -> Result<i32, OmemError> {
            let col = batch
                .column_by_name(name)
                .ok_or_else(|| OmemError::Storage(format!("missing column: {name}")))?;
            let arr = col
                .as_any()
                .downcast_ref::<Int32Array>()
                .ok_or_else(|| OmemError::Storage(format!("column {name} is not Int32")))?;
            Ok(arr.value(row))
        };

        let get_bool_or = |name: &str, default: bool| -> bool {
            batch
                .column_by_name(name)
                .and_then(|col| {
                    col.as_any()
                        .downcast_ref::<arrow_array::BooleanArray>()
                        .map(|a| {
                            if a.is_null(row) {
                                default
                            } else {
                                a.value(row)
                            }
                        })
                })
                .unwrap_or(default)
        };

        let tags_json = get_str("tags")?;
        let tags: Vec<String> = serde_json::from_str(&tags_json)
            .map_err(|e| OmemError::Storage(format!("failed to parse tags: {e}")))?;

        let relations_json = get_str("relations")?;
        let relations: Vec<MemoryRelation> = serde_json::from_str(&relations_json)
            .map_err(|e| OmemError::Storage(format!("failed to parse relations: {e}")))?;

        let category: Category = get_str("category")?
            .parse()
            .map_err(|e: String| OmemError::Storage(e))?;
        let memory_type: MemoryType = get_str("memory_type")?
            .parse()
            .map_err(|e: String| OmemError::Storage(e))?;
        let state: MemoryState = get_str("state")?
            .parse()
            .map_err(|e: String| OmemError::Storage(e))?;
        let tier: Tier = get_str("tier")?
            .parse()
            .map_err(|e: String| OmemError::Storage(e))?;

        let provenance_str = get_str_or("provenance", "");
        let provenance: Option<Provenance> = if provenance_str.is_empty() {
            None
        } else {
            serde_json::from_str(&provenance_str).ok()
        };

        let version: Option<u64> = batch
            .column_by_name("version")
            .and_then(|col| col.as_any().downcast_ref::<UInt64Array>())
            .and_then(|arr| {
                if arr.is_null(row) {
                    None
                } else {
                    Some(arr.value(row))
                }
            });

        Ok(Memory {
            id: get_str("id")?,
            content: get_str("content")?,
            l0_abstract: get_str("l0_abstract")?,
            l1_overview: get_str("l1_overview")?,
            l2_content: get_str("l2_content")?,
            category,
            memory_type,
            state,
            tier,
            importance: get_f32("importance")?,
            confidence: get_f32("confidence")?,
            access_count: get_i32("access_count")? as u32,
            tags,
            scope: get_str("scope")?,
            agent_id: get_opt_str("agent_id")?,
            session_id: get_opt_str("session_id")?,
            tenant_id: get_str("tenant_id")?,
            source: get_opt_str("source")?,
            relations,
            superseded_by: get_opt_str("superseded_by")?,
            invalidated_at: get_opt_str("invalidated_at")?,
            created_at: get_str("created_at")?,
            updated_at: get_str("updated_at")?,
            last_accessed_at: get_opt_str("last_accessed_at")?,
            space_id: get_str_or("space_id", ""),
            visibility: get_str_or("visibility", "global"),
            owner_agent_id: get_str_or("owner_agent_id", ""),
            provenance,
            version,
            tier_history: get_opt_str("tier_history")?,
            cluster_id: get_opt_str("cluster_id")?,
            is_cluster_anchor: get_bool_or("is_cluster_anchor", false),
            metadata: get_opt_str("metadata")?.and_then(|s| serde_json::from_str(&s).ok()),
        })
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
                let score = arr.value(row);
                return score.clamp(0.0, 1.0);
            }
        }
        0.0
    }

    pub async fn list_all_active(&self) -> Result<Vec<Memory>, OmemError> {
        let table = self.open_table().await?;
        let batches: Vec<RecordBatch> = table
            .query()
            .execute()
            .await
            .map_err(|e| OmemError::Storage(format!("list all query failed: {e}")))?
            .try_collect()
            .await
            .map_err(|e| OmemError::Storage(format!("collect failed: {e}")))?;

        let memories = tokio::task::spawn_blocking(move || {
            Self::batch_to_memories(&batches)
        }).await.map_err(|e| OmemError::Internal(format!("spawn_blocking: {e}")))??;
        Ok(memories)
    }

    pub async fn find_memories_by_session_id(
        &self,
        session_id: &str,
        limit: usize,
    ) -> Result<Vec<Memory>, OmemError> {
        let table = self.open_table().await?;
        let filter = format!("session_id = '{}'", escape_sql(session_id));
        let batches: Vec<RecordBatch> = table
            .query()
            .only_if(&filter)
            .limit(limit)
            .execute()
            .await
            .map_err(|e| OmemError::Storage(format!("find_memories_by_session_id query failed: {e}")))?
            .try_collect()
            .await
            .map_err(|e| OmemError::Storage(format!("collect failed: {e}")))?;

        let memories = tokio::task::spawn_blocking(move || {
            Self::batch_to_memories(&batches)
        }).await.map_err(|e| OmemError::Internal(format!("spawn_blocking: {e}")))??;
        Ok(memories)
    }

    pub async fn create(&self, memory: &Memory, vector: Option<&[f32]>) -> Result<(), OmemError> {
        let batch = Self::memory_to_batch(memory, vector)?;
        let table = self.open_table().await?;
        let reader = RecordBatchIterator::new(vec![Ok(batch)], Self::schema());
        table
            .add(Box::new(reader) as Box<dyn arrow_array::RecordBatchReader + Send>)
            .execute()
            .await
            .map_err(|e| OmemError::Storage(format!("failed to insert memory: {e}")))?;

        // Auto-create FTS index after first successful write.
        // LanceDB requires data in the table before creating FTS indexes.
        if !self.fts_indexed.load(Ordering::Relaxed) {
            if let Err(e) = self.create_fts_index().await {
                tracing::warn!("Failed to create FTS index (will retry on next write): {e}");
            } else {
                self.fts_indexed.store(true, Ordering::Relaxed);
            }
        }

        // OOM guard: no maybe_optimize on write path — auto_cleanup handles it
        Ok(())
    }

    pub async fn get_by_id(&self, id: &str) -> Result<Option<Memory>, OmemError> {
        let table = self.open_table().await?;
        let batches: Vec<RecordBatch> = table
            .query()
            .only_if(format!("id = '{}'", escape_sql(id)))
            .limit(1)
            .execute()
            .await
            .map_err(|e| OmemError::Storage(format!("query failed: {e}")))?
            .try_collect()
            .await
            .map_err(|e| OmemError::Storage(format!("collect failed: {e}")))?;

        let memories = tokio::task::spawn_blocking(move || {
            Self::batch_to_memories(&batches)
        }).await.map_err(|e| OmemError::Internal(format!("spawn_blocking: {e}")))??;
        Ok(memories.into_iter().next())
    }

    /// Retrieve only the vector embedding for a memory by its ID.
    /// Returns `Ok(None)` if the memory is not found or has been deleted.
    pub async fn get_vector_by_id(&self, id: &str) -> Result<Option<Vec<f32>>, OmemError> {
        let table = self.open_table().await?;
        let batches: Vec<RecordBatch> = table
            .query()
            .only_if(format!("id = '{}'", escape_sql(id)))
            .limit(1)
            .execute()
            .await
            .map_err(|e| OmemError::Storage(format!("vector query failed: {e}")))?
            .try_collect()
            .await
            .map_err(|e| OmemError::Storage(format!("collect failed: {e}")))?;

        if batches.is_empty() || batches[0].num_rows() == 0 {
            return Ok(None);
        }

        let batch = &batches[0];
        let col = batch
            .column_by_name("vector")
            .ok_or_else(|| OmemError::Storage("missing vector column".to_string()))?;
        let fsl = col
            .as_any()
            .downcast_ref::<FixedSizeListArray>()
            .ok_or_else(|| OmemError::Storage("vector column is not FixedSizeList".to_string()))?;
        let inner = fsl.value(0);
        let float_arr = inner
            .as_any()
            .downcast_ref::<Float32Array>()
            .ok_or_else(|| OmemError::Storage("vector inner is not Float32".to_string()))?;
        Ok(Some(float_arr.values().to_vec()))
    }

    pub async fn get_all_vectors(&self) -> Result<Vec<(String, Vec<f32>)>, OmemError> {
        let table = self.open_table().await?;
        let batches: Vec<RecordBatch> = table
            .query()
            .select(Select::columns(&["id", "vector"]))
            .execute()
            .await
            .map_err(|e| OmemError::Storage(format!("get_all_vectors query failed: {e}")))?
            .try_collect()
            .await
            .map_err(|e| OmemError::Storage(format!("collect failed: {e}")))?;

        let mut results = Vec::new();
        for batch in &batches {
            let id_col = batch
                .column_by_name("id")
                .ok_or_else(|| OmemError::Storage("missing id column".to_string()))?;
            let id_arr = id_col
                .as_any()
                .downcast_ref::<StringArray>()
                .ok_or_else(|| OmemError::Storage("id column is not StringArray".to_string()))?;

            let vec_col = batch
                .column_by_name("vector")
                .ok_or_else(|| OmemError::Storage("missing vector column".to_string()))?;
            let fsl = vec_col
                .as_any()
                .downcast_ref::<FixedSizeListArray>()
                .ok_or_else(|| OmemError::Storage("vector column is not FixedSizeList".to_string()))?;

            for row in 0..batch.num_rows() {
                if fsl.is_null(row) {
                    continue;
                }
                let id = id_arr.value(row).to_string();
                let inner = fsl.value(row);
                let float_arr = inner
                    .as_any()
                    .downcast_ref::<Float32Array>()
                    .ok_or_else(|| OmemError::Storage("vector inner is not Float32".to_string()))?;
                results.push((id, float_arr.values().to_vec()));
            }
        }
        Ok(results)
    }

    pub async fn update(&self, memory: &Memory, vector: Option<&[f32]>) -> Result<(), OmemError> {
        // Auto-increment version on every update
        let mut mem = memory.clone();
        mem.version = Some(mem.version.unwrap_or(0) + 1);
        mem.updated_at = chrono::Utc::now().to_rfc3339();

        if let Some(v) = vector {
            // Vector update path: delete + re-insert (vectors cannot be updated via expressions)
            let table = self.open_table().await?;
            table
                .delete(&format!("id = '{}'", escape_sql(&mem.id)))
                .await
                .map_err(|e| OmemError::Storage(format!("delete for update failed: {e}")))?;

            let batch = Self::memory_to_batch(&mem, Some(v))?;
            let reader = RecordBatchIterator::new(vec![Ok(batch)], Self::schema());
            table
                .add(Box::new(reader) as Box<dyn arrow_array::RecordBatchReader + Send>)
                .execute()
                .await
                .map_err(|e| OmemError::Storage(format!("re-insert for update failed: {e}")))?;
        } else {
            // Scalar-only update path: use native table.update() to avoid version bloat
            let tags_json = serde_json::to_string(&mem.tags)
                .map_err(|e| OmemError::Storage(format!("failed to serialize tags: {e}")))?;
            let relations_json = serde_json::to_string(&mem.relations)
                .map_err(|e| OmemError::Storage(format!("failed to serialize relations: {e}")))?;
            let provenance_json: Option<String> = mem
                .provenance
                .as_ref()
                .map(serde_json::to_string)
                .transpose()
                .map_err(|e| OmemError::Storage(format!("failed to serialize provenance: {e}")))?;
            let provenance_source_id: Option<String> = mem
                .provenance
                .as_ref()
                .map(|p| p.shared_from_memory.clone());

            let safe_id = escape_sql(&mem.id);
            let table = self.open_table().await?;
            table
                .update()
                .only_if(format!("id = '{safe_id}'"))
                .column("content", sql_str(&mem.content))
                .column("l0_abstract", sql_str(&mem.l0_abstract))
                .column("l1_overview", sql_str(&mem.l1_overview))
                .column("l2_content", sql_str(&mem.l2_content))
                .column("category", sql_str(&mem.category.to_string()))
                .column("memory_type", sql_str(&mem.memory_type.to_string()))
                .column("state", sql_str(&mem.state.to_string()))
                .column("tier", sql_str(&mem.tier.to_string()))
                .column("importance", format!("{:.6}", mem.importance))
                .column("confidence", format!("{:.6}", mem.confidence))
                .column("access_count", format!("{}", mem.access_count as i32))
                .column("tags", sql_str(&tags_json))
                .column("scope", sql_str(&mem.scope))
                .column("agent_id", sql_opt_str(&mem.agent_id))
                .column("session_id", sql_opt_str(&mem.session_id))
                .column("tenant_id", sql_str(&mem.tenant_id))
                .column("source", sql_opt_str(&mem.source))
                .column("relations", sql_str(&relations_json))
                .column("superseded_by", sql_opt_str(&mem.superseded_by))
                .column("invalidated_at", sql_opt_str(&mem.invalidated_at))
                .column("updated_at", sql_str(&mem.updated_at))
                .column("last_accessed_at", sql_opt_str(&mem.last_accessed_at))
                .column("space_id", sql_str(&mem.space_id))
                .column("visibility", sql_str(&mem.visibility))
                .column("owner_agent_id", sql_str(&mem.owner_agent_id))
                .column("provenance", sql_opt_str(&provenance_json))
                .column("version", format!("{}", mem.version.unwrap_or(0)))
                .column("provenance_source_id", sql_opt_str(&provenance_source_id))
                .column("tier_history", sql_opt_str(&mem.tier_history))
                .column("cluster_id", sql_opt_str(&mem.cluster_id))
                .column("is_cluster_anchor", if mem.is_cluster_anchor { "true" } else { "false" })
                .execute()
                .await
                .map_err(|e| OmemError::Storage(format!("update failed: {e}")))?;
        }
        Ok(())
    }

    pub async fn update_memory_cluster_id(
        &self,
        memory_id: &str,
        cluster_id: Option<&str>,
        is_anchor: bool,
    ) -> Result<(), OmemError> {
        let table = self.open_table().await?;
        let safe_id = escape_sql(memory_id);
        let cluster_value = match cluster_id {
            Some(cid) => format!("'{}'", escape_sql(cid)),
            None => "null".to_string(),
        };
        table
            .update()
            .only_if(format!("id = '{safe_id}'"))
            .column("cluster_id", cluster_value)
            .column("is_cluster_anchor", if is_anchor { "true" } else { "false" })
            .execute()
            .await
            .map_err(|e| OmemError::Storage(format!("update cluster_id failed: {e}")))?;
        Ok(())
    }

    pub async fn clear_all_cluster_ids(&self) -> Result<u64, OmemError> {
        let table = self.open_table().await?;
        let result = table
            .update()
            .only_if("cluster_id IS NOT NULL")
            .column("cluster_id", "null")
            .column("is_cluster_anchor", "false")
            .execute()
            .await
            .map_err(|e| OmemError::Storage(format!("batch clear cluster_id failed: {e}")))?;
        Ok(result.rows_updated)
    }

    pub async fn hard_delete(&self, id: &str) -> Result<(), OmemError> {
        let table = self.open_table().await?;
        table
            .delete(&format!("id = '{}'", escape_sql(id)))
            .await
            .map_err(|e| OmemError::Storage(format!("hard_delete failed: {e}")))?;
        Ok(())
    }

    pub async fn batch_hard_delete_by_ids(&self, ids: &[String]) -> Result<usize, OmemError> {
        if ids.is_empty() {
            return Ok(0);
        }
        let table = self.open_table().await?;
        let safe_ids: Vec<String> = ids.iter().map(|id| format!("'{}'", escape_sql(id))).collect();
        let id_list = safe_ids.join(", ");
        table
            .delete(&format!("id IN ({id_list})"))
            .await
            .map_err(|e| OmemError::Storage(format!("batch_hard_delete_by_ids failed: {e}")))?;
        Ok(ids.len())
    }

    pub async fn list(&self, limit: usize, offset: usize) -> Result<Vec<Memory>, OmemError> {
        let table = self.open_table().await?;
        let batches: Vec<RecordBatch> = table
            .query()
            .limit(limit + offset)
            .execute()
            .await
            .map_err(|e| OmemError::Storage(format!("list query failed: {e}")))?
            .try_collect()
            .await
            .map_err(|e| OmemError::Storage(format!("collect failed: {e}")))?;

        let all = tokio::task::spawn_blocking(move || {
            Self::batch_to_memories(&batches)
        }).await.map_err(|e| OmemError::Internal(format!("spawn_blocking: {e}")))??;
        Ok(all.into_iter().skip(offset).take(limit).collect())
    }

    pub async fn list_by_cluster_id(
        &self,
        cluster_id: &str,
    ) -> Result<Vec<Memory>, OmemError> {
        let table = self.open_table().await?;
        let safe_cid = cluster_id.replace("'", "''");
        let batches: Vec<RecordBatch> = table
            .query()
            .only_if(format!("cluster_id = '{}'", safe_cid))
            .execute()
            .await
            .map_err(|e| OmemError::Storage(format!("list_by_cluster_id query failed: {e}")))?
            .try_collect()
            .await
            .map_err(|e| OmemError::Storage(format!("collect failed: {e}")))?;

        let memories = tokio::task::spawn_blocking(move || {
            Self::batch_to_memories(&batches)
        }).await.map_err(|e| OmemError::Internal(format!("spawn_blocking: {e}")))??;
        Ok(memories)
    }

    pub async fn vector_search(
        &self,
        query_vector: &[f32],
        limit: usize,
        min_score: f32,
        scope_filter: Option<&str>,
        visibility_filter: Option<&str>,
        tags_filter: Option<&[String]>,
    ) -> Result<Vec<(Memory, f32)>, OmemError> {
        let table = self.open_table().await?;
        let mut query = table
            .query()
            .nearest_to(query_vector)
            .map_err(|e| OmemError::Storage(format!("vector query build failed: {e}")))?;

        query = query.limit(limit);

        let mut filter = String::new();
        if let Some(scope) = scope_filter {
            filter.push_str(&format!("scope = '{}'", escape_sql(scope)));
        }
        if let Some(vis) = visibility_filter {
            if !filter.is_empty() {
                filter.push_str(" AND ");
            }
            filter.push_str(&format!("({vis})"));
        }
        if let Some(tags) = tags_filter {
            for tag in tags {
                if !filter.is_empty() {
                    filter.push_str(" AND ");
                }
                filter.push_str(&format!("tags LIKE '%\"{}\"%'", escape_sql(tag)));
            }
        }
        if !filter.is_empty() {
            query = query.only_if(filter);
        }

        let batches: Vec<RecordBatch> = query
            .execute()
            .await
            .map_err(|e| OmemError::Storage(format!("vector search failed: {e}")))?
            .try_collect()
            .await
            .map_err(|e| OmemError::Storage(format!("collect failed: {e}")))?;

        let mut results = Vec::new();
        let mut raw_count = 0;
        let mut scores_debug = Vec::new();
        for batch in &batches {
            for i in 0..batch.num_rows() {
                raw_count += 1;
                let score = Self::extract_score(batch, i);
                if scores_debug.len() < 5 {
                    scores_debug.push(score);
                }
                if score >= min_score {
                    let memory = Self::row_to_memory(batch, i)?;
                    results.push((memory, score));
                }
            }
        }
        tracing::info!(raw_count = raw_count, filtered_count = results.len(), min_score = min_score, ?scores_debug, "vector_search_filter");
        Ok(results)
    }

    pub async fn fts_search(
        &self,
        query: &str,
        limit: usize,
        scope_filter: Option<&str>,
        visibility_filter: Option<&str>,
        tags_filter: Option<&[String]>,
    ) -> Result<Vec<(Memory, f32)>, OmemError> {
        let table = self.open_table().await?;

        let fts_query = lance_index::scalar::FullTextSearchQuery::new(query.to_string());

        let mut q = table
            .query()
            .full_text_search(fts_query)
            .select(Select::All)
            .limit(limit);

        let mut filter = String::new();
        if let Some(scope) = scope_filter {
            filter.push_str(&format!("scope = '{}'", escape_sql(scope)));
        }
        if let Some(vis) = visibility_filter {
            if !filter.is_empty() {
                filter.push_str(" AND ");
            }
            filter.push_str(&format!("({vis})"));
        }
        if let Some(tags) = tags_filter {
            for tag in tags {
                if !filter.is_empty() {
                    filter.push_str(" AND ");
                }
                filter.push_str(&format!("tags LIKE '%\"{}\"%'", escape_sql(tag)));
            }
        }
        if !filter.is_empty() {
            q = q.postfilter().only_if(filter);
        }

        let batches: Vec<RecordBatch> = q
            .execute()
            .await
            .map_err(|e| OmemError::Storage(format!("FTS search failed: {e}")))?
            .try_collect()
            .await
            .map_err(|e| OmemError::Storage(format!("collect failed: {e}")))?;

        let mut results = Vec::new();
        for batch in &batches {
            for i in 0..batch.num_rows() {
                let score = Self::extract_score(batch, i);
                let memory = Self::row_to_memory(batch, i)?;
                results.push((memory, score));
            }
        }
        Ok(results)
    }

    pub fn build_visibility_filter(&self, agent_id: &str, accessible_spaces: &[String]) -> String {
        let mut vis_conditions = vec!["visibility = 'global'".to_string()];

        if !agent_id.is_empty() {
            vis_conditions.push(format!(
                "(visibility = 'private' AND owner_agent_id = '{}')",
                agent_id.replace('\'', "''")
            ));
        }

        for space in accessible_spaces {
            vis_conditions.push(format!(
                "visibility = 'shared:{}'",
                space.replace('\'', "''")
            ));
        }

        vis_conditions.join(" OR ")
    }

    pub async fn create_vector_index(&self) -> Result<(), OmemError> {
        let table = self.open_table().await?;
        let count = table.count_rows(None).await
            .map_err(|e| OmemError::Storage(format!("count_rows failed: {e}")))?;

        if count < 100_000 {
            tracing::info!("Skipping vector index: {count} rows < 100K threshold");
            return Ok(());
        }

        table
            .create_index(
                &["vector"],
                Index::IvfHnswSq(
                    lancedb::index::vector::IvfHnswSqIndexBuilder::default()
                        .distance_type(lancedb::DistanceType::Cosine),
                ),
            )
            .execute()
            .await
            .map_err(|e| OmemError::Storage(format!("failed to create vector index: {e}")))?;
        Ok(())
    }

    pub async fn create_fts_index(&self) -> Result<(), OmemError> {
        let table = self.open_table().await?;

        // Use ngram tokenizer for better CJK support.
        // simple tokenizer splits on whitespace/punctuation only — useless for Chinese.
        // ngram(2,4) generates all 2-4 char substrings, enabling substring matching for CJK.
        let fts_params = FtsIndexBuilder::default()
            .base_tokenizer("ngram".to_string())
            .ngram_min_length(2)
            .ngram_max_length(4)
            .stem(false)
            .remove_stop_words(false);

        table
            .create_index(&["content"], Index::FTS(fts_params.clone()))
            .execute()
            .await
            .map_err(|e| {
                OmemError::Storage(format!("failed to create FTS index on content: {e}"))
            })?;
        table
            .create_index(&["l0_abstract"], Index::FTS(fts_params))
            .execute()
            .await
            .map_err(|e| {
                OmemError::Storage(format!("failed to create FTS index on l0_abstract: {e}"))
            })?;
        Ok(())
    }

    pub async fn list_filtered(
        &self,
        filter: &ListFilter,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<Memory>, OmemError> {
        let mut memories = if let Some(ref q) = filter.q {
            // Full-text search path: use FTS with postfilter for other conditions
            let table = self.open_table().await?;
            let fts_query = lance_index::scalar::FullTextSearchQuery::new(q.to_string());
            let mut query = table
                .query()
                .full_text_search(fts_query)
                .select(Select::All)
                .limit(10000);

            let where_clause = Self::build_where_clause(filter);
            if where_clause != "true" {
                query = query.postfilter().only_if(where_clause);
            }

            let batches: Vec<RecordBatch> = query
                .execute()
                .await
                .map_err(|e| OmemError::Storage(format!("FTS list query failed: {e}")))?
                .try_collect()
                .await
                .map_err(|e| OmemError::Storage(format!("collect failed: {e}")))?;

            tokio::task::spawn_blocking(move || {
                Self::batch_to_memories(&batches)
            }).await.map_err(|e| OmemError::Internal(format!("spawn_blocking: {e}")))??
        } else {
            // Original scalar filter path
            let table = self.open_table().await?;
            let where_clause = Self::build_where_clause(filter);

            let batches: Vec<RecordBatch> = table
                .query()
                .only_if(&where_clause)
                .execute()
                .await
                .map_err(|e| OmemError::Storage(format!("list_filtered query failed: {e}")))?
                .try_collect()
                .await
                .map_err(|e| OmemError::Storage(format!("collect failed: {e}")))?;

            tokio::task::spawn_blocking(move || {
                Self::batch_to_memories(&batches)
            }).await.map_err(|e| OmemError::Internal(format!("spawn_blocking: {e}")))??
        };

        // Sort in Rust (LanceDB query builder doesn't support ORDER BY)
        match filter.sort.as_str() {
            "importance" => memories.sort_by(|a, b| {
                a.importance
                    .partial_cmp(&b.importance)
                    .unwrap_or(std::cmp::Ordering::Equal)
            }),
            "access_count" => memories.sort_by_key(|m| m.access_count),
            "updated_at" => memories.sort_by(|a, b| a.updated_at.cmp(&b.updated_at)),
            _ => memories.sort_by(|a, b| a.created_at.cmp(&b.created_at)),
        }
        if filter.order == "desc" {
            memories.reverse();
        }

        Ok(memories.into_iter().skip(offset).take(limit).collect())
    }

    pub async fn count_filtered(&self, filter: &ListFilter) -> Result<usize, OmemError> {
        if let Some(ref q) = filter.q {
            // Full-text search path
            let table = self.open_table().await?;
            let fts_query = lance_index::scalar::FullTextSearchQuery::new(q.to_string());
            let mut query = table
                .query()
                .full_text_search(fts_query)
                .select(Select::All)
                .limit(10000);

            let where_clause = Self::build_where_clause(filter);
            if where_clause != "true" {
                query = query.postfilter().only_if(where_clause);
            }

            let batches: Vec<RecordBatch> = query
                .execute()
                .await
                .map_err(|e| OmemError::Storage(format!("FTS count query failed: {e}")))?
                .try_collect()
                .await
                .map_err(|e| OmemError::Storage(format!("collect failed: {e}")))?;

            let memories = tokio::task::spawn_blocking(move || {
                Self::batch_to_memories(&batches)
            }).await.map_err(|e| OmemError::Internal(format!("spawn_blocking: {e}")))??;

            Ok(memories.len())
        } else {
            // Original scalar filter path
            let table = self.open_table().await?;
            let where_clause = Self::build_where_clause(filter);

            let count = table
                .count_rows(Some(where_clause))
                .await
                .map_err(|e| OmemError::Storage(format!("count failed: {e}")))?;

            Ok(count)
        }
    }

    /// Find memories whose provenance.shared_from_memory matches the given original memory ID.
    /// Used by the unshare handler to locate shared copies in a target space.
    pub async fn find_by_provenance_source(
        &self,
        source_memory_id: &str,
    ) -> Result<Vec<Memory>, OmemError> {
        let table = self.open_table().await?;

        let schema = table
            .schema()
            .await
            .map_err(|e| OmemError::Storage(format!("schema check failed: {e}")))?;
        if schema.field_with_name("provenance_source_id").is_err() {
            return Ok(vec![]);
        }

        let filter = format!(
            "provenance_source_id = '{}'",
            escape_sql(source_memory_id)
        );
        let batches: Vec<RecordBatch> = table
            .query()
            .only_if(filter)
            .execute()
            .await
            .map_err(|e| OmemError::Storage(format!("provenance query failed: {e}")))?
            .try_collect()
            .await
            .map_err(|e| OmemError::Storage(format!("collect failed: {e}")))?;

        let memories = tokio::task::spawn_blocking(move || {
            Self::batch_to_memories(&batches)
        }).await.map_err(|e| OmemError::Internal(format!("spawn_blocking: {e}")))??;
        Ok(memories)
    }

    pub async fn batch_hard_delete(&self, filter: &str) -> Result<usize, OmemError> {
        let table = self.open_table().await?;
        let count = table
            .count_rows(Some(filter.to_string()))
            .await
            .map_err(|e| OmemError::Storage(format!("count_before_delete failed: {e}")))?;
        table
            .delete(filter)
            .await
            .map_err(|e| OmemError::Storage(format!("batch_hard_delete failed: {e}")))?;
        Ok(count)
    }

    pub async fn count_by_filter(&self, filter: &str) -> Result<usize, OmemError> {
        let table = self.open_table().await?;
        let count = table
            .count_rows(Some(filter.to_string()))
            .await
            .map_err(|e| OmemError::Storage(format!("count_by_filter failed: {e}")))?;
        Ok(count)
    }

    pub async fn delete_all(&self) -> Result<usize, OmemError> {
        let table = self.open_table().await?;
        let count = table.count_rows(None).await
            .map_err(|e| OmemError::Storage(format!("count_rows failed: {e}")))?;
        table
            .delete("true")
            .await
            .map_err(|e| OmemError::Storage(format!("delete_all failed: {e}")))?;
        Ok(count)
    }

    pub async fn get_memories_by_ids(
        &self,
        ids: &[String],
    ) -> Result<Vec<Memory>, OmemError> {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        let table = self.open_table().await?;
        let id_list = ids
            .iter()
            .map(|id| format!("'{}'", escape_sql(id)))
            .collect::<Vec<_>>()
            .join(", ");
        let filter = format!("id IN ({id_list})");
        let batches: Vec<RecordBatch> = table
            .query()
            .only_if(&filter)
            .execute()
            .await
            .map_err(|e| OmemError::Storage(format!("batch get failed: {e}")))?
            .try_collect()
            .await
            .map_err(|e| OmemError::Storage(format!("collect failed: {e}")))?;

        let memories = tokio::task::spawn_blocking(move || {
            Self::batch_to_memories(&batches)
        }).await.map_err(|e| OmemError::Internal(format!("spawn_blocking: {e}")))??;
        Ok(memories)
    }

    fn build_where_clause(filter: &ListFilter) -> String {
        let mut conditions = Vec::new();

        if let Some(s) = &filter.state {
            conditions.push(format!("state = '{}'", escape_sql(s)));
        }

        if let Some(ref cat) = filter.category {
            conditions.push(format!("category = '{}'", escape_sql(cat)));
        }
        if let Some(ref t) = filter.tier {
            conditions.push(format!("tier = '{}'", escape_sql(t)));
        }
        if let Some(ref mt) = filter.memory_type {
            conditions.push(format!("memory_type = '{}'", escape_sql(mt)));
        }
        if let Some(ref tags) = filter.tags {
            for tag in tags {
                let escaped = escape_sql(tag);
                conditions.push(format!("(tags LIKE '%\"{}\"%')", escaped));
            }
        }
        if let Some(ref v) = filter.visibility {
            conditions.push(format!("visibility = '{}'", escape_sql(v)));
        }

        if conditions.is_empty() {
            "true".to_string()
        } else {
            conditions.join(" AND ")
        }
    }

    /// Optimize LanceDB tables: compact → prune → index optimize to reclaim disk space
    /// and maintain query performance.
    pub async fn optimize(&self) -> Result<(), OmemError> {
        let table = self.open_table().await?;

        // Step 1: Compact — merge small fragment files produced by frequent updates
        table
            .optimize(OptimizeAction::Compact {
                options: CompactionOptions::default(),
                remap_options: None,
            })
            .await
            .map_err(|e| OmemError::Storage(format!("optimize compact failed: {e}")))?;

        // Step 2: Prune — remove all old versions (we don't use time travel)
        table
            .optimize(OptimizeAction::Prune {
                older_than: Some(
                    chrono::Duration::try_minutes(0)
                        .unwrap_or_else(|| chrono::Duration::minutes(0)),
                ),
                delete_unverified: Some(true),
                error_if_tagged_old_versions: None,
            })
            .await
            .map_err(|e| OmemError::Storage(format!("optimize prune failed: {e}")))?;

        // Step 3: Optimize indices — merge unindexed data into existing indices
        table
            .optimize(OptimizeAction::Index(
                lance_index::optimize::OptimizeOptions::default(),
            ))
            .await
            .map_err(|e| OmemError::Storage(format!("optimize index failed: {e}")))?;

        // Session recalls table — compact + prune only (no vector index)
        if let Ok(sr_table) = self.open_session_recalls_table().await {
            let _ = sr_table
                .optimize(OptimizeAction::Compact {
                    options: CompactionOptions::default(),
                    remap_options: None,
                })
                .await;
            let _ = sr_table
                .optimize(OptimizeAction::Prune {
                    older_than: Some(
                        chrono::Duration::try_minutes(0)
                            .unwrap_or_else(|| chrono::Duration::minutes(0)),
                    ),
                    delete_unverified: Some(true),
                    error_if_tagged_old_versions: None,
                })
                .await;
        }

        Ok(())
    }

    /// Lazy compact: check version count after write ops, auto-compact when > threshold.
    /// Prevents the version bloat that causes OOM (44435 versions → 2.5G memory).
    /// NOTE: No longer called from write path. Used by PruneDaemon background task only.
    pub async fn maybe_optimize(&self) {
        let table = match self.open_table().await {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!(error = %e, "maybe_optimize: failed to open table");
                return;
            }
        };

        let version: u64 = table.version().await.unwrap_or(0);
        const COMPACT_THRESHOLD: u64 = 50;

        if version <= COMPACT_THRESHOLD {
            return;
        }

        tracing::info!(%version, threshold = COMPACT_THRESHOLD, "lazy_compact: version count exceeded threshold, running optimize");

        if let Err(e) = self.optimize().await {
            tracing::warn!(error = %e, "lazy_compact: optimize failed");
        }
    }

    /// Prune old versions without compacting — safe to run concurrently with writes.
    /// Prune only deletes manifest files, does not rewrite data, so no commit conflicts.
    pub async fn prune_old_versions(&self) -> Result<u64, OmemError> {
        let table = self.open_table().await?;
        let version: u64 = table.version().await.unwrap_or(0);

        if version <= 10 {
            return Ok(version);
        }

        let stats = table
            .optimize(OptimizeAction::Prune {
                older_than: Some(
                    chrono::Duration::try_minutes(0)
                        .unwrap_or_else(|| chrono::Duration::minutes(0)),
                ),
                delete_unverified: Some(true),
                error_if_tagged_old_versions: None,
            })
            .await
            .map_err(|e| OmemError::Storage(format!("prune failed: {e}")))?;

        let pruned = stats.prune.map(|p| p.bytes_removed).unwrap_or(0);
        tracing::info!(version_before = %version, bytes_removed = %pruned, "prune_old_versions completed");

        let new_version: u64 = table.version().await.unwrap_or(version);
        Ok(new_version)
    }
}

fn option_str(opt: &Option<String>) -> Option<&str> {
    opt.as_deref()
}

fn escape_sql(s: &str) -> String {
    s.replace('\'', "''")
}

fn sql_str(s: &str) -> String {
    format!("'{}'", escape_sql(s))
}

fn sql_opt_str(opt: &Option<String>) -> String {
    match opt {
        Some(s) => format!("'{}'", escape_sql(s)),
        None => "null".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    async fn setup() -> (LanceStore, TempDir) {
        let dir = TempDir::new().expect("failed to create temp dir");
        let store = LanceStore::new(dir.path().to_str().unwrap())
            .await
            .expect("failed to create store");
        store.init_table().await.expect("failed to init table");
        (store, dir)
    }

    fn make_memory(tenant: &str, content: &str) -> Memory {
        Memory::new(content, Category::Preferences, MemoryType::Insight, tenant)
    }

    #[tokio::test]
    async fn test_create_and_get_by_id() {
        let (store, _dir) = setup().await;
        let mem = make_memory("t-001", "user prefers dark mode");

        store.create(&mem, None).await.unwrap();

        let fetched = store.get_by_id(&mem.id).await.unwrap();
        assert!(fetched.is_some());
        let fetched = fetched.unwrap();
        assert_eq!(fetched.id, mem.id);
        assert_eq!(fetched.content, "user prefers dark mode");
        assert_eq!(fetched.tenant_id, "t-001");
        assert_eq!(fetched.category, Category::Preferences);
        assert_eq!(fetched.memory_type, MemoryType::Insight);
        assert_eq!(fetched.state, MemoryState::Active);
        assert_eq!(fetched.tier, Tier::Peripheral);
        assert!((fetched.importance - 0.5).abs() < f32::EPSILON);
        assert!((fetched.confidence - 0.5).abs() < f32::EPSILON);
        assert_eq!(fetched.access_count, 0);
        assert_eq!(fetched.scope, "global");
    }

    #[tokio::test]
    async fn test_vector_search() {
        let (store, _dir) = setup().await;

        let mut v1 = vec![0.0f32; VECTOR_DIM as usize];
        v1[0] = 1.0;
        let mut v2 = vec![0.0f32; VECTOR_DIM as usize];
        v2[0] = 0.9;
        v2[1] = 0.1;
        let mut v3 = vec![0.0f32; VECTOR_DIM as usize];
        v3[1] = 1.0;

        let m1 = make_memory("t-001", "closest match");
        let m2 = make_memory("t-001", "second closest");
        let m3 = make_memory("t-001", "furthest match");

        store.create(&m1, Some(&v1)).await.unwrap();
        store.create(&m2, Some(&v2)).await.unwrap();
        store.create(&m3, Some(&v3)).await.unwrap();

        let mut query_vec = vec![0.0f32; VECTOR_DIM as usize];
        query_vec[0] = 1.0;

        let results = store
            .vector_search(&query_vec, 3, 0.0, None, None, None)
            .await
            .unwrap();

        assert!(!results.is_empty());
        assert_eq!(results[0].0.content, "closest match");
        if results.len() >= 2 {
            assert!(results[0].1 >= results[1].1);
        }
    }

    #[tokio::test]
    async fn test_fts_search() {
        let (store, _dir) = setup().await;

        let m1 = make_memory("t-001", "rust programming language is fast");
        let m2 = make_memory("t-001", "python is a popular scripting language");
        let m3 = make_memory("t-001", "the weather is sunny today");

        store.create(&m1, None).await.unwrap();
        store.create(&m2, None).await.unwrap();
        store.create(&m3, None).await.unwrap();

        store.create_fts_index().await.unwrap();

        let results = store
            .fts_search("programming language", 10, None, None, None)
            .await
            .unwrap();

        assert!(!results.is_empty());
        let contents: Vec<&str> = results.iter().map(|(m, _)| m.content.as_str()).collect();
        assert!(contents.contains(&"rust programming language is fast"));
    }

    #[tokio::test]
    async fn test_hard_delete() {
        let (store, _dir) = setup().await;
        let mem = make_memory("t-001", "to be deleted");

        store.create(&mem, None).await.unwrap();

        let before = store.get_by_id(&mem.id).await.unwrap();
        assert!(before.is_some());
        assert_eq!(before.unwrap().state, MemoryState::Active);

        store.hard_delete(&mem.id).await.unwrap();

        let after = store.get_by_id(&mem.id).await.unwrap();
        assert!(after.is_none());
    }

    #[tokio::test]
    async fn test_list_with_pagination() {
        let (store, _dir) = setup().await;

        for i in 0..5 {
            let mem = make_memory("t-001", &format!("memory {i}"));
            store.create(&mem, None).await.unwrap();
        }

        let page1 = store.list(2, 0).await.unwrap();
        assert_eq!(page1.len(), 2);

        let page2 = store.list(2, 2).await.unwrap();
        assert_eq!(page2.len(), 2);

        let page3 = store.list(2, 4).await.unwrap();
        assert_eq!(page3.len(), 1);
    }

    #[tokio::test]
    async fn test_multi_tenant_isolation() {
        let (store_a, _dir_a) = setup().await;
        let (store_b, _dir_b) = setup().await;

        let mut va = vec![0.0f32; VECTOR_DIM as usize];
        va[0] = 1.0;
        let mut vb = vec![0.0f32; VECTOR_DIM as usize];
        vb[0] = 1.0;

        let mem_a = make_memory("tenant_A", "secret data for A");
        let mem_b = make_memory("tenant_B", "secret data for B");

        store_a.create(&mem_a, Some(&va)).await.unwrap();
        store_b.create(&mem_b, Some(&vb)).await.unwrap();

        let list_a = store_a.list(100, 0).await.unwrap();
        assert_eq!(list_a.len(), 1);
        assert_eq!(list_a[0].tenant_id, "tenant_A");

        let list_b = store_b.list(100, 0).await.unwrap();
        assert_eq!(list_b.len(), 1);
        assert_eq!(list_b[0].tenant_id, "tenant_B");
    }

    #[tokio::test]
    async fn test_list_filtered_by_category() {
        let (store, _dir) = setup().await;

        let m1 = Memory::new(
            "dark mode pref",
            Category::Preferences,
            MemoryType::Insight,
            "t-001",
        );
        let m2 = Memory::new(
            "another pref",
            Category::Preferences,
            MemoryType::Insight,
            "t-001",
        );
        let m3 = Memory::new(
            "meeting happened",
            Category::Events,
            MemoryType::Session,
            "t-001",
        );

        store.create(&m1, None).await.unwrap();
        store.create(&m2, None).await.unwrap();
        store.create(&m3, None).await.unwrap();

        let filter = ListFilter {
            category: Some("preferences".to_string()),
            ..Default::default()
        };
        let results = store.list_filtered(&filter, 100, 0).await.unwrap();
        assert_eq!(results.len(), 2);

        let filter_events = ListFilter {
            category: Some("events".to_string()),
            ..Default::default()
        };
        let results = store.list_filtered(&filter_events, 100, 0).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].content, "meeting happened");
    }

    #[tokio::test]
    async fn test_list_filtered_by_tier() {
        let (store, _dir) = setup().await;

        let mut m1 = make_memory("t-001", "core memory");
        m1.tier = Tier::Core;
        let mut m2 = make_memory("t-001", "working memory");
        m2.tier = Tier::Working;
        let m3 = make_memory("t-001", "peripheral memory");

        store.create(&m1, None).await.unwrap();
        store.create(&m2, None).await.unwrap();
        store.create(&m3, None).await.unwrap();

        let filter = ListFilter {
            tier: Some("core".to_string()),
            ..Default::default()
        };
        let results = store.list_filtered(&filter, 100, 0).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].content, "core memory");
    }

    #[tokio::test]
    async fn test_list_filtered_sort_by_importance() {
        let (store, _dir) = setup().await;

        let mut m1 = make_memory("t-001", "low importance");
        m1.importance = 0.2;
        let mut m2 = make_memory("t-001", "high importance");
        m2.importance = 0.9;
        let mut m3 = make_memory("t-001", "mid importance");
        m3.importance = 0.5;

        store.create(&m1, None).await.unwrap();
        store.create(&m2, None).await.unwrap();
        store.create(&m3, None).await.unwrap();

        let filter = ListFilter {
            sort: "importance".to_string(),
            order: "desc".to_string(),
            ..Default::default()
        };
        let results = store.list_filtered(&filter, 100, 0).await.unwrap();
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].content, "high importance");
        assert_eq!(results[1].content, "mid importance");
        assert_eq!(results[2].content, "low importance");
    }

    #[tokio::test]
    async fn test_count_filtered() {
        let (store, _dir) = setup().await;

        for i in 0..5 {
            let mem = make_memory("t-001", &format!("memory {i}"));
            store.create(&mem, None).await.unwrap();
        }

        let filter = ListFilter::default();
        let count = store.count_filtered(&filter).await.unwrap();
        assert_eq!(count, 5);

        let limited = store.list_filtered(&filter, 2, 0).await.unwrap();
        assert_eq!(limited.len(), 2);
    }

    #[test]
    fn test_visibility_filter_global() {
        let store = tokio::runtime::Runtime::new().unwrap().block_on(async {
            let dir = TempDir::new().unwrap();
            LanceStore::new(dir.path().to_str().unwrap()).await.unwrap()
        });
        let result = store.build_visibility_filter("", &[]);
        assert!(result.contains("visibility = 'global'"));
        assert!(!result.contains("private"));
    }

    #[test]
    fn test_visibility_filter_private() {
        let store = tokio::runtime::Runtime::new().unwrap().block_on(async {
            let dir = TempDir::new().unwrap();
            LanceStore::new(dir.path().to_str().unwrap()).await.unwrap()
        });
        let result = store.build_visibility_filter("agent-1", &[]);
        assert!(result.contains("visibility = 'global'"));
        assert!(result.contains("visibility = 'private' AND owner_agent_id = 'agent-1'"));
    }

    #[test]
    fn test_visibility_filter_shared() {
        let store = tokio::runtime::Runtime::new().unwrap().block_on(async {
            let dir = TempDir::new().unwrap();
            LanceStore::new(dir.path().to_str().unwrap()).await.unwrap()
        });
        let spaces = vec!["team:backend".to_string(), "org:acme".to_string()];
        let result = store.build_visibility_filter("agent-1", &spaces);
        assert!(result.contains("visibility = 'global'"));
        assert!(result.contains("visibility = 'private' AND owner_agent_id = 'agent-1'"));
        assert!(result.contains("visibility = 'shared:team:backend'"));
        assert!(result.contains("visibility = 'shared:org:acme'"));
    }

    #[test]
    fn test_visibility_filter_escapes_sql() {
        let store = tokio::runtime::Runtime::new().unwrap().block_on(async {
            let dir = TempDir::new().unwrap();
            LanceStore::new(dir.path().to_str().unwrap()).await.unwrap()
        });
        let result = store.build_visibility_filter("agent'inject", &["space'bad".to_string()]);
        assert!(result.contains("agent''inject"));
        assert!(result.contains("space''bad"));
    }

    #[tokio::test]
    async fn test_schema_evolution_adds_missing_columns() {
        let dir = TempDir::new().unwrap();
        let store = LanceStore::new(dir.path().to_str().unwrap()).await.unwrap();

        let old_schema = Arc::new(Schema::new(
            LanceStore::schema()
                .fields()
                .iter()
                .filter(|f| f.name() != "version" && f.name() != "provenance_source_id")
                .cloned()
                .collect::<Vec<_>>(),
        ));
        assert_eq!(old_schema.fields().len(), 29);

        store
            .db
            .create_empty_table(&store.table_name, old_schema)
            .execute()
            .await
            .unwrap();

        let table_before = store.open_table().await.unwrap();
        let schema_before = table_before.schema().await.unwrap();
        assert!(schema_before.field_with_name("version").is_err());
        assert!(schema_before
            .field_with_name("provenance_source_id")
            .is_err());

        store.init_table().await.unwrap();

        let table_after = store.open_table().await.unwrap();
        let schema_after = table_after.schema().await.unwrap();
        assert!(schema_after.field_with_name("version").is_ok());
        assert!(schema_after.field_with_name("provenance_source_id").is_ok());
        assert_eq!(schema_after.fields().len(), 31);
    }

    #[tokio::test]
    async fn test_init_table_idempotent() {
        let dir = TempDir::new().unwrap();
        let store = LanceStore::new(dir.path().to_str().unwrap()).await.unwrap();

        store.init_table().await.unwrap();

        let table = store.open_table().await.unwrap();
        let schema = table.schema().await.unwrap();
        let col_count = schema.fields().len();

        store.init_table().await.unwrap();

        let table2 = store.open_table().await.unwrap();
        let schema2 = table2.schema().await.unwrap();
        assert_eq!(schema2.fields().len(), col_count);
    }

    #[tokio::test]
    async fn test_find_by_provenance_source_missing_column() {
        let dir = TempDir::new().unwrap();
        let store = LanceStore::new(dir.path().to_str().unwrap()).await.unwrap();

        let old_schema = Arc::new(Schema::new(
            LanceStore::schema()
                .fields()
                .iter()
                .filter(|f| f.name() != "provenance_source_id")
                .cloned()
                .collect::<Vec<_>>(),
        ));
        store
            .db
            .create_empty_table(&store.table_name, old_schema)
            .execute()
            .await
            .unwrap();

        let result = store.find_by_provenance_source("some-id").await.unwrap();
        assert!(result.is_empty());
    }
}
