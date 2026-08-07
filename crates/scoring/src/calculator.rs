use clickhouse::{Client, Row};
use serde::Deserialize;
use chrono::Utc;
use trampantojo_core::{ScoreFactor, TrustScore};

#[derive(Row, Deserialize)]
struct EventRow {
    trust_after: f32,
    merged_at: i64,
}

/// Calcula el nuevo trust score consultando el último evento en ClickHouse
/// y aplicando un factor de decaimiento (decay) basado en el tiempo transcurrido.
pub async fn calculate_decayed_score(
    client: &Client,
    ioc_value: &str,
) -> anyhow::Result<TrustScore> {
    // 1. Consultar el último evento para este IoC
    let query = "SELECT trust_after, merged_at FROM ioc_score_events WHERE ioc_value = ? ORDER BY merged_at DESC LIMIT 1";
    let mut cursor = client.query(query).bind(ioc_value).fetch::<EventRow>()?;

    let mut factors = Vec::new();
    let base_score;
    let last_event_time;

    if let Some(row) = cursor.next().await? {
        base_score = row.trust_after;
        last_event_time = row.merged_at;

        factors.push(ScoreFactor {
            reason: format!("Último score registrado: {}", base_score),
            weight: base_score,
        });
    } else {
        // Si no hay historial, el score es 0
        return Ok(TrustScore {
            value: 0.0,
            factors: vec![],
        });
    }

    if base_score <= 0.0 {
        return Ok(TrustScore {
            value: 0.0,
            factors,
        });
    }

    // 2. Calcular la edad del IoC en días
    let now = Utc::now().timestamp_millis();
    let age_ms = now.saturating_sub(last_event_time);
    let age_days = (age_ms as f64) / (1000.0 * 60.0 * 60.0 * 24.0);

    // 3. Aplicar decay matemático. Ejemplo: Vida media (half-life) de 30 días.
    let half_life_days = 30.0;
    let decay_factor = 0.5_f64.powf(age_days / half_life_days) as f32;
    
    let new_score = base_score * decay_factor;

    if decay_factor < 0.99 {
        factors.push(ScoreFactor {
            reason: format!("Decaimiento temporal (antigüedad: {:.1} días)", age_days),
            weight: -(base_score - new_score),
        });
    }

    Ok(TrustScore {
        value: new_score,
        factors,
    })
}
