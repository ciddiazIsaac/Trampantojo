use async_trait::async_trait;
use axum::{
    extract::{FromRequestParts, Query, State},
    http::{request::Parts, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use sqlx::postgres::PgPoolOptions;
use std::sync::Arc;
use storage::{
    clickhouse::ClickHouseIocEventStore,
    postgres::PgIocRepository,
    redis_streams::RedisNotificationQueue,
};
use trampantojo_core::IocRepository;
use ingestion::IngestionPipeline;

mod middleware;
mod routes;

// ---------------------------------------------------------------------------
// Tipo de error unificado
//
// Un solo enum cubre todos los caminos de falla de la API. Quien integre
// este servicio escribe un solo `if response.error` en vez de uno por capa.
// La alternativa (devolver StatusCode en un arm y Response en otro) ya la
// vivimos — dos formatos distintos para el mismo cliente es una inconsistencia
// de contrato, no un detalle cosmético.
// ---------------------------------------------------------------------------

pub enum ApiError {
    /// El request está malformado (parámetro faltante, valor inválido, etc.).
    /// El mensaje llega al cliente — debe ser genérico pero accionable.
    BadRequest(String),

    /// API Key inválida o inactiva.
    Unauthorized(String),

    /// Rate limit excedido.
    TooManyRequests,

    /// Falla de infraestructura (DB caída, timeout de pool, etc.).
    /// El detalle real va a los logs vía tracing; al cliente solo le llega
    /// un mensaje genérico para no filtrar internals.
    Internal,
}

/// Cuerpo JSON compartido para todos los errores. Un campo "error" con string
/// es suficiente para este contrato — si en el futuro necesitás error codes
/// estructurados (para i18n o para que el cliente tome decisiones), este es
/// el único lugar que cambiás.
#[derive(Serialize)]
pub struct ErrorBody {
    pub error: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            ApiError::BadRequest(msg)     => (StatusCode::BAD_REQUEST, msg),
            ApiError::Unauthorized(msg)   => (StatusCode::UNAUTHORIZED, msg),
            ApiError::TooManyRequests     => (StatusCode::TOO_MANY_REQUESTS, "demasiadas peticiones".to_string()),
            ApiError::Internal            => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "error interno, inténtalo de nuevo".to_string(),
            ),
        };
        (status, Json(ErrorBody { error: message })).into_response()
    }
}

// ---------------------------------------------------------------------------
// Extractor con rechazo tipado
//
// ValidatedQuery<T> reemplaza Query<T> directo. La diferencia: cuando Axum
// no puede parsear el query string, Query<T> devuelve texto plano; este
// extractor convierte ese rechazo en ApiError::BadRequest, manteniendo el
// formato JSON consistente con el resto de los errores.
// ---------------------------------------------------------------------------

struct ValidatedQuery<T>(T);

