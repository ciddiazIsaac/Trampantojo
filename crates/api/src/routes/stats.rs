use axum::{Json, extract::State, response::IntoResponse};
use serde::Deserialize;

use crate::{ApiError, AppState, ValidatedQuery};

#[derive(Deserialize)]
pub struct StatsParams {
    pub days: Option<u32>,
}

pub async fn get_stats(
    State(state): State<AppState>,
    ValidatedQuery(params): ValidatedQuery<StatsParams>,
) -> Result<impl IntoResponse, ApiError> {
    // Por defecto 7 días, mínimo 1 y máximo 90 para evitar consultas vacías o muy pesadas.
    // days=0 produciría un resultado vacío sin error, lo cual es confuso para el cliente.
    let days = params.days.unwrap_or(7);
    let days = days.clamp(1, 90);

    let stats = state.stats_store.get_daily_stats(days).await.map_err(|e| {
        tracing::error!("Error consultando stats: {:?}", e);
        ApiError::Internal
    })?;

    Ok(Json(stats))
}
