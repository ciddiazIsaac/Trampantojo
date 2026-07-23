use axum::{
    extract::{ConnectInfo, State},
    http::{HeaderMap, StatusCode},
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
// Tipos de entrada de la API pública
//
// IndicatorType en el dominio usa #[serde(tag = "type")] para serializar como
// {"type": "domain"} — correcto para almacenamiento interno, horrible para un
// endpoint público. IndicatorTypeInput serializa/deserializa como string plano
// ("domain", "url", etc.) y luego convertimos al tipo de dominio.
// ---------------------------------------------------------------------------
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum IndicatorTypeInput {
    Domain,
    Url,
    IpAddress,
    PhoneNumber,
    FileHash,
}

impl From<IndicatorTypeInput> for IndicatorType {
    fn from(v: IndicatorTypeInput) -> Self {
        match v {
            IndicatorTypeInput::Domain => IndicatorType::Domain,
            IndicatorTypeInput::Url => IndicatorType::Url,
            IndicatorTypeInput::IpAddress => IndicatorType::IpAddress,
            IndicatorTypeInput::PhoneNumber => IndicatorType::PhoneNumber,
            IndicatorTypeInput::FileHash => IndicatorType::FileHash,
        }
    }
}

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
    trusted_proxies: Vec<ipnet::IpNet>,
}

#[derive(Deserialize)]
struct ReportParams {
    indicator_type: IndicatorTypeInput,
    value: String,
    impersonates: Option<String>,
}

/// Endpoint para que la comunidad reporte IoCs.
/// Extrae la IP de origen, la hashea (MVP de identidad) y la pasa al pipeline
/// para evitar que un solo reportante infle artificialmente la corroboración.
async fn report_indicator(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(params): Json<ReportParams>,
) -> impl IntoResponse {
    let normalized = trampantojo_core::normalize_ioc_value(&params.value);

    // Validar si la conexión TCP real viene de un proxy confiable
    let is_trusted_proxy = state
        .trusted_proxies
        .iter()
        .any(|net| net.contains(&addr.ip()));

    // Identidad liviana: proxy-aware IP solo si viene de proxy confiable
    let ip_str = if is_trusted_proxy {
        headers
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.split(',').next())
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| addr.ip().to_string())
    } else {
        addr.ip().to_string()
    };

    let mut hasher = Sha256::new();
    hasher.update(ip_str.as_bytes());
    let reporter_hash = hex::encode(hasher.finalize());

    let incoming = Ioc {
        id: uuid::Uuid::new_v4(),
        indicator_type: params.indicator_type.into(),
        value: normalized,
        source: Source::Community { corroborations: 1 },
        trust_score: TrustScore {
            value: trampantojo_core::Ioc::community_score(1),
            factors: vec![trampantojo_core::ScoreFactor {
                reason: "1 reportes comunitarios corroborados".into(),
                weight: 1.0,
            }],
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
    dotenvy::dotenv().ok();
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

    let trusted_proxies_str = std::env::var("TRUSTED_PROXIES")
        .unwrap_or_else(|_| "127.0.0.0/8,::1/128,10.0.0.0/8,172.16.0.0/12,192.168.0.0/16".to_string());
    
    let trusted_proxies: Vec<ipnet::IpNet> = trusted_proxies_str
        .split(',')
        .filter_map(|s| s.trim().parse().ok())
        .collect();

    let state = AppState {
        pipeline,
        trusted_proxies,
    };

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
