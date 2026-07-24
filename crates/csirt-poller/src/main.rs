//! csirt-poller
//!
//! Binario que consume la API REST pública del CSIRT Chile, filtra alertas de
//! phishing con TLP:CLEAR y alimenta el IngestionPipeline con los IoC.
//!
//! Variables de entorno:
//!   DATABASE_URL              (requerida) — Postgres
//!   CLICKHOUSE_URL            (opcional, default: http://localhost:8123)
//!   CSIRT_POLL_INTERVAL_SECS  (opcional, default: 3600)
//!   CSIRT_API_BASE            (opcional, default: https://www.csirt.gob.cl)

mod client;
mod filter;
mod checkpoint;

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use ingestion::IngestionPipeline;
use sqlx::postgres::PgPoolOptions;
use storage::{
    clickhouse::ClickHouseIocEventStore,
    postgres::PgIocRepository,
    redis_streams::RedisNotificationQueue,
};
use trampantojo_core::{Ioc, IocStatus, ScoreFactor, Source, TrustScore};
use chrono::Utc;

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt::init();

    let database_url  = std::env::var("DATABASE_URL").expect("DATABASE_URL debe estar seteado");
    let clickhouse_url = std::env::var("CLICKHOUSE_URL")
        .unwrap_or_else(|_| "http://localhost:8123".to_string());
    let api_base = std::env::var("CSIRT_API_BASE")
        .unwrap_or_else(|_| "https://www.csirt.gob.cl".to_string());
    let poll_secs: u64 = std::env::var("CSIRT_POLL_INTERVAL_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3600);

    let pool = PgPoolOptions::new()
        .max_connections(3)
        .connect(&database_url)
        .await?;

    let repo        = PgIocRepository::new(pool.clone());
    let event_store = ClickHouseIocEventStore::new(&clickhouse_url);
    let pipeline = match std::env::var("REDIS_URL") {
        Ok(redis_url) => match RedisNotificationQueue::new(&redis_url).await {
            Ok(queue) => {
                tracing::info!("Cola de notificaciones Redis Streams conectada");
                Arc::new(IngestionPipeline::with_notification_queue(
                    repo, event_store, Arc::new(queue),
                ))
            }
            Err(e) => {
                tracing::warn!(error = %e, "No se pudo conectar a Redis — sin notificaciones");
                Arc::new(IngestionPipeline::new(repo, event_store))
            }
        },
        Err(_) => {
            tracing::warn!("REDIS_URL no configurada — sin notificaciones (modo desarrollo)");
            Arc::new(IngestionPipeline::new(repo, event_store))
        }
    };
    let http_client = client::CsirtClient::new(&api_base)?;

    tracing::info!(interval_secs = poll_secs, "CSIRT poller iniciado");

    // Si poll_secs == 0 (útil para debug manual), ejecuta una sola vez y sale.
    let run_once = poll_secs == 0;
    let interval_duration = if run_once {
        Duration::from_secs(1)
    } else {
        Duration::from_secs(poll_secs)
    };

    let mut ticker = tokio::time::interval(interval_duration);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        ticker.tick().await;
        if let Err(e) = poll_cycle(&pool, &http_client, &pipeline).await {
            tracing::error!(error = %e, "Error en ciclo de polling (se reintentará en el próximo tick)");
        }
        if run_once {
            break;
        }
    }

    Ok(())
}

