use std::sync::Arc;

use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use argon2::password_hash::SaltString;
use axum::extract::{Extension, State};
use axum::Json;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::api::server::{personal_space_id, AppState};
use crate::domain::error::OmemError;
use crate::domain::tenant::AuthInfo;

#[derive(Deserialize)]
pub struct SetVaultPasswordRequest {
    pub password: String,
}

#[derive(Deserialize)]
pub struct VerifyVaultPasswordRequest {
    pub password: String,
}

#[derive(Serialize)]
pub struct VaultStatusResponse {
    pub has_password: bool,
}

#[derive(Serialize)]
pub struct VaultVerifyResponse {
    pub valid: bool,
}

#[deprecated(since = "0.4.0", note = "Use hash_password_argon2 instead")]
fn hash_password(password: &str, salt: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(password.as_bytes());
    hasher.update(salt.as_bytes());
    hex::encode(hasher.finalize())
}

#[deprecated(since = "0.4.0", note = "Argon2 PHC format includes salt")]
fn generate_salt() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let salt: [u8; 16] = rng.gen();
    hex::encode(salt)
}

fn hash_password_argon2(password: &str) -> Result<String, String> {
    let salt = SaltString::generate(&mut rand::rngs::OsRng);
    let argon2 = Argon2::default();
    let hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| format!("Argon2 hash failed: {}", e))?;
    Ok(hash.to_string())
}

fn verify_password_argon2(password: &str, hash: &str) -> Result<bool, String> {
    let parsed = PasswordHash::new(hash)
        .map_err(|e| format!("Invalid hash format: {}", e))?;
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok())
}

/// Set or update vault password for the current space
pub async fn set_vault_password(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
    Json(req): Json<SetVaultPasswordRequest>,
) -> Result<Json<serde_json::Value>, OmemError> {
    let space_id = personal_space_id(&auth.tenant_id);
    let password_hash = hash_password_argon2(&req.password)
        .map_err(|e| OmemError::Internal(format!("failed to hash password: {e}")))?;

    state
        .space_store
        .set_vault_password(&space_id, &password_hash, "")
        .await
        .map_err(|e| OmemError::Internal(format!("failed to set vault password: {e}")))?;

    Ok(Json(serde_json::json!({"success": true})))
}

/// Verify vault password
pub async fn verify_vault_password(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
    Json(req): Json<VerifyVaultPasswordRequest>,
) -> Result<Json<VaultVerifyResponse>, OmemError> {
    let space_id = personal_space_id(&auth.tenant_id);

    let stored = state
        .space_store
        .get_vault_password(&space_id)
        .await
        .map_err(|e| OmemError::Internal(format!("failed to get vault password: {e}")))?;

    #[allow(deprecated)]
    let valid = match stored {
        Some((hash, salt)) => {
            if hash.starts_with("$argon2") {
                verify_password_argon2(&req.password, &hash)
                    .map_err(|e| OmemError::Internal(format!("failed to verify password: {e}")))?
            } else {
                let expected = hash_password(&req.password, &salt);
                if expected == hash {
                    // 旧密码验证通过，自动迁移为Argon2
                    if let Ok(new_hash) = hash_password_argon2(&req.password) {
                        let _ = state
                            .space_store
                            .set_vault_password(&space_id, &new_hash, "")
                            .await;
                    }
                    true
                } else {
                    false
                }
            }
        }
        None => false,
    };

    Ok(Json(VaultVerifyResponse { valid }))
}

/// Delete vault password
pub async fn delete_vault_password(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
) -> Result<Json<serde_json::Value>, OmemError> {
    let space_id = personal_space_id(&auth.tenant_id);

    state
        .space_store
        .delete_vault_password(&space_id)
        .await
        .map_err(|e| OmemError::Internal(format!("failed to delete vault password: {e}")))?;

    Ok(Json(serde_json::json!({"success": true})))
}

/// Get vault status (whether password is set)
pub async fn get_vault_status(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthInfo>,
) -> Result<Json<VaultStatusResponse>, OmemError> {
    let space_id = personal_space_id(&auth.tenant_id);

    let has_password = state
        .space_store
        .get_vault_password(&space_id)
        .await
        .map_err(|e| OmemError::Internal(format!("failed to get vault status: {e}")))?;

    Ok(Json(VaultStatusResponse {
        has_password: has_password.is_some(),
    }))
}
