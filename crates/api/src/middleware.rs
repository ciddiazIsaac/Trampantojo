use crate::{ApiError, AppState};
use axum::{
    extract::{ConnectInfo, Request, State},
    middleware::Next,
    response::Response,
};
use std::net::SocketAddr;
use trampantojo_core::{RateLimitPlan, hash_api_key};

// ---------------------------------------------------------------------------
// Rate limiting dinámico por plan de API Key
//
// Los límites (capacity, refill_rate) se derivan del campo `plan` almacenado
// en la tabla `api_keys` de Postgres. La conversión a límites numéricos vive
// en `RateLimitPlan` (trampantojo-core) como regla de negocio, no aquí.
//
// Semántica del token bucket:
//   capacity    = tokens máximos acumulables (burst permitido)
//   refill_rate = tokens recargados por segundo (throughput sostenido)
//
// Planes actuales: Anonymous < Free < Premium < Enterprise.
// Un plan desconocido en DB cae a los límites de Free (conservador).
// ---------------------------------------------------------------------------

pub async fn rate_limit_middleware(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    req: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let api_key = req.headers().get("X-API-Key").and_then(|v| v.to_str().ok());

    let (identifier, capacity, refill_rate) = match api_key {
        Some(key) => {
            let hash = hash_api_key(key);
            match state.api_keys.find_by_hash(&hash).await {
                Ok(Some(info)) if info.is_active => {
                    let plan = RateLimitPlan::from_plan_str(&info.plan);
                    tracing::debug!(
                        plan = %plan,
                        org_id = %info.org_id,
                        "rate limit resuelto para api key autenticada"
                    );
                    (format!("api_key:{}", hash), plan.capacity(), plan.refill_rate())
                }
                Ok(_) => {
                    // Clave presentada pero inválida o inactiva — rechazo explícito.
                    // No tratamos como anónimo: si alguien envió una key, esperaba
                    // autenticarse; ignorarla silenciosamente sería confuso.
                    return Err(ApiError::Unauthorized(
                        "API Key inválida o inactiva".to_string(),
                    ));
                }
                Err(e) => {
                    tracing::error!(error = %e, "error consultando api_keys en postgres");
                    return Err(ApiError::Internal);
                }
            }
        }
        None => {
            // Sin key: rate limit por IP usando límites del plan Anonymous.
            let plan = RateLimitPlan::Anonymous;
            (format!("ip:{}", addr.ip()), plan.capacity(), plan.refill_rate())
        }
    };

    match state
        .rate_limiter
        .check_limit(&identifier, capacity, refill_rate)
        .await
    {
        Ok((allowed, _remaining)) => {
            if allowed {
                Ok(next.run(req).await)
            } else {
                Err(ApiError::TooManyRequests)
            }
        }
        Err(e) => {
            tracing::error!(error = %e, "error en rate_limiter (Redis)");
            // Fail-closed: si Redis cae, rechazamos antes que arriesgar que
            // Postgres absorba tráfico sin límite. En producción, un Redis caído
            // debería despertar una alerta de on-call antes de que esto sea notable.
            Err(ApiError::Internal)
        }
    }
}

// ---------------------------------------------------------------------------
// Middleware de JWT para proteger rutas de administración / dashboard
// ---------------------------------------------------------------------------
use jsonwebtoken::{decode, DecodingKey, Validation, Algorithm};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    pub sub: String, // email
    pub exp: usize,
    pub org_id: String,
    pub role: String,
}

pub async fn jwt_middleware(
    mut req: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let auth_header = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|val| val.to_str().ok());

    let token = match auth_header {
        Some(auth) if auth.starts_with("Bearer ") => &auth["Bearer ".len()..],
        _ => {
            return Err(ApiError::Unauthorized("Falta header de autorización o es inválido".to_string()));
        }
    };

    let jwt_secret = std::env::var("JWT_SECRET").unwrap_or_else(|_| "default_secret_for_dev".to_string());

    let token_data = match decode::<Claims>(
        token,
        &DecodingKey::from_secret(jwt_secret.as_ref()),
        &Validation::new(Algorithm::HS256),
    ) {
        Ok(data) => data,
        Err(e) => {
            tracing::debug!(error = %e, "token jwt inválido");
            return Err(ApiError::Unauthorized("Token inválido o expirado".to_string()));
        }
    };

    // Inject claims into request extensions so handlers can use them
    req.extensions_mut().insert(token_data.claims);

    Ok(next.run(req).await)
}
