use std::sync::Arc;

use axum::extract::{Extension, Query, State};
use axum::Json;
use serde::Deserialize;

use crate::api::server::AppState;
use crate::domain::error::OmemError;
use crate::domain::tenant::AuthInfo;

#[derive(Deserialize)]
pub struct ProfileQuery {
    #[serde(default)]
    pub q: String,
}

/// GET /v1/profile — Returns user profile via V2 injection builder.
/// Maintains V1 API contract for openclaw/mcp compatibility.
pub async fn get_profile(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
    Query(_params): Query<ProfileQuery>,
) -> Result<Json<serde_json::Value>, OmemError> {
    let result = state
        .injection_builder
        .build_injection(&auth.tenant_id, None)?;

    if result.content.is_empty() {
        Ok(Json(serde_json::json!({
            "static_facts": [],
            "dynamic_context": [],
            "search_results": null
        })))
    } else {
        Ok(Json(serde_json::json!({
            "static_facts": [],
            "dynamic_context": [result.content],
            "search_results": null
        })))
    }
}
