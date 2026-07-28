use axum::{
    extract::{ConnectInfo, State},
    http::HeaderMap,
    response::IntoResponse,
};
use chrono::Utc;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::net::SocketAddr;
use trampantojo_core::{IndicatorType, Ioc, IocStatus, Source, TrustScore};

use crate::{ApiError, AppState, ValidatedJson};

// ---------------------------------------------------------------------------
// Conversión del tipo de indicador para la API pública
//
// IndicatorType en el dominio usa #[serde(tag = "type")] — correcto para
// almacenamiento interno, pero incómodo para un endpoint público donde el
// cliente quiere enviar simplemente "domain" como string plano.
// IndicatorTypeInput serializa/deserializa como string y luego convierte
// al tipo de dominio, manteniendo el contrato público limpio.
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IndicatorTypeInput {
    Domain,
    Url,
    IpAddress,
    PhoneNumber,
    FileHash,
}

impl From<IndicatorTypeInput> for IndicatorType {
    fn from(v: IndicatorTypeInput) -> Self {
        match v {
            IndicatorTypeInput::Domain      => IndicatorType::Domain,
            IndicatorTypeInput::Url         => IndicatorType::Url,
            IndicatorTypeInput::IpAddress   => IndicatorType::IpAddress,
            IndicatorTypeInput::PhoneNumber => IndicatorType::PhoneNumber,
            IndicatorTypeInput::FileHash    => IndicatorType::FileHash,
        }
    }
}

#[derive(Deserialize)]
pub struct ReportParams {
    pub indicator_type: IndicatorTypeInput,
    pub value: String,
    pub impersonates: Option<String>,
}

// ---------------------------------------------------------------------------
// Handler
//
// Extrae la IP real del reportante con lógica proxy-aware: si la conexión
// TCP llega desde un proxy en la lista TRUSTED_PROXIES, se lee
// X-Forwarded-For y se toma la IP de origen real (descartando las de los
// proxies intermedios). En cualquier otro caso se usa la IP TCP directa.
//
// La IP nunca se almacena — se hashea inmediatamente. El reporter_hash sirve
// como gate de deduplicación para evitar que un solo origen infle artificialmente
// las corroboraciones de un IoC.
// ---------------------------------------------------------------------------

pub async fn report_indicator(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    ValidatedJson(params): ValidatedJson<ReportParams>,
) -> Result<impl IntoResponse, ApiError> {
    let normalized = trampantojo_core::normalize_ioc_value(&params.value);

    // Verificar si la conexión TCP real viene de un proxy confiable.
    let is_trusted_proxy = state
        .trusted_proxies
        .iter()
        .any(|net| net.contains(&addr.ip()));

    // Identidad liviana: proxy-aware solo si viene de proxy confiable.
    let ip_str = if is_trusted_proxy {
        headers
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .map(|s| {
                // Recorrer X-Forwarded-For en orden inverso, saltando proxies
                // conocidos, para llegar al primer hop no-proxy (el origen real).
                let mut candidate: Option<std::net::IpAddr> = None;
                for part in s.split(',').rev() {
                    let trimmed = part.trim();
                    match trimmed.parse::<std::net::IpAddr>() {
                        Ok(ip) if state.trusted_proxies.iter().any(|net| net.contains(&ip)) => {
                            continue;
                        }
                        Ok(ip) => {
                            candidate = Some(ip);
                            break;
                        }
                        Err(_) => break,
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
                reason: "1 reporte comunitario corroborado".into(),
                weight: 1.0,
            }],
        },
        status: IocStatus::Active,
        impersonates: params.impersonates,
        first_seen: Utc::now(),
        last_seen: Utc::now(),
    };

    state
        .pipeline
        .ingest(incoming, Some(&reporter_hash))
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "error interno ingiriendo reporte comunitario");
            ApiError::Internal
        })?;

    // 202 Accepted: el reporte fue recibido y procesado.
    // No bloqueamos la respuesta esperando que ClickHouse confirme.
    Ok(axum::http::StatusCode::ACCEPTED)
}
