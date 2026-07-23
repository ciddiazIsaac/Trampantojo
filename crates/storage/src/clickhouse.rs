use async_trait::async_trait;
use clickhouse::{insert::Insert, Row};
use serde::Serialize;
use trampantojo_core::{Ioc, IocEventStore, Source};
use uuid::Uuid;


// ---------------------------------------------------------------------------
// Fila de evento — espejo directo del schema de ClickHouse
//
// El crate `clickhouse` usa serde para serializar a RowBinary, así que
// el orden de los campos en el struct debe coincidir con el ORDER BY
// de la tabla (no es estrictamente necesario para Row, pero sí para
// mantener la correspondencia legible con el schema SQL).
//
// LowCardinality(String) → String en Rust (el driver lo codifica igual).
// Nullable(T) → Option<T>.
// DateTime64(3, 'UTC') → i64 en milisegundos desde epoch, via time::OffsetDateTime
// o directamente como u64. Usamos i64 con el helper de chrono.
// ---------------------------------------------------------------------------

/// Mapeado 1:1 a la tabla `ioc_score_events` de ClickHouse.
/// No se expone fuera de este módulo — el resto del sistema solo conoce `Ioc`.
#[derive(Row, Serialize)]
struct ScoreEventRow {
    /// Generado en Rust para idempotencia en reintentos.
    event_id: uuid::Uuid,

    ioc_value: String,

    /// LowCardinality(String) en CH → String en Rust, mismo encoding.
    ioc_type: String,

    /// String vacío si no aplica — evita el costo de NULL en LowCardinality.
    impersonates: String,

    source_kind: String,
    source_issuer: Option<String>,

    /// Snapshot del contador al momento de este evento.
    corroborations_after: u32,

    trust_before: Option<f32>,
    trust_after: f32,

    /// DateTime64(3, 'UTC'): milisegundos desde epoch UTC.
    /// clickhouse-rs espera i64 para DateTime64 cuando se serializa con serde.
    merged_at: i64,
}

/// Extrae el string de source_kind sin duplicar el match en el código del handler.
fn source_kind_str(s: &Source) -> &'static str {
    match s {
        Source::Official { .. } => "official",
        Source::Community { .. } => "community",
    }
}

fn indicator_type_str(t: &trampantojo_core::IndicatorType) -> &'static str {
    match t {
        trampantojo_core::IndicatorType::Domain => "domain",
        trampantojo_core::IndicatorType::Url => "url",
        trampantojo_core::IndicatorType::IpAddress => "ip_address",
        trampantojo_core::IndicatorType::PhoneNumber => "phone_number",
        trampantojo_core::IndicatorType::FileHash => "file_hash",
    }
}

// ---------------------------------------------------------------------------
// Implementación del store
// ---------------------------------------------------------------------------

pub struct ClickHouseIocEventStore {
    client: clickhouse::Client,
}

impl ClickHouseIocEventStore {
    /// `url` es la URL HTTP del servidor ClickHouse, p.ej.:
    /// "http://localhost:8123"
    ///
    /// El cliente es barato de clonar (Arc interno), así que puede
    /// compartirse sin problemas entre threads via Arc<dyn IocEventStore>.
    pub fn new(url: &str) -> Self {
        let client = clickhouse::Client::default().with_url(url);
        Self { client }
    }
}

#[async_trait]
impl IocEventStore for ClickHouseIocEventStore {
    /// Registra un evento de scoring para cada Ioc::merge ejecutado,
    /// sin importar si el trust_value cambió. Ver docs de IocEventStore
    /// en trampantojo-core para la justificación del "evento por merge".
    async fn record_scoring_event(&self, ioc: &Ioc, trust_before: Option<f32>) -> anyhow::Result<()> {
        let (source_issuer, corroborations_after) = match &ioc.source {
            Source::Official { issuer, .. } => (Some(issuer.clone()), 0u32),
            Source::Community { corroborations } => (None, *corroborations),
        };

        let row = ScoreEventRow {
            event_id: Uuid::new_v4(),
            ioc_value: ioc.value.clone(),
            ioc_type: indicator_type_str(&ioc.indicator_type).to_string(),
            impersonates: ioc.impersonates.clone().unwrap_or_default(),
            source_kind: source_kind_str(&ioc.source).to_string(),
            source_issuer,
            corroborations_after,
            trust_before,
            trust_after: ioc.trust_score.value,
            // chrono::DateTime<Utc>.timestamp_millis() → i64
            merged_at: ioc.last_seen.timestamp_millis(),
        };

        let mut insert: Insert<ScoreEventRow> = self.client.insert("ioc_score_events").await?;
        insert.write(&row).await?;
        insert.end().await?;

        Ok(())
    }

    /// Registra cuándo la API consultó un indicador y si hubo match.
    /// Útil para métricas de uso y para detectar patrones de consulta
    /// (p.ej: alguien escaneando miles de dominios seguidos).
    ///
    /// Por ahora no implementado — placeholder para la siguiente iteración.
    async fn record_verification_query(&self, _value: &str, _matched: bool) -> anyhow::Result<()> {
        // TODO: insertar en una tabla ioc_query_events separada (distinto
        // patrón de acceso que los eventos de scoring: alta cardinalidad en
        // `value`, sin impersonates, sin trust_score).
        Ok(())
    }
}
