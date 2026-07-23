use async_trait::async_trait;
use axum::{
    extract::{FromRequestParts, Query, State},
    http::{request::Parts, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use sqlx::postgres::PgPoolOptions;
use std::sync::Arc;
use storage::postgres::PgIocRepository;
use trampantojo_core::IocRepository;

// ---------------------------------------------------------------------------
// Tipo de error unificado
//
// Un solo enum cubre todos los caminos de falla de la API. Quien integre
// este servicio escribe un solo `if response.error` en vez de uno por capa.
// La alternativa (devolver StatusCode en un arm y Response en otro) ya la
// vivimos — dos formatos distintos para el mismo cliente es una inconsistencia
// de contrato, no un detalle cosmético.
// ---------------------------------------------------------------------------

enum ApiError {
    /// El request está malformado (parámetro faltante, valor inválido, etc.).
    /// El mensaje llega al cliente — debe ser genérico pero accionable.
    BadRequest(String),

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
struct ErrorBody {
    error: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            ApiError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg),
            ApiError::Internal => (
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

#[derive(Clone)]
struct AppState {
    repo: Arc<dyn IocRepository>,
}

// ---------------------------------------------------------------------------
// Tipos de request / response
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
// Handler
//
// Firma: Result<Json<CheckResponse>, ApiError>
// Ambos caminos de falla (parámetro inválido y DB caída) salen ahora por el
// mismo tipo — el cliente recibe exactamente el mismo formato JSON en los dos
// casos, solo con distinto status HTTP y mensaje.
// ---------------------------------------------------------------------------

/// El endpoint que justifica todo lo demás: esto es lo que va a llamar
/// una fintech en su checkout, o el bot de WhatsApp cuando alguien pega
/// un link sospechoso. Todo lo demás del sistema existe para que esta
/// respuesta sea rápida y confiable.
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

    // `other` en vez de `_` para evitar que el borrow checker se queje:
    // con `_` el valor se mueve en el primer arm y no podemos usarlo en el
    // segundo. Con un binding nombrado, el compilador entiende que solo uno
    // de los dos brazos se ejecuta.
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
// Punto de entrada
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let database_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL debe estar seteado (ver docker-compose.yml)");

    // acquire_timeout en 2 s: si Postgres tarda más en darnos una conexión
    // del pool, ya algo está muy mal — no tiene sentido hacer esperar al
    // cliente 30 s (el default de sqlx) para confirmar lo que ya sabíamos
    // al segundo dos. Si ves timeouts falsos-positivos bajo carga legítima
    // (no caída, solo tráfico alto), este es el primer número que ajustás.
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .acquire_timeout(std::time::Duration::from_secs(2))
        .connect(&database_url)
        .await?;

    let state = AppState {
        repo: Arc::new(PgIocRepository::new(pool)),
    };

    let app = Router::new()
        .route("/v1/check", get(check_indicator))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    tracing::info!("api escuchando en :8080");
    axum::serve(listener, app).await?;

    Ok(())
}
