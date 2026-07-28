use axum::{
    extract::{ConnectInfo, Request, State},
    middleware::Next,
    response::Response,
};
use std::net::SocketAddr;
use trampantojo_core::hash_api_key;
use crate::{ApiError, AppState};

// ---------------------------------------------------------------------------
// Límites del token bucket por categoría de cliente
//
// Separados en constantes nombradas para que el lector entienda la intención
// sin tener que descifrar números mágicos inline. Si los datos reales muestran
// que 5 req/s es demasiado restrictivo para anónimos, hay un solo lugar donde
// cambiarlo.
//
// Semántica: capacity = tokens máximos acumulables (burst), refill_rate =
// tokens que se recargan por segundo (throughput sostenido).
// ---------------------------------------------------------------------------

/// Clientes sin API Key: acceso libre pero reducido. Suficiente para probar
/// el endpoint o para el bot/dashboard interno, sin que un solo cliente
/// anónimo pueda saturar Postgres.
const ANON_CAPACITY: u32    = 5;
const ANON_REFILL_RATE: u32 = 1;

/// Clientes con API Key válida: límite alto pensado para integraciones reales
/// (fintechs, CERTs) que necesitan throughput sostenido. En el futuro esto
/// debería venir de `info.plan` en lugar de ser una constante global.
const KEYED_CAPACITY: u32    = 100;
const KEYED_REFILL_RATE: u32 = 100;

pub async fn rate_limit_middleware(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    req: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let api_key = req
        .headers()
        .get("X-API-Key")
        .and_then(|v| v.to_str().ok());

    let (identifier, capacity, refill_rate) = match api_key {
        Some(key) => {
            let hash = hash_api_key(key);
            match state.api_keys.find_by_hash(&hash).await {
                Ok(Some(info)) if info.is_active => {
                    // TODO (post-MVP): leer capacity/refill_rate desde info.plan
                    // para que cada organización tenga límites según su contrato.
                    (format!("api_key:{}", hash), KEYED_CAPACITY, KEYED_REFILL_RATE)
                }
                Ok(_) => {
                    // Clave presentada pero inválida o inactiva — rechazo explícito.
                    // No tratamos como anónimo: si alguien envió una key, esperaba
                    // autenticarse; ignorarla silenciosamente sería confuso.
                    return Err(ApiError::Unauthorized("API Key inválida o inactiva".to_string()));
                }
                Err(e) => {
                    tracing::error!(error = %e, "error consultando api_keys en postgres");
                    return Err(ApiError::Internal);
                }
            }
        }
        None => {
            // Sin key: rate limit por IP (anónimo).
            (format!("ip:{}", addr.ip()), ANON_CAPACITY, ANON_REFILL_RATE)
        }
    };

    match state.rate_limiter.check_limit(&identifier, capacity, refill_rate).await {
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
