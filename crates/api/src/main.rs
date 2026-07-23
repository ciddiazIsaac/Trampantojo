use axum::{
    extract::{Query, State},
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use sqlx::postgres::PgPoolOptions;
use std::sync::Arc;
use storage::postgres::PgIocRepository;
use trampantojo_core::IocRepository;

#[derive(Clone)]
struct AppState {
    repo: Arc<dyn IocRepository>,
}

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

/// El endpoint que justifica todo lo demás: esto es lo que va a llamar
/// una fintech en su checkout, o el bot de WhatsApp cuando alguien pega
/// un link sospechoso. Todo lo demás del sistema existe para que esta
/// respuesta sea rápida y confiable.
async fn check_indicator(
    State(state): State<AppState>,
    Query(params): Query<CheckParams>,
) -> Result<Json<CheckResponse>, axum::http::StatusCode> {
    let normalized = trampantojo_core::normalize_ioc_value(&params.value);

    let found = state
        .repo
        .find_by_value(&normalized)
        .await
        .map_err(|e| {
            tracing::error!("Error al consultar la base de datos: {:?}", e);
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        })?;

    match found {
        Some(ioc) if ioc.trust_score.is_actionable() => Ok(Json(CheckResponse {
            value: normalized,
            is_known_threat: true,
            trust_value: Some(ioc.trust_score.value),
            impersonates: ioc.impersonates,
        })),
        _ => Ok(Json(CheckResponse {
            value: normalized,
            is_known_threat: false,
            trust_value: found.map(|i| i.trust_score.value),
            impersonates: None,
        })),
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let database_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL debe estar seteado (ver docker-compose.yml)");

    let pool = PgPoolOptions::new()
        .max_connections(10)
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