/// Un ciclo completo: leer checkpoint → paginar API → filtrar → ingerir → guardar checkpoint.
async fn poll_cycle(
    pool: &sqlx::PgPool,
    http_client: &client::CsirtClient,
    pipeline: &Arc<IngestionPipeline>,
) -> Result<()> {
    // 1. Leer estado persistido
    let state = checkpoint::load(pool).await?;
    tracing::info!(
        last_polled_at = %state.last_polled_at,
        seen_codes = state.seen_codes.len(),
        "Iniciando ciclo de polling"
    );

    let mut new_codes: Vec<String> = Vec::new();
    let mut latest_date = state.last_polled_at;
    let mut page = 1u32;

    loop {
        let resp = http_client
            .fetch_alerts(state.last_polled_at, page)
            .await?;

        tracing::debug!(page, total = resp.count, items = resp.items.len(), "Página recibida");

        for alert in &resp.items {
            let code = alert.code.clone();

            // --- Filtro 1: TLP ---
            if !filter::is_tlp_public(&alert.tlp) {
                tracing::warn!(code, tlp = %alert.tlp, "Alerta descartada: TLP restringido");
                continue;
            }

            // --- Filtro 2: tipo de incidente (tag phishing) ---
            if !filter::is_phishing(&alert.tags) {
                tracing::debug!(
                    code,
                    incident_type = %alert.incident_type,
                    "Alerta descartada: tipo no soportado"
                );
                continue;
            }

            // --- Filtro 3: sin IoC ---
            if alert.iocs.is_empty() {
                tracing::debug!(code, "Alerta descartada: sin IoCs");
                continue;
            }

            // --- Filtro 4: ya procesado ---
            if state.seen_codes.contains(&code) {
                tracing::debug!(code, "Alerta ya procesada, saltando");
                continue;
            }

            // --- Ingerir cada IoC de la alerta ---
            let mut ingested = 0u32;
            for raw_ioc in &alert.iocs {
                let Some(indicator_type) = filter::map_ioc_type(&raw_ioc.ioc_type) else {
                    tracing::debug!(
                        code,
                        ioc_type = %raw_ioc.ioc_type,
                        "ioc_type no soportado, saltando"
                    );
                    continue;
                };

                // Defang + normalizar
                let clean_value = trampantojo_core::normalize_ioc_value(
                    &trampantojo_core::refang(&raw_ioc.value)
                );

                let ioc = Ioc {
                    id: uuid::Uuid::new_v4(),
                    indicator_type,
                    value: clean_value,
                    source: Source::Official {
                        issuer: "CSIRT Chile".to_string(),
                        advisory_url: Some(format!(
                            "https://www.csirt.gob.cl/alertas/{}/",
                            code.to_lowercase()
                        )),
                    },
                    trust_score: TrustScore {
                        value: 1.0,
                        factors: vec![ScoreFactor {
                            reason: format!("Alerta oficial CSIRT Chile ({})", code),
                            weight: 1.0,
                        }],
                    },
                    status: IocStatus::Active,
                    // El campo `impersonates` no está en el AlertSchema de la API —
                    // se infiere del título cuando el patrón es "Entidad - Campaña Fraudulenta".
                    impersonates: parse_impersonates(&alert.title),
                    first_seen: alert.date,
                    last_seen: Utc::now(),
                };

                match pipeline.ingest(ioc, None).await {
                    Ok(_) => ingested += 1,
                    Err(e) => tracing::error!(error = %e, code, "Error ingiriendo IoC"),
                }
            }

            tracing::info!(code, ingested, "Alerta procesada");
            new_codes.push(code);

            // Avanzar cursor si esta alerta es más reciente
            if alert.date > latest_date {
                latest_date = alert.date;
            }
        }

        // ¿Quedan más páginas?
        let fetched_so_far = (page as usize) * resp.items.len().max(1);
        if fetched_so_far >= resp.count as usize || resp.items.is_empty() {
            break;
        }
        page += 1;
    }

    // 2. Persistir nuevo checkpoint
    if !new_codes.is_empty() || latest_date > state.last_polled_at {
        checkpoint::save(pool, latest_date, &new_codes, &state.seen_codes).await?;
        tracing::info!(
            new_alerts = new_codes.len(),
            "Checkpoint actualizado"
        );
    } else {
        tracing::info!("Sin alertas nuevas en este ciclo");
    }

    Ok(())
}

/// Extrae la entidad suplantada del título cuando sigue el patrón
/// "Entidad - Campaña Fraudulenta" o "Entidad - Campaña Fraudulenta".
/// Retorna None si el patrón no aplica (ej: alertas de vulnerabilidades).
fn parse_impersonates(title: &str) -> Option<String> {
    let suffixes = [
        " - Campaña Fraudulenta",
        " - Campaign Fraudulenta",   // typo observado en algunas alertas
    ];
    for suffix in &suffixes {
        if let Some(entity) = title.strip_suffix(suffix) {
            return Some(entity.trim().to_string());
        }
    }
    None
}
