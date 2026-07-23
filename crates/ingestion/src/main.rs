use axum::{
    extract::{ConnectInfo, State},
    http::StatusCode,
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use chrono::Utc;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use sqlx::postgres::PgPoolOptions;
use std::{net::SocketAddr, sync::Arc};
use storage::{clickhouse::ClickHouseIocEventStore, postgres::PgIocRepository};
use trampantojo_core::{IndicatorType, Ioc, IocEventStore, IocRepository, IocStatus, Source, TrustScore};

// ---------------------------------------------------------------------------
// Ingestion Pipeline
//
// Orquesta la ingesta de indicadores. Implementa una asimetría deliberada:
// - Postgres es crítico. Si falla, el pipeline aborta (fail-closed).
// - ClickHouse es analítico. Si falla, se loguea y el pipeline sigue (fail-open).
// ---------------------------------------------------------------------------
pub struct IngestionPipeline {
    repo: PgIocRepository,
    event_store: ClickHouseIocEventStore,
}

impl IngestionPipeline {
    pub fn new(repo: PgIocRepository, event_store: ClickHouseIocEventStore) -> Self {
        Self { repo, event_store }
    }

    /// Ingiere un nuevo reporte (o actualización) de un indicador.
    pub async fn ingest(&self, incoming: Ioc, deduplication_id: Option<&str>) -> anyhow::Result<()> {
        // 1. Persistencia transaccional en Postgres.
        // Captura el estado `trust_before` bajo el lock FOR UPDATE
        // para evitar condiciones de carrera (TOCTOU) antes de fusionar.
        // Además, si deduplication_id está presente, se inserta en community_reports.
        let outcome = self.repo.upsert(&incoming, deduplication_id).await?;

        // Si fue descartado por el filtro de deduplicación, no hubo merge,
        // por lo tanto no registramos evento en ClickHouse.
        if !outcome.was_merged {
            return Ok(());
        }

        // 2. Registro del evento en ClickHouse (auditoría / analítica).
        // Si ClickHouse falla, logueamos el error pero NO abortamos la operación.
        // Es preferible perder un evento analítico que rechazar una alerta de seguridad real.
        if let Err(e) = self
            .event_store
            .record_scoring_event(&outcome.ioc, outcome.trust_before)
            .await
        {
            tracing::error!(
                error = %e,
                ioc_value = %outcome.ioc.value,
                "Fallo al registrar evento en ClickHouse (fail-open activado: la ingesta continúa)"
            );
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// HTTP Intake (Reportes comunitarios)
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct AppState {
    pipeline: Arc<IngestionPipeline>,
}

#[derive(Deserialize)]
struct ReportParams {
    indicator_type: IndicatorType,
    value: String,
    impersonates: Option<String>,
}

/// Endpoint para que la comunidad reporte IoCs.
/// Extrae la IP de origen, la hashea (MVP de identidad) y la pasa al pipeline
/// para evitar que un solo reportante infle artificialmente la corroboración.
async fn report_indicator(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(params): Json<ReportParams>,
) -> impl IntoResponse {
    let normalized = trampantojo_core::normalize_ioc_value(&params.value);

    // Identidad liviana: hash(IP)
    let ip_str = addr.ip().to_string();
    let mut hasher = Sha256::new();
    hasher.update(ip_str.as_bytes());
    let reporter_hash = hex::encode(hasher.finalize());

    let incoming = Ioc {
        id: uuid::Uuid::new_v4(),
        indicator_type: params.indicator_type,
        value: normalized,
        // Empezamos asumiendo 0 corroboraciones; el merge se encarga de sumar.
        source: Source::Community { corroborations: 0 },
        trust_score: TrustScore {
            value: 0.0,
            factors: vec![],
        },
        status: IocStatus::Active,
        impersonates: params.impersonates,
        first_seen: Utc::now(),
        last_seen: Utc::now(),
    };

    match state.pipeline.ingest(incoming, Some(&reporter_hash)).await {
        Ok(_) => StatusCode::ACCEPTED,
        Err(e) => {
            tracing::error!(error = %e, "Error interno ingiriendo reporte comunitario");
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL debe estar seteado");
    let clickhouse_url = std::env::var("CLICKHOUSE_URL").unwrap_or_else(|_| "http://localhost:8123".to_string());

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await?;

    let repo = PgIocRepository::new(pool);
    let event_store = ClickHouseIocEventStore::new(&clickhouse_url);
    let pipeline = Arc::new(IngestionPipeline::new(repo, event_store));

    let state = AppState { pipeline };

    let app = Router::new()
        .route("/v1/report", post(report_indicator))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8081").await?;
    tracing::info!("Ingestion intake escuchando en :8081");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;

    Ok(())
}
