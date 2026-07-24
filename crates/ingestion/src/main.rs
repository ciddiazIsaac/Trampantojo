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
use ingestion::IngestionPipeline;
use storage::{
    clickhouse::ClickHouseIocEventStore,
    postgres::PgIocRepository,
    redis_streams::RedisNotificationQueue,
};
use trampantojo_core::{IndicatorType, Ioc, IocStatus, Source, TrustScore};

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

    // Identidad liviana: proxy-aware IP solo si viene de proxy confiable.
    //
    // X-Forwarded-For puede ser una lista: cliente, proxy1, proxy2, ...
    // El proxy de confianza agrega la IP del remitente al *final* sin tocar
    // lo que ya venía, así que un atacante puede preescribir valores a la
    // izquierda. La única IP confiable es la primera a la izquierda de
    // la cadena de proxies conocidos, leída de derecha a izquierda.
    let ip_str = if is_trusted_proxy {
        headers
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .map(|s| {
                // Caminar de derecha a izquierda, saltando IPs de proxies
                // de confianza, y quedarse con la primera que no lo sea.
                let mut candidate: Option<std::net::IpAddr> = None;
                for part in s.split(',').rev() {
                    let trimmed = part.trim();
                    match trimmed.parse::<std::net::IpAddr>() {
                        Ok(ip) if state.trusted_proxies.iter().any(|net| net.contains(&ip)) => {
                            // Es un proxy conocido — seguir hacia la izquierda.
                            continue;
                        }
                        Ok(ip) => {
                            // Primera IP que no es proxy de confianza → cliente real.
                            candidate = Some(ip);
                            break;
                        }
                        Err(_) => {
                            // Token no parseable; detener el recorrido.
                            break;
                        }
                    }
                }
                candidate
                    .map(|ip| ip.to_string())
                    .unwrap_or_else(|| addr.ip().to_string())
            })
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

    let repo        = PgIocRepository::new(pool);
    let event_store = ClickHouseIocEventStore::new(&clickhouse_url);

    // Cola de notificaciones — opcional en desarrollo (sin Redis), requerida en producción.
    // Si REDIS_URL no está seteada se arranca sin cola y se loguea un aviso.
    let pipeline = match std::env::var("REDIS_URL") {
        Ok(redis_url) => {
            match RedisNotificationQueue::new(&redis_url).await {
                Ok(queue) => {
                    tracing::info!("Cola de notificaciones Redis Streams conectada");
                    Arc::new(IngestionPipeline::with_notification_queue(
                        repo,
                        event_store,
                        Arc::new(queue),
                    ))
                }
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        "No se pudo conectar a Redis — pipeline sin notificaciones (revisar REDIS_URL)"
                    );
                    Arc::new(IngestionPipeline::new(repo, event_store))
                }
            }
        }
        Err(_) => {
            tracing::warn!("REDIS_URL no configurada — pipeline sin notificaciones (modo desarrollo)");
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
