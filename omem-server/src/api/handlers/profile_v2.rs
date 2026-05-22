use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::api::server::AppState;
use crate::domain::error::OmemError;
use crate::domain::tenant::AuthInfo;
use crate::profile_v2::slots::is_valid_slot_name;
use crate::profile_v2::types::*;

// ---------------------------------------------------------------------------
// Request DTOs
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct CreatePreferenceBody {
    pub slot: String,
    pub value: String,
    pub confidence: Option<f32>,
    pub scope: Option<String>,
    pub project_path: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdatePreferenceBody {
    pub value: Option<String>,
    pub confidence: Option<f32>,
    pub scope: Option<String>,
    pub project_path: Option<String>,
}

#[derive(Deserialize)]
pub struct PreferenceQuery {
    pub project_path: Option<String>,
}

#[derive(Deserialize)]
pub struct TriggerInductionBody {
    /// Optional candidate texts for induction. If empty, the engine will skip
    /// (below threshold). Caller should provide relevant memory contents.
    #[serde(default)]
    pub candidate_texts: Vec<String>,
    pub project_path: Option<String>,
}

// ---------------------------------------------------------------------------
// Response DTOs
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct PreferenceResponse {
    pub id: String,
    pub slot: String,
    pub value: String,
    pub confidence: f32,
    pub scope: String,
    pub project_path: Option<String>,
    pub source: String,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Serialize)]
pub struct StatsResponse {
    pub total: i32,
    pub by_scope: HashMap<String, i32>,
    pub by_status: HashMap<String, i32>,
    pub last_induction_at: Option<String>,
}

#[derive(Serialize)]
pub struct InjectionResponse {
    pub content: String,
    pub preference_count: i32,
    pub estimated_tokens: i32,
}

#[derive(Serialize)]
pub struct InductionRunResponse {
    pub id: String,
    pub status: String,
    pub candidate_count: i32,
    pub extracted_count: i32,
    pub error: Option<String>,
    pub started_at: String,
    pub completed_at: Option<String>,
}

#[derive(Serialize)]
pub struct VersionResponse {
    pub id: String,
    pub preference_count: i32,
    pub created_at: String,
}

#[derive(Serialize)]
pub struct ChangelogResponse {
    pub id: String,
    pub preference_id: String,
    pub action: String,
    pub old_value: Option<String>,
    pub new_value: Option<String>,
    pub source: String,
    pub created_at: String,
}

