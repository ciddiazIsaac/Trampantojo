//! HTTP client para la API pública del CSIRT Chile.
//! Tipado directamente contra el AlertSchema del OpenAPI oficial.

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::Deserialize;

// ---------------------------------------------------------------------------
// Tipos que mapean el AlertSchema / IOCSchema del OpenAPI
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct AlertsResponse {
    pub items: Vec<Alert>,
    pub count: u64,
}

#[derive(Debug, Deserialize)]
pub struct Alert {
    pub code: String,
    pub title: String,
    pub incident_type: String,
    pub tlp: String,
    pub tags: Vec<String>,
    pub iocs: Vec<IocEntry>,
    pub date: DateTime<Utc>,
    #[allow(dead_code)]
    pub latest_revision_created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct IocEntry {
    pub ioc_type: String,
    pub value: String,
    #[allow(dead_code)]
    pub comment: Option<String>,
}

// ---------------------------------------------------------------------------
// Cliente
// ---------------------------------------------------------------------------

pub struct CsirtClient {
    http: reqwest::Client,
    base: String,
}

impl CsirtClient {
    pub fn new(base: &str) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent("trampantojo-csirt-poller/0.1 (portafolio; contacto: ver repositorio)")
            .timeout(std::time::Duration::from_secs(30))
            .build()?;
        Ok(Self { http, base: base.trim_end_matches('/').to_string() })
    }

    /// Obtiene una página de alertas desde `from_date` en adelante.
    /// page_size fijo a 100 — la API no tiene un máximo documentado, pero
    /// 100 es conservador y cubre sobradamente el volumen diario del CSIRT.
    pub async fn fetch_alerts(
        &self,
        from_date: DateTime<Utc>,
        page: u32,
    ) -> Result<AlertsResponse> {
        let url = format!("{}/api/v1/alerts/", self.base);
        let from_str = from_date.to_rfc3339();

        let resp = self
            .http
            .get(&url)
            .query(&[
                ("from_date", from_str.as_str()),
                ("page_size", "100"),
                ("page", &page.to_string()),
            ])
            .send()
            .await?
            .error_for_status()?
            .json::<AlertsResponse>()
            .await?;

        Ok(resp)
    }
}
