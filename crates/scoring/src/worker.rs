use std::time::Duration;
use clickhouse::Client as ChClient;
use sqlx::PgPool;
use tracing::{error, info, warn};
use trampantojo_core::{IocEventStore, IocRepository};

use crate::calculator;

pub async fn run_worker(
    pg_repo: impl IocRepository,
    pool: PgPool,
    ch_client: ChClient,
    ch_store: impl IocEventStore,
) {
    let interval = std::env::var("WORKER_INTERVAL_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3600); // 1 hora por defecto

    info!("Worker de recalificación iniciado. Intervalo: {} segundos", interval);

    loop {
        info!("Iniciando ciclo de recalificación...");
        if let Err(e) = process_active_iocs(&pg_repo, &pool, &ch_client, &ch_store).await {
            error!("Error en ciclo de recalificación: {}", e);
        }
        info!("Ciclo de recalificación finalizado. Durmiendo {} segundos...", interval);
        tokio::time::sleep(Duration::from_secs(interval)).await;
    }
}

async fn process_active_iocs(
    pg_repo: &impl IocRepository,
    pool: &PgPool,
    ch_client: &ChClient,
    ch_store: &impl IocEventStore,
) -> anyhow::Result<()> {
    let active_iocs = pg_repo.find_active().await?;
    info!("Encontrados {} IoCs activos para evaluar decaimiento.", active_iocs.len());

    let mut updated_count = 0;

    for mut ioc in active_iocs {
        let old_trust_value = ioc.trust_score.value;

        let new_score = match calculator::calculate_decayed_score(ch_client, &ioc.value).await {
            Ok(score) => score,
            Err(e) => {
                warn!("No se pudo calcular decaimiento para {}: {}", ioc.value, e);
                continue;
            }
        };

        // Si el puntaje bajó por el decay factor
        if (new_score.value - old_trust_value).abs() > f32::EPSILON {
            let result = sqlx::query("UPDATE iocs SET trust_value = $1 WHERE value = $2")
                .bind(new_score.value)
                .bind(&ioc.value)
                .execute(pool)
                .await;
                
            if let Err(e) = result {
                error!("Error al actualizar Postgres para {}: {}", ioc.value, e);
                continue;
            }

            ioc.trust_score = new_score;
            
            if let Err(e) = ch_store.record_scoring_event(&ioc, Some(old_trust_value)).await {
                error!("Error al registrar decaimiento en ClickHouse para {}: {}", ioc.value, e);
            }
            
            updated_count += 1;
        }
    }

    info!("Recalificación completa. {} IoCs actualizados.", updated_count);

    Ok(())
}
