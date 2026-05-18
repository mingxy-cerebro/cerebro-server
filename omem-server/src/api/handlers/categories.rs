use std::sync::Arc;

use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::api::server::AppState;
use crate::domain::category::{CategoryConfig, CategoryUpdate};
use crate::domain::error::OmemError;
use crate::domain::tenant::AuthInfo;

// ── DTOs ──

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CategoryResponse {
    pub name: String,
    pub display_name: String,
    pub description: String,
    pub decision_rule: Option<String>,
    pub always_merge: bool,
    pub append_only: bool,
    pub temporal_versioned: bool,
    pub merge_supported: bool,
    pub admission_weight: f32,
    pub importance_base: f32,
    pub prompt_format: Option<String>,
    pub default_visibility: String,
    pub default_scope: String,
    pub default_ttl_days: Option<i32>,
    pub sort_order: i32,
    pub is_active: bool,
}

impl From<CategoryConfig> for CategoryResponse {
    fn from(c: CategoryConfig) -> Self {
        CategoryResponse {
            name: c.name,
            display_name: c.display_name,
            description: c.description,
            decision_rule: c.decision_rule,
            always_merge: c.always_merge,
            append_only: c.append_only,
            temporal_versioned: c.temporal_versioned,
            merge_supported: c.merge_supported,
            admission_weight: c.admission_weight,
            importance_base: c.importance_base,
            prompt_format: c.prompt_format,
            default_visibility: c.default_visibility,
            default_scope: c.default_scope,
            default_ttl_days: c.default_ttl_days,
            sort_order: c.sort_order,
            is_active: c.is_active,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CreateCategoryBody {
    pub name: String,
    pub display_name: String,
    pub description: String,
    #[serde(default)]
    pub decision_rule: Option<String>,
    #[serde(default)]
    pub always_merge: bool,
    #[serde(default)]
    pub append_only: bool,
    #[serde(default)]
    pub temporal_versioned: bool,
    #[serde(default)]
    pub merge_supported: bool,
    #[serde(default = "default_weight")]
    pub admission_weight: f32,
    #[serde(default = "default_weight")]
    pub importance_base: f32,
    #[serde(default)]
    pub prompt_format: Option<String>,
    #[serde(default = "default_visibility")]
    pub default_visibility: String,
    #[serde(default = "default_scope")]
    pub default_scope: String,
    #[serde(default)]
    pub default_ttl_days: Option<i32>,
    #[serde(default)]
    pub sort_order: i32,
    #[serde(default = "default_true")]
    pub is_active: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct UpdateCategoryBody {
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub decision_rule: Option<String>,
    pub always_merge: Option<bool>,
    pub append_only: Option<bool>,
    pub temporal_versioned: Option<bool>,
    pub merge_supported: Option<bool>,
    pub admission_weight: Option<f32>,
    pub importance_base: Option<f32>,
    pub prompt_format: Option<String>,
    pub is_active: Option<bool>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AliasBody {
    pub alias: String,
    pub target: String,
}

#[derive(Serialize)]
pub struct AliasResponse {
    pub alias: String,
    pub target: String,
}

// ── Serde defaults ──

fn default_weight() -> f32 {
    0.50
}
fn default_visibility() -> String {
    "global".to_string()
}
fn default_scope() -> String {
    "global".to_string()
}
fn default_true() -> bool {
    true
}

// ── Handlers ──

pub async fn list_categories(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
) -> Result<Json<Vec<CategoryResponse>>, OmemError> {
    let cats = state
        .category_registry
        .get_active_categories(&auth.tenant_id)?;
    Ok(Json(cats.into_iter().map(CategoryResponse::from).collect()))
}

pub async fn get_category(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
    Path(name): Path<String>,
) -> Result<Json<CategoryResponse>, OmemError> {
    let cat = state
        .category_registry
        .find_by_name(&auth.tenant_id, &name)?
        .ok_or_else(|| OmemError::NotFound(format!("category {}", name)))?;
    Ok(Json(CategoryResponse::from(cat)))
}

pub async fn create_category(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
    Json(body): Json<CreateCategoryBody>,
) -> Result<impl IntoResponse, OmemError> {
    if body.name.is_empty() {
        return Err(OmemError::Validation("name is required".to_string()));
    }
    if body.display_name.is_empty() {
        return Err(OmemError::Validation(
            "display_name is required".to_string(),
        ));
    }

    let config = CategoryConfig {
        name: body.name.to_lowercase(),
        display_name: body.display_name,
        description: body.description,
        decision_rule: body.decision_rule,
        always_merge: body.always_merge,
        append_only: body.append_only,
        temporal_versioned: body.temporal_versioned,
        merge_supported: body.merge_supported,
        admission_weight: body.admission_weight,
        importance_base: body.importance_base,
        prompt_format: body.prompt_format,
        default_visibility: body.default_visibility,
        default_scope: body.default_scope,
        default_ttl_days: body.default_ttl_days,
        sort_order: body.sort_order,
        is_active: body.is_active,
    };

    state
        .category_registry
        .create_category(&auth.tenant_id, &config)?;

    let created = state
        .category_registry
        .find_by_name(&auth.tenant_id, &config.name)?
        .ok_or_else(|| OmemError::Internal("category just created not found".to_string()))?;

    Ok((StatusCode::CREATED, Json(CategoryResponse::from(created))))
}

pub async fn update_category(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
    Path(name): Path<String>,
    Json(body): Json<UpdateCategoryBody>,
) -> Result<Json<CategoryResponse>, OmemError> {
    // Verify category exists
    state
        .category_registry
        .find_by_name(&auth.tenant_id, &name)?
        .ok_or_else(|| OmemError::NotFound(format!("category {}", name)))?;

    let update = CategoryUpdate {
        display_name: body.display_name,
        description: body.description,
        decision_rule: body.decision_rule,
        always_merge: body.always_merge,
        append_only: body.append_only,
        temporal_versioned: body.temporal_versioned,
        merge_supported: body.merge_supported,
        admission_weight: body.admission_weight,
        importance_base: body.importance_base,
        prompt_format: body.prompt_format,
        is_active: body.is_active,
    };

    state
        .category_registry
        .update_category(&auth.tenant_id, &name, &update)?;

    let updated = state
        .category_registry
        .find_by_name(&auth.tenant_id, &name)?
        .ok_or_else(|| OmemError::NotFound(format!("category {}", name)))?;

    Ok(Json(CategoryResponse::from(updated)))
}

pub async fn delete_category(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, OmemError> {
    state
        .category_registry
        .delete_category(&auth.tenant_id, &name)?;

    Ok(Json(serde_json::json!({"status": "deleted"})))
}

pub async fn list_aliases(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
) -> Result<Json<Vec<AliasResponse>>, OmemError> {
    let aliases = state.category_registry.get_aliases(&auth.tenant_id)?;
    let response: Vec<AliasResponse> = aliases
        .into_iter()
        .map(|(alias, target)| AliasResponse { alias, target })
        .collect();
    Ok(Json(response))
}

pub async fn create_alias(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
    Json(body): Json<AliasBody>,
) -> Result<impl IntoResponse, OmemError> {
    if body.alias.is_empty() {
        return Err(OmemError::Validation("alias is required".to_string()));
    }
    if body.target.is_empty() {
        return Err(OmemError::Validation("target is required".to_string()));
    }

    // Validate target category exists
    let target_exists = state
        .category_registry
        .find_by_name(&auth.tenant_id, &body.target)?
        .is_some();
    if !target_exists {
        return Err(OmemError::Validation(format!(
            "target category '{}' does not exist",
            body.target
        )));
    }

    state
        .category_registry
        .create_alias(&auth.tenant_id, &body.alias, &body.target)?;

    Ok((
        StatusCode::CREATED,
        Json(AliasResponse {
            alias: body.alias,
            target: body.target,
        }),
    ))
}

pub async fn delete_alias(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
    Path(alias): Path<String>,
) -> Result<Json<serde_json::Value>, OmemError> {
    state
        .category_registry
        .delete_alias(&auth.tenant_id, &alias)?;

    Ok(Json(serde_json::json!({"status": "deleted"})))
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    async fn setup_categories_app() -> (axum::Router, tempfile::TempDir) {
        crate::api::tests::setup_app().await
    }

    async fn seed_tenant_and_get_key(app: &axum::Router) -> String {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/tenants")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"name":"test-cats"}"#))
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response.into_body().collect().await.expect("body").to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        json["api_key"].as_str().expect("api_key").to_string()
    }

    #[tokio::test]
    async fn test_list_categories_returns_seed_data() {
        let (app, _dir) = setup_categories_app().await;
        let api_key = seed_tenant_and_get_key(&app).await;

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/categories")
                    .header("x-api-key", &api_key)
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{
                        "name": "test_cat",
                        "display_name": "Test",
                        "description": "Created for list test"
                    }"#))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::CREATED);

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/v1/categories")
                    .header("x-api-key", &api_key)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response.into_body().collect().await.expect("body").to_bytes();
        let cats: Vec<serde_json::Value> = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(cats.len(), 1);
        assert_eq!(cats[0]["name"], "test_cat");
    }

    #[tokio::test]
    async fn test_create_and_get_category() {
        let (app, _dir) = setup_categories_app().await;
        let api_key = seed_tenant_and_get_key(&app).await;

        let create_body = r#"{
            "name": "custom_cat",
            "display_name": "Custom",
            "description": "A custom category",
            "admission_weight": 0.60,
            "importance_base": 0.50
        }"#;

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/categories")
                    .header("x-api-key", &api_key)
                    .header("content-type", "application/json")
                    .body(Body::from(create_body))
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::CREATED);
        let bytes = response.into_body().collect().await.expect("body").to_bytes();
        let created: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(created["name"], "custom_cat");

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/v1/categories/custom_cat")
                    .header("x-api-key", &api_key)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response.into_body().collect().await.expect("body").to_bytes();
        let fetched: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(fetched["name"], "custom_cat");
        assert_eq!(fetched["display_name"], "Custom");
    }

    #[tokio::test]
    async fn test_update_category_weight() {
        let (app, _dir) = setup_categories_app().await;
        let api_key = seed_tenant_and_get_key(&app).await;

        let create_body = r#"{
            "name": "to_update",
            "display_name": "Before",
            "description": "Will be updated"
        }"#;
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/categories")
                    .header("x-api-key", &api_key)
                    .header("content-type", "application/json")
                    .body(Body::from(create_body))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::CREATED);

        let update_body = r#"{"admission_weight": 0.99}"#;

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/v1/categories/to_update")
                    .header("x-api-key", &api_key)
                    .header("content-type", "application/json")
                    .body(Body::from(update_body))
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response.into_body().collect().await.expect("body").to_bytes();
        let updated: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        assert!((updated["admission_weight"].as_f64().unwrap() - 0.99).abs() < 0.01);
    }

    #[tokio::test]
    async fn test_delete_category() {
        let (app, _dir) = setup_categories_app().await;
        let api_key = seed_tenant_and_get_key(&app).await;

        let create_body = r#"{
            "name": "to_delete",
            "display_name": "Will Delete",
            "description": "Will be deleted"
        }"#;
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/categories")
                    .header("x-api-key", &api_key)
                    .header("content-type", "application/json")
                    .body(Body::from(create_body))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::CREATED);

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/v1/categories/to_delete")
                    .header("x-api-key", &api_key)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/v1/categories")
                    .header("x-api-key", &api_key)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response.into_body().collect().await.expect("body").to_bytes();
        let cats: Vec<serde_json::Value> = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(cats.len(), 0);
    }
}
