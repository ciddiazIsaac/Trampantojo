use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgPool};
use trampantojo_core::{Ioc, IocRepository, IocStatus, IndicatorType, Source, TrustScore, MergeOutcome};
use uuid::Uuid;

pub struct PgIocRepository {
    pool: PgPool,
}

impl PgIocRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

/// Fila plana tal como sale de Postgres — un espejo 1:1 de las columnas.
/// Nunca se expone fuera de este módulo; el resto del sistema solo conoce `Ioc`.
#[derive(FromRow)]
struct IocRow {
    id: Uuid,
    indicator_type: String,
    value: String,
    status: String,
    impersonates: Option<String>,
    source_kind: String,
    source_issuer: Option<String>,
    source_advisory_url: Option<String>,
    corroborations: i32,
    trust_value: f32,
    first_seen: DateTime<Utc>,
    last_seen: DateTime<Utc>,
}

impl TryFrom<IocRow> for Ioc {
    type Error = anyhow::Error;

    fn try_from(row: IocRow) -> Result<Self, Self::Error> {
        let indicator_type = match row.indicator_type.as_str() {
            "domain" => IndicatorType::Domain,
            "url" => IndicatorType::Url,
            "ip_address" => IndicatorType::IpAddress,
            "phone_number" => IndicatorType::PhoneNumber,
            "file_hash" => IndicatorType::FileHash,
            other => anyhow::bail!("indicator_type desconocido en la fila: {other}"),
        };

        let status = match row.status.as_str() {
            "active" => IocStatus::Active,
            "expired" => IocStatus::Expired,
            "disputed" => IocStatus::Disputed,
            other => anyhow::bail!("status desconocido en la fila: {other}"),
        };

        let source = match row.source_kind.as_str() {
            "official" => Source::Official {
                issuer: row.source_issuer.unwrap_or_default(),
                advisory_url: row.source_advisory_url,
            },
            "community" => Source::Community {
                corroborations: row.corroborations.max(0) as u32,
            },
            other => anyhow::bail!("source_kind desconocido en la fila: {other}"),
        };

        Ok(Ioc {
            id: row.id,
            indicator_type,
            value: row.value,
            source,
            // Los `factors` detallados no viven en Postgres (ver docs/data-model.md) —
            // al reconstruir desde esta tabla solo tenemos el valor operativo.
            trust_score: TrustScore { value: row.trust_value, factors: vec![] },
            status,
            impersonates: row.impersonates,
            first_seen: row.first_seen,
            last_seen: row.last_seen,
        })
    }
}

/// Conversión explícita a los literales exactos del enum de Postgres.
/// No usar Debug/derive acá — "IpAddress" -> "ipaddress" no es "ip_address".
fn indicator_type_to_db(t: &IndicatorType) -> &'static str {
    match t {
        IndicatorType::Domain => "domain",
        IndicatorType::Url => "url",
        IndicatorType::IpAddress => "ip_address",
        IndicatorType::PhoneNumber => "phone_number",
        IndicatorType::FileHash => "file_hash",
    }
}

fn status_to_db(s: &IocStatus) -> &'static str {
    match s {
        IocStatus::Active => "active",
        IocStatus::Expired => "expired",
        IocStatus::Disputed => "disputed",
    }
}

const SELECT_COLUMNS: &str = r#"
    id, indicator_type::text, value, status::text, impersonates,
    source_kind::text, source_issuer, source_advisory_url,
    corroborations, trust_value, first_seen, last_seen
"#;

#[async_trait]
impl IocRepository for PgIocRepository {
    async fn find_by_value(&self, value: &str) -> anyhow::Result<Option<Ioc>> {
        let row: Option<IocRow> = sqlx::query_as(&format!(
            "SELECT {SELECT_COLUMNS} FROM iocs WHERE value = $1"
        ))
        .bind(value)
        .fetch_optional(&self.pool)
        .await?;

        row.map(Ioc::try_from).transpose()
    }

    /// Lock de fila + fusión en Rust puro (`Ioc::merge`, testeada aparte)
    /// + un solo write. La transacción evita que dos ingestas concurrentes
    /// se pisen la corroboración una a la otra, y además asegura que
    /// el check de deduplicación sea atómico.
    async fn upsert(&self, incoming: &Ioc, deduplication_id: Option<&str>) -> anyhow::Result<MergeOutcome> {
        let mut tx = self.pool.begin().await?;

        let existing_row: Option<IocRow> = sqlx::query_as(&format!(
            "SELECT {SELECT_COLUMNS} FROM iocs WHERE value = $1 FOR UPDATE"
        ))
        .bind(&incoming.value)
        .fetch_optional(&mut *tx)
        .await?;

        let existing = existing_row.map(Ioc::try_from).transpose()?;
        
        // Si hay un ID de deduplicación (ej: hash de IP comunitaria),
        // intentamos insertarlo primero. Si la fila ya existía (0 rows affected),
        // abortamos la fusión: este reportante ya había votado por este indicador.
        if let Some(reporter_hash) = deduplication_id {
            let inserted = sqlx::query(
                "INSERT INTO community_reports (indicator_type, value, reporter_hash)
                 VALUES ($1::indicator_type, $2, $3)
                 ON CONFLICT DO NOTHING"
            )
            .bind(indicator_type_to_db(&incoming.indicator_type))
            .bind(&incoming.value)
            .bind(reporter_hash)
            .execute(&mut *tx)
            .await?;

            if inserted.rows_affected() == 0 {
                tx.commit().await?;
                // Devolvemos el estado actual (o el entrante si por alguna razón extraña
                // existía en la tabla de deduplicación pero no en iocs).
                return Ok(MergeOutcome {
                    ioc: existing.unwrap_or(incoming.clone()),
                    trust_before: None,
                    was_merged: false,
                });
            }
        }

        let trust_before = existing.as_ref().map(|e| e.trust_score.value);
        let merged = Ioc::merge(existing, incoming.clone());

        let (source_kind, source_issuer, source_advisory_url, corroborations) = match &merged.source {
            Source::Official { issuer, advisory_url } => {
                ("official", Some(issuer.clone()), advisory_url.clone(), 0i32)
            }
            Source::Community { corroborations } => ("community", None, None, *corroborations as i32),
        };

        sqlx::query(
            r#"
            INSERT INTO iocs (
                id, indicator_type, value, status, impersonates,
                source_kind, source_issuer, source_advisory_url, corroborations,
                trust_value, first_seen, last_seen
            ) VALUES ($1, $2::indicator_type, $3, $4::ioc_status, $5, $6::source_kind, $7, $8, $9, $10, $11, $11)
            ON CONFLICT (indicator_type, value) DO UPDATE SET
                status = EXCLUDED.status,
                source_kind = EXCLUDED.source_kind,
                source_issuer = EXCLUDED.source_issuer,
                source_advisory_url = EXCLUDED.source_advisory_url,
                corroborations = EXCLUDED.corroborations,
                trust_value = EXCLUDED.trust_value,
                last_seen = EXCLUDED.last_seen
            "#,
        )
        .bind(merged.id)
        .bind(indicator_type_to_db(&merged.indicator_type))
        .bind(&merged.value)
        .bind(status_to_db(&merged.status))
        .bind(&merged.impersonates)
        .bind(source_kind)
        .bind(source_issuer)
        .bind(source_advisory_url)
        .bind(corroborations)
        .bind(merged.trust_score.value)
        .bind(merged.last_seen)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(MergeOutcome {
            ioc: merged,
            trust_before,
            was_merged: true,
        })
    }
}
