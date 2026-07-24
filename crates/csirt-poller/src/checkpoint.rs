//! Persistencia del estado del poller en Postgres.
//!
//! La tabla `csirt_poller_state` tiene exactamente una fila (id=1).
//! La migración 0003 la crea e inserta la fila con valores por defecto.
//!
//! Usamos sqlx::query() sin macro `!` — el proyecto no tiene un servidor
//! Postgres encendido en tiempo de compilación, y el patrón establecido en
//! el resto del workspace es el builder sin verificación estática.

use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgPool};

pub struct PollerState {
    pub last_polled_at: DateTime<Utc>,
    pub seen_codes: Vec<String>,
}

/// Fila de la tabla csirt_poller_state, mapeada por nombre de columna.
#[derive(FromRow)]
struct PollerStateRow {
    last_polled_at: DateTime<Utc>,
    seen_codes: Vec<String>,
}

/// Lee el estado actual del poller. Si la fila no existe (debería haberla
/// creado la migración), usa valores por defecto seguros.
pub async fn load(pool: &PgPool) -> Result<PollerState> {
    let row: Option<PollerStateRow> = sqlx::query_as(
        "SELECT last_polled_at, seen_codes FROM csirt_poller_state WHERE id = 1",
    )
    .fetch_optional(pool)
    .await?;

    Ok(match row {
        Some(r) => PollerState {
            last_polled_at: r.last_polled_at,
            seen_codes: r.seen_codes,
        },
        None => {
            tracing::warn!(
                "Fila de estado no encontrada — usando defaults (¿falta migración 0003?)"
            );
            PollerState {
                last_polled_at: Utc::now() - chrono::Duration::days(30),
                seen_codes: Vec::new(),
            }
        }
    })
}

/// Actualiza el checkpoint tras un ciclo exitoso.
///
/// - `latest_date`: el `date` más reciente visto en el lote (avanza el cursor).
/// - `new_codes`: códigos procesados en este ciclo, a agregar a seen_codes.
/// - `existing_codes`: los seen_codes que ya había antes (para la unión en memoria).
pub async fn save(
    pool: &PgPool,
    latest_date: DateTime<Utc>,
    new_codes: &[String],
    existing_codes: &[String],
) -> Result<()> {
    // Unimos los códigos existentes con los nuevos en memoria para evitar
    // una race condition de lectura-modificación-escritura en SQL.
    let mut all_codes = existing_codes.to_vec();
    all_codes.extend_from_slice(new_codes);

    sqlx::query(
        "UPDATE csirt_poller_state \
         SET last_polled_at = GREATEST(last_polled_at, $1), \
             seen_codes      = $2 \
         WHERE id = 1",
    )
    .bind(latest_date)
    .bind(&all_codes)
    .execute(pool)
    .await?;

    Ok(())
}

