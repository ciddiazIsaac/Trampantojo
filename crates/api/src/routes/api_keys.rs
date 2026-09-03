use crate::{ApiError, AppState, ValidatedJson};
use axum::{
    extract::{Extension, State},
    Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use crate::middleware::Claims;
use trampantojo_core::hash_api_key;

#[derive(Serialize)]
pub struct ApiKeyResponse {
    pub key_hash: String,
    pub org_id: Uuid,
    pub plan: String,
    pub is_active: bool,
    // Note: We only return the raw key on creation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_key: Option<String>,
}

pub async fn list_api_keys(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Vec<ApiKeyResponse>>, ApiError> {
    let org_id = Uuid::parse_str(&claims.org_id).map_err(|_| {
        ApiError::BadRequest("Invalid org_id in token".to_string())
    })?;

    let keys = state.api_keys.find_by_org(org_id).await.map_err(|e| {
        tracing::error!(error = %e, "Failed to list API keys");
        ApiError::Internal
    })?;

    let response: Vec<ApiKeyResponse> = keys.into_iter().map(|k| ApiKeyResponse {
        key_hash: k.key_hash,
        org_id: k.org_id,
        plan: k.plan,
        is_active: k.is_active,
        raw_key: None,
    }).collect();

    Ok(Json(response))
}

#[derive(Deserialize)]
pub struct CreateApiKeyRequest {
    pub plan: String, // e.g. "free", "premium"
}

pub async fn create_api_key(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    ValidatedJson(payload): ValidatedJson<CreateApiKeyRequest>,
) -> Result<Json<ApiKeyResponse>, ApiError> {
    let org_id = Uuid::parse_str(&claims.org_id).map_err(|_| {
        ApiError::BadRequest("Invalid org_id in token".to_string())
    })?;

    // Generate a secure random string for the API key using Uuid
    let raw_key = Uuid::new_v4().to_string().replace("-", "");

    // In a real scenario, we might prefix it to make it recognizable, e.g., "tr_"
    let prefixed_key = format!("tr_{}", raw_key);
    let key_hash = hash_api_key(&prefixed_key);

    let key_info = state.api_keys.create(org_id, &payload.plan, &key_hash).await.map_err(|e| {
        tracing::error!(error = %e, "Failed to create API key");
        ApiError::Internal
    })?;

    Ok(Json(ApiKeyResponse {
        key_hash: key_info.key_hash,
        org_id: key_info.org_id,
        plan: key_info.plan,
        is_active: key_info.is_active,
        raw_key: Some(prefixed_key), // Only returned once!
    }))
}
