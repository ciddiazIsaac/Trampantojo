//! ingestion — worker interno
//!
//! Este binario ya no sirve HTTP. La ruta POST /v1/report fue movida al
//! binario `api`, que la cubre con el mismo middleware de autenticación y
//! rate limiting que /v1/check.
//!
//! La librería `ingestion` (src/lib.rs) sigue siendo la implementación de
//! IngestionPipeline — compartida por `api` y `csirt-poller`. Este binario
//! existe como punto de extensión para futuros workers internos (ej: re-scoring
//! batch, expiración de IoCs, sincronización con feeds externos que no sean
//! REST poll).
//!
//! Variables de entorno reconocidas: ninguna por ahora.

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt::init();

    tracing::info!(
        "worker de ingestion iniciado — sin tareas activas en esta versión. \
         Extender aquí para jobs batch internos (re-scoring, expiración, etc.)"
    );

    // Placeholder: en un futuro paso este loop podría arrancar workers
    // background (ej: tokio::spawn para re-scoring periódico).
    // Por ahora el proceso termina limpiamente — el scheduler de k8s
    // o un CronJob se encargará de volver a invocarlo cuando sea necesario.
    Ok(())
}