#[derive(Serialize)]
pub struct TriggerInductionResponse {
    pub run_id: Option<String>,
    pub extracted_count: usize,
    pub message: String,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn preference_to_response(pref: &Preference) -> PreferenceResponse {
    PreferenceResponse {
        id: pref.id.clone(),
        slot: pref.slot.clone(),
        value: pref.value.clone(),
        confidence: pref.confidence,
        scope: pref.scope.as_str().to_string(),
        project_path: pref.project_path.clone(),
        source: pref.source.clone(),
        status: pref.status.as_str().to_string(),
        created_at: pref.created_at.to_rfc3339(),
        updated_at: pref.updated_at.to_rfc3339(),
    }
}

fn run_to_response(run: &InductionRun) -> InductionRunResponse {
    InductionRunResponse {
        id: run.id.clone(),
        status: run.status.clone(),
        candidate_count: run.candidate_count,
        extracted_count: run.extracted_count,
        error: run.error.clone(),
        started_at: run.started_at.to_rfc3339(),
        completed_at: run.completed_at.map(|t| t.to_rfc3339()),
    }
}

fn version_to_response(v: &ProfileVersion) -> VersionResponse {
    VersionResponse {
        id: v.id.clone(),
        preference_count: v.preference_count,
        created_at: v.created_at.to_rfc3339(),
    }
}

fn changelog_to_response(entry: &ProfileChangelog) -> ChangelogResponse {
    ChangelogResponse {
        id: entry.id.clone(),
        preference_id: entry.preference_id.clone(),
        action: entry.action.clone(),
        old_value: entry.old_value.clone(),
        new_value: entry.new_value.clone(),
        source: entry.source.clone(),
        created_at: entry.created_at.to_rfc3339(),
    }
}

// ---------------------------------------------------------------------------
// CRUD — Preferences
// ---------------------------------------------------------------------------

/// GET /v2/profile/preferences — List preferences (optional project_path filter)
pub async fn get_preferences(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
    Query(params): Query<PreferenceQuery>,
) -> Result<impl IntoResponse, OmemError> {
    let store = state.profile_v2_service.store();
    let prefs = store.get_preferences(
        &auth.tenant_id,
        params.project_path.as_deref(),
    )?;
    let responses: Vec<PreferenceResponse> = prefs.iter().map(preference_to_response).collect();
    Ok((StatusCode::OK, Json(responses)))
}

/// GET /v2/profile/preferences/{id} — Get single preference
pub async fn get_preference(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, OmemError> {
    let store = state.profile_v2_service.store();
    let pref = store
        .get_preference_by_id(&id)?
        .ok_or_else(|| OmemError::NotFound(format!("preference {id}")))?;
    if pref.tenant_id != auth.tenant_id {
        return Err(OmemError::Unauthorized("not your preference".to_string()));
    }
    Ok((StatusCode::OK, Json(preference_to_response(&pref))))
}

/// POST /v2/profile/preferences — Create explicit preference
pub async fn create_preference(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
    Json(body): Json<CreatePreferenceBody>,
) -> Result<impl IntoResponse, OmemError> {
    if !is_valid_slot_name(&body.slot) {
        return Err(OmemError::Validation(format!(
            "invalid slot name: {}",
            body.slot
        )));
    }

    if body.value.trim().is_empty() {
        return Err(OmemError::Validation("value must not be empty".to_string()));
    }

    let now = Utc::now();
    let pref = Preference {
        id: uuid::Uuid::new_v4().to_string(),
        tenant_id: auth.tenant_id.clone(),
        slot: body.slot.clone(),
        value: body.value.clone(),
        confidence: body.confidence.unwrap_or(0.9),
        scope: match body.scope.as_deref() {
            Some("project") => PreferenceScope::Project,
            _ => PreferenceScope::Global,
        },
        project_path: body.project_path,
        source: "explicit".to_string(),
        status: PreferenceStatus::Active,
        last_reinforced_at: now,
        created_at: now,
        updated_at: now,
    };

    let store = state.profile_v2_service.store();
    let created = store.upsert_preference(&pref)?;
    store.invalidate_cache(&auth.tenant_id);

    // Record changelog
    store.record_changelog(&ProfileChangelog {
        id: uuid::Uuid::new_v4().to_string(),
        tenant_id: auth.tenant_id.clone(),
        preference_id: created.id.clone(),
        action: "created".to_string(),
        old_value: None,
        new_value: Some(created.value.clone()),
        source: "explicit".to_string(),
        created_at: Utc::now(),
    })?;

    Ok((StatusCode::CREATED, Json(preference_to_response(&created))))
}

/// PUT /v2/profile/preferences/{id} — Update preference value/confidence/scope
pub async fn update_preference(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
    Path(id): Path<String>,
    Json(body): Json<UpdatePreferenceBody>,
) -> Result<impl IntoResponse, OmemError> {
    let store = state.profile_v2_service.store();

    let mut pref = store
        .get_preference_by_id(&id)?
        .ok_or_else(|| OmemError::NotFound(format!("preference {id}")))?;

    // Verify ownership
    if pref.tenant_id != auth.tenant_id {
        return Err(OmemError::Unauthorized("not your preference".to_string()));
    }

    let old_value = pref.value.clone();
    let mut changed = false;

    if let Some(value) = body.value {
        if value.trim().is_empty() {
            return Err(OmemError::Validation("value must not be empty".to_string()));
        }
        pref.value = value;
        changed = true;
    }

    if let Some(confidence) = body.confidence {
        if !(0.0..=1.0).contains(&confidence) {
            return Err(OmemError::Validation(
                "confidence must be between 0.0 and 1.0".to_string(),
            ));
        }
        pref.confidence = confidence;
        changed = true;
    }

    if let Some(scope) = body.scope {
        pref.scope = match scope.as_str() {
            "project" => PreferenceScope::Project,
            "global" => PreferenceScope::Global,
            _ => return Err(OmemError::Validation(format!("invalid scope: {scope}"))),
        };
        changed = true;
    }

    if let Some(project_path) = body.project_path {
        pref.project_path = Some(project_path);
        changed = true;
    }

    if !changed {
        return Ok((StatusCode::OK, Json(preference_to_response(&pref))));
    }

    pref.updated_at = Utc::now();
    let updated = store.upsert_preference(&pref)?;
    store.invalidate_cache(&auth.tenant_id);

    // Record changelog
    store.record_changelog(&ProfileChangelog {
        id: uuid::Uuid::new_v4().to_string(),
        tenant_id: auth.tenant_id.clone(),
        preference_id: updated.id.clone(),
        action: "updated".to_string(),
        old_value: Some(old_value),
        new_value: Some(updated.value.clone()),
        source: "explicit".to_string(),
        created_at: Utc::now(),
    })?;

    // Invalidate injection cache for this tenant
    state.injection_builder.invalidate_cache(&auth.tenant_id);

    Ok((StatusCode::OK, Json(preference_to_response(&updated))))
}

/// DELETE /v2/profile/preferences/{id} — Delete preference
pub async fn delete_preference(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, OmemError> {
    let store = state.profile_v2_service.store();

    let pref = store
        .get_preference_by_id(&id)?
        .ok_or_else(|| OmemError::NotFound(format!("preference {id}")))?;

    if pref.tenant_id != auth.tenant_id {
        return Err(OmemError::Unauthorized("not your preference".to_string()));
    }

    let deleted = store.delete_preference(&id)?;
    if !deleted {
        return Err(OmemError::NotFound(format!("preference {id}")));
    }
    store.invalidate_cache(&auth.tenant_id);

    // Record changelog
    store.record_changelog(&ProfileChangelog {
        id: uuid::Uuid::new_v4().to_string(),
        tenant_id: auth.tenant_id.clone(),
        preference_id: id.clone(),
        action: "deleted".to_string(),
        old_value: Some(pref.value),
        new_value: None,
        source: "explicit".to_string(),
        created_at: Utc::now(),
    })?;

    state.injection_builder.invalidate_cache(&auth.tenant_id);

    Ok(StatusCode::NO_CONTENT.into_response())
}

// ---------------------------------------------------------------------------
// Injection
// ---------------------------------------------------------------------------

/// GET /v2/profile/inject — Build and return <cerebro-profile> injection
pub async fn get_injection(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
    Query(params): Query<PreferenceQuery>,
) -> Result<impl IntoResponse, OmemError> {
    let result = state.injection_builder.build_injection(
        &auth.tenant_id,
        params.project_path.as_deref(),
    )?;

    Ok((
        StatusCode::OK,
        Json(InjectionResponse {
            content: result.content,
            preference_count: result.preference_count,
            estimated_tokens: result.estimated_tokens,
        }),
    ))
}

// ---------------------------------------------------------------------------
// Induction
// ---------------------------------------------------------------------------

/// POST /v2/profile/induction/trigger — Manually trigger induction
pub async fn trigger_induction(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
    Json(body): Json<TriggerInductionBody>,
) -> Result<impl IntoResponse, OmemError> {
    let candidates = body.candidate_texts;

    let result = state
        .induction_engine
        .trigger_induction(&auth.tenant_id, "manual", &candidates)
        .await?;

    match result {
        Some(induction_result) => {
            state.injection_builder.invalidate_cache(&auth.tenant_id);
            Ok((
                StatusCode::OK,
                Json(TriggerInductionResponse {
                    run_id: Some(induction_result.run_id),
                    extracted_count: induction_result.extracted_count,
                    message: format!(
                        "induction completed, extracted {} preferences",
                        induction_result.extracted_count
                    ),
                }),
            ))
        }
        None => Ok((
            StatusCode::OK,
            Json(TriggerInductionResponse {
                run_id: None,
                extracted_count: 0,
                message: "induction skipped (disabled, locked, cooldown, or insufficient candidates)".to_string(),
            }),
        )),
    }
}

/// GET /v2/profile/induction/runs — Query induction run history
pub async fn get_induction_runs(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
) -> Result<impl IntoResponse, OmemError> {
    let store = state.profile_v2_service.store();
    let runs = store.get_induction_runs(&auth.tenant_id, 20)?;
    let responses: Vec<InductionRunResponse> = runs.iter().map(run_to_response).collect();
    Ok((StatusCode::OK, Json(responses)))
}

// ---------------------------------------------------------------------------
// Profile Management
// ---------------------------------------------------------------------------

/// GET /v2/profile — Get full profile (all active preferences)
pub async fn get_full_profile(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
    Query(params): Query<PreferenceQuery>,
) -> Result<impl IntoResponse, OmemError> {
    let store = state.profile_v2_service.store();
    let prefs = store.get_preferences(
        &auth.tenant_id,
        params.project_path.as_deref(),
    )?;
    let responses: Vec<PreferenceResponse> = prefs.iter().map(preference_to_response).collect();
    Ok((StatusCode::OK, Json(responses)))
}

/// GET /v2/profile/versions — Get profile version history
pub async fn get_profile_versions(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
) -> Result<impl IntoResponse, OmemError> {
    let store = state.profile_v2_service.store();
    let versions = store.get_versions(&auth.tenant_id, 20)?;
    let responses: Vec<VersionResponse> = versions.iter().map(version_to_response).collect();
    Ok((StatusCode::OK, Json(responses)))
}

/// GET /v2/profile/changelog — Get preference change log
pub async fn get_changelog(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
) -> Result<impl IntoResponse, OmemError> {
    let store = state.profile_v2_service.store();
    let entries = store.get_changelog(&auth.tenant_id, 50)?;
    let responses: Vec<ChangelogResponse> = entries.iter().map(changelog_to_response).collect();
    Ok((StatusCode::OK, Json(responses)))
}

/// GET /v2/profile/stats — Get profile statistics
pub async fn get_profile_stats(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
) -> Result<impl IntoResponse, OmemError> {
    let store = state.profile_v2_service.store();
    let prefs = store.get_preferences(&auth.tenant_id, None)?;

    let mut by_scope: HashMap<String, i32> = HashMap::new();
    let mut by_status: HashMap<String, i32> = HashMap::new();

    for pref in &prefs {
        *by_scope.entry(pref.scope.as_str().to_string()).or_insert(0) += 1;
        *by_status.entry(pref.status.as_str().to_string()).or_insert(0) += 1;
    }

    // Get last induction run timestamp
    let runs = store.get_induction_runs(&auth.tenant_id, 1)?;
    let last_induction_at = runs.first().map(|r| r.started_at.to_rfc3339());

    Ok((
        StatusCode::OK,
        Json(StatsResponse {
            total: prefs.len() as i32,
            by_scope,
            by_status,
            last_induction_at,
        }),
    ))
}
