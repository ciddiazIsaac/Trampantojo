use anyhow::Result;
use std::sync::Arc;
use storage::{clickhouse::ClickHouseIocEventStore, postgres::PgIocRepository};
use trampantojo_core::{
    crossed_actionable_threshold, Ioc, IocEventStore, IocRepository, NotificationEvent,
    NotificationQueue,
};

// ---------------------------------------------------------------------------
// Ingestion Pipeline
//
// Orquesta la ingesta de indicadores. Implementa una asimetría deliberada:
// - Postgres es crítico. Si falla, el pipeline aborta (fail-closed).
// - ClickHouse es analítico. Si falla, se loguea y el pipeline sigue (fail-open).
// - Redis Streams (notificaciones) es urgente pero fail-open: si falla el enqueue,
//   se loguea como error! (más ruidoso que ClickHouse) y la ingesta sigue.
//   Postgres ya confirmó el estado — no tiene sentido revertirlo porque Redis tuvo un hipo.
// ---------------------------------------------------------------------------
pub struct IngestionPipeline {
    pub(crate) repo: PgIocRepository,
    pub(crate) event_store: ClickHouseIocEventStore,
    /// Cola de notificaciones edge-triggered. `None` en tests o entornos sin Redis.
    notification_queue: Option<Arc<dyn NotificationQueue>>,
}

impl IngestionPipeline {
    pub fn new(repo: PgIocRepository, event_store: ClickHouseIocEventStore) -> Self {
        Self { repo, event_store, notification_queue: None }
    }

    /// Versión con cola de notificaciones habilitada.
    pub fn with_notification_queue(
        repo: PgIocRepository,
        event_store: ClickHouseIocEventStore,
        queue: Arc<dyn NotificationQueue>,
    ) -> Self {
        Self { repo, event_store, notification_queue: Some(queue) }
    }

    /// Ingiere un nuevo reporte (o actualización) de un indicador.
    pub async fn ingest(&self, incoming: Ioc, deduplication_id: Option<&str>) -> Result<()> {
        // 1. Persistencia transaccional en Postgres (fail-closed).
        let outcome = self.repo.upsert(&incoming, deduplication_id).await?;

        // Si fue descartado por deduplicación no hubo merge — nada más que hacer.
        if !outcome.was_merged {
            return Ok(());
        }

        // 2. Evaluación edge-triggered: ¿acaba de cruzar el umbral de notificación?
        // Se evalúa ANTES de ClickHouse para que, si Redis también falla, al menos
        // el intento de notificación ocurra lo antes posible después del commit.
        if let Some(queue) = &self.notification_queue {
            let trust_after = outcome.ioc.trust_score.value;
            if crossed_actionable_threshold(outcome.trust_before, trust_after) {
                let event = NotificationEvent {
                    ioc_value:      outcome.ioc.value.clone(),
                    indicator_type: outcome.ioc.indicator_type.clone(),
                    impersonates:   outcome.ioc.impersonates.clone(),
                    trust_value:    trust_after,
                    source:         outcome.ioc.source.clone(),
                };
                // Fail-open con error! — más ruidoso que ClickHouse porque
                // una notificación perdida tiene mayor costo operacional.
                if let Err(e) = queue.enqueue(&event).await {
                    tracing::error!(
                        error = %e,
                        ioc_value = %event.ioc_value,
                        "Fallo al encolar notificación en Redis Streams \
                         (el IoC está guardado en Postgres; revisar Redis)"
                    );
                } else {
                    tracing::info!(
                        ioc_value = %event.ioc_value,
                        trust_value = trust_after,
                        "Notificación encolada — IoC cruzó umbral de acción"
                    );
                }
            }
        }

        // 3. Registro analítico en ClickHouse (fail-open con warn).
        if let Err(e) = self
            .event_store
            .record_scoring_event(&outcome.ioc, outcome.trust_before)
            .await
        {
            tracing::warn!(
                error = %e,
                ioc_value = %outcome.ioc.value,
                "Fallo al registrar evento en ClickHouse (fail-open activado: la ingesta continúa)"
            );
        }

        Ok(())
    }
}
