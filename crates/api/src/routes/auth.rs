use crate::{ApiError, AppState};
use axum::{
    extract::State,
    Json,
};
use serde::{Deserialize, Serialize};
use jsonwebtoken::{encode, EncodingKey, Header};
use bcrypt::{hash, verify, DEFAULT_COST};
use crate::middleware::Claims;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

#[derive(Deserialize)]
pub struct RegisterRequest {
    pub email: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct AuthResponse {
    pub token: String,
    pub user_id: Uuid,
    pub email: String,
    pub org_id: Uuid,
    pub role: String,
}

pub async fn register_user(
    State(state): State<AppState>,
    Json(payload): Json<RegisterRequest>,
) -> Result<Json<AuthResponse>, ApiError> {
    if payload.password.len() < 8 {
        return Err(ApiError::BadRequest("Password must be at least 8 characters".to_string()));
    }

    // Check if user already exists
    if state.users.find_by_email(&payload.email).await.unwrap_or(None).is_some() {
        return Err(ApiError::BadRequest("User with this email already exists".to_string()));
    }

    let password_hash = hash(&payload.password, DEFAULT_COST).map_err(|e| {
        tracing::error!(error = %e, "Failed to hash password");
        ApiError::Internal
    })?;

    let org_id = Uuid::new_v4();

    let user = state.users.create_user(&payload.email, &password_hash, org_id, "admin").await.map_err(|e| {
        tracing::error!(error = %e, "Failed to create user");
        ApiError::Internal
    })?;

    let token = issue_jwt(&user.email, user.org_id.to_string(), &user.role)?;

    Ok(Json(AuthResponse {
        token,
        user_id: user.id,
        email: user.email,
        org_id: user.org_id,
        role: user.role,
    }))
}

#[derive(Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

pub async fn login_user(
    State(state): State<AppState>,
    Json(payload): Json<LoginRequest>,
) -> Result<Json<AuthResponse>, ApiError> {
    let user = match state.users.find_by_email(&payload.email).await {
        Ok(Some(u)) => u,
        _ => {
            return Err(ApiError::Unauthorized("Invalid email or password".to_string()));
        }
    };

    let is_valid = verify(&payload.password, &user.password_hash).unwrap_or(false);
    if !is_valid {
        return Err(ApiError::Unauthorized("Invalid email or password".to_string()));
    }

    let token = issue_jwt(&user.email, user.org_id.to_string(), &user.role)?;

    Ok(Json(AuthResponse {
        token,
        user_id: user.id,
        email: user.email,
        org_id: user.org_id,
        role: user.role,
    }))
}

fn issue_jwt(email: &str, org_id: String, role: &str) -> Result<String, ApiError> {
    let jwt_secret = std::env::var("JWT_SECRET").unwrap_or_else(|_| "default_secret_for_dev".to_string());
    
    let expiration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as usize
        + (24 * 3600); // 24 hours

    let claims = Claims {
        sub: email.to_string(),
        exp: expiration,
        org_id,
        role: role.to_string(),
    };

    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(jwt_secret.as_bytes()),
    ).map_err(|e| {
        tracing::error!(error = %e, "Failed to issue JWT");
        ApiError::Internal
    })
}
