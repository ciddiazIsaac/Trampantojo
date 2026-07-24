use anyhow::Result;
use storage::{clickhouse::ClickHouseIocEventStore, postgres::PgIocRepository};
use trampantojo_core::{Ioc, IocEventStore, IocRepository};

// ---------------------------------------------------------------------------
// Ingestion Pipeline
//
// Orquesta la ingesta de indicadores. Implementa una asimetría deliberada:
// - Postgres es crítico. Si falla, el pipeline aborta (fail-closed).
// - ClickHouse es analítico. Si falla, se loguea y el pipeline sigue (fail-open).
// ---------------------------------------------------------------------------
pub struct IngestionPipeline {
    pub(crate) repo: PgIocRepository,
    pub(crate) event_store: ClickHouseIocEventStore,
}

impl IngestionPipeline {
    pub fn new(repo: PgIocRepository, event_store: ClickHouseIocEventStore) -> Self {
        Self { repo, event_store }
    }

    /// Ingiere un nuevo reporte (o actualización) de un indicador.
    pub async fn ingest(&self, incoming: Ioc, deduplication_id: Option<&str>) -> Result<()> {
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