#[async_trait]
impl<S, T> FromRequestParts<S> for ValidatedQuery<T>
where
    T: for<'de> Deserialize<'de> + Send,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        match Query::<T>::from_request_parts(parts, state).await {
            Ok(Query(value)) => Ok(ValidatedQuery(value)),
            Err(_rejection) => {
                // _rejection descartado a propósito: su Display expone
                // internals de Axum. El mensaje fijo es más seguro y
                // suficientemente accionable para el integrador.
                Err(ApiError::BadRequest(
                    "falta el parámetro obligatorio 'value'".to_string(),
                ))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Estado compartido de la aplicación
// ---------------------------------------------------------------------------

use trampantojo_core::ApiKeyRepository;

#[derive(Clone)]
pub struct AppState {
    pub repo: Arc<dyn IocRepository>,
    pub api_keys: Arc<dyn ApiKeyRepository>,
    pub rate_limiter: Arc<storage::rate_limit::RedisRateLimiter>,
    pub pipeline: Arc<IngestionPipeline>,
    /// CIDRs que el API puede confiar para leer X-Forwarded-For.
    /// Leídos de TRUSTED_PROXIES en el arranque; inmutables en runtime.
    pub trusted_proxies: Vec<ipnet::IpNet>,
    /// Pool directo de Postgres para /healthz (SELECT 1).
    /// Separado del trait IocRepository para no contaminar el contrato
    /// de dominio con una operación puramente operacional.
    pub pg_pool: Arc<sqlx::PgPool>,
}

// ---------------------------------------------------------------------------
// Tipos de request / response de /v1/check
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct CheckParams {
    value: String,
}

#[derive(Serialize)]
struct CheckResponse {
    value: String,
    is_known_threat: bool,
    trust_value: Option<f32>,
    impersonates: Option<String>,
}

// ---------------------------------------------------------------------------
// Handler de /v1/check
//
// El endpoint que justifica todo lo demás: esto es lo que va a llamar
// una fintech en su checkout, o el bot de WhatsApp cuando alguien pega
// un link sospechoso. Todo lo demás del sistema existe para que esta
// respuesta sea rápida y confiable.
// ---------------------------------------------------------------------------

async fn check_indicator(
    State(state): State<AppState>,
    ValidatedQuery(params): ValidatedQuery<CheckParams>,
) -> Result<Json<CheckResponse>, ApiError> {
    // normalize_ioc_value vive en trampantojo-core como función pura
    // testeable: lowercase, quitar protocolo, trim, etc. No se duplica aquí.
    let normalized = trampantojo_core::normalize_ioc_value(&params.value);

    let found = state
        .repo
        .find_by_value(&normalized)
        .await
        .map_err(|e| {
            // El detalle real (mensaje de sqlx, tipo de error) va a los logs.
            // Al cliente solo llega ApiError::Internal con mensaje genérico.
            tracing::error!(error = %e, "fallo al consultar iocs en postgres");
            ApiError::Internal
        })?;

    Ok(match found {
        Some(ioc) if ioc.trust_score.is_actionable() => Json(CheckResponse {
            value: normalized,
            is_known_threat: true,
            trust_value: Some(ioc.trust_score.value),
            impersonates: ioc.impersonates,
        }),
        other => Json(CheckResponse {
            value: normalized,
            is_known_threat: false,
            trust_value: other.map(|i| i.trust_score.value),
            impersonates: None,
        }),
    })
}

// ---------------------------------------------------------------------------
// Handler de /healthz
//
// Usa SELECT 1 directo sobre el pool en lugar de find_by_value, que
// genera ruido en los logs de Postgres con cada probe de liveness/readiness
// de k8s (puede ser cientos por minuto). sqlx::query("SELECT 1") es la
// forma idiomática de verificar conectividad sin efectos secundarios.
// ---------------------------------------------------------------------------

async fn healthz(State(state): State<AppState>) -> impl IntoResponse {
    // Extraemos el pool del repo concreto para poder hacer un ping directo.
    // Si el pool no puede adquirir conexión en el timeout configurado, falla.
    match sqlx::query("SELECT 1")
        .execute(state.pg_pool.as_ref())
        .await
    {
        Ok(_)  => (StatusCode::OK, "OK"),
        Err(_) => (StatusCode::SERVICE_UNAVAILABLE, "Unavailable"),
    }
}

// ---------------------------------------------------------------------------
// Punto de entrada
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt::init();

    let database_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL debe estar seteado (ver .env)");
    let clickhouse_url = std::env::var("CLICKHOUSE_URL")
        .unwrap_or_else(|_| "http://localhost:8123".to_string());
    let redis_url = std::env::var("REDIS_URL")
        .unwrap_or_else(|_| "redis://127.0.0.1:6379".into());

    let pool = PgPoolOptions::new()
        .max_connections(10)
        .acquire_timeout(std::time::Duration::from_secs(2))
        .connect(&database_url)
        .await?;

    let redis_client = redis::Client::open(redis_url.clone())?;
    let redis_conn = redis::aio::ConnectionManager::new(redis_client).await?;

    let repo     = PgIocRepository::new(pool.clone());
    let repo_arc = Arc::new(repo.clone());

    let event_store = ClickHouseIocEventStore::new(&clickhouse_url);

    // Construir el pipeline con cola de notificaciones opcional.
    // Misma lógica de arranque que tenía ingestion/main.rs — fail-open
    // si Redis no está disponible: se loguea un warning y el pipeline
    // funciona sin notificaciones hasta que Redis vuelva.
    let pipeline = match RedisNotificationQueue::new(&redis_url).await {
        Ok(queue) => {
            tracing::info!("cola de notificaciones Redis Streams conectada");
            Arc::new(IngestionPipeline::with_notification_queue(
                repo,
                event_store,
                Arc::new(queue),
            ))
        }
        Err(e) => {
            tracing::warn!(
                error = %e,
                "no se pudo conectar a Redis para notificaciones \
                 (pipeline arranca sin cola — revisar REDIS_URL)"
            );
            Arc::new(IngestionPipeline::new(repo, event_store))
        }
    };

    let trusted_proxies_str = std::env::var("TRUSTED_PROXIES")
        .unwrap_or_else(|_| "127.0.0.0/8,::1/128,10.0.0.0/8,172.16.0.0/12,192.168.0.0/16".to_string());
    let trusted_proxies: Vec<ipnet::IpNet> = trusted_proxies_str
        .split(',')
        .filter_map(|s| s.trim().parse().ok())
        .collect();

    let state = AppState {
        repo: repo_arc.clone(),
        api_keys: repo_arc,
        rate_limiter: Arc::new(storage::rate_limit::RedisRateLimiter::new(redis_conn)),
        pipeline,
        trusted_proxies,
        pg_pool: Arc::new(pool),
    };

    // api_routes agrupa todas las rutas que requieren auth + rate limiting.
    // El route_layer se aplica una sola vez y cubre /v1/check y /v1/report.
    let api_routes = Router::new()
        .route("/v1/check", get(check_indicator))
        .route("/v1/report", post(routes::report::report_indicator))
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            middleware::rate_limit_middleware,
        ));

    let app = Router::new()
        .route("/healthz", get(healthz))
        .merge(api_routes)
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    tracing::info!("api escuchando en :8080");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await?;

    Ok(())
}
