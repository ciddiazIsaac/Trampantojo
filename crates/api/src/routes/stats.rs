use axum::{
    extract::State,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use std::cmp::min;

use crate::{ApiError, AppState, ValidatedQuery};

#[derive(Deserialize)]
pub struct StatsParams {
    pub days: Option<u32>,
}

pub async fn get_stats(
    State(state): State<AppState>,
    ValidatedQuery(params): ValidatedQuery<StatsParams>,
) -> Result<impl IntoResponse, ApiError> {
    // Por defecto 7 días, limitamos a 90 máximo para evitar consultas muy pesadas
    let days = params.days.unwrap_or(7);
    let days = min(days, 90);

    let stats = state
        .stats_store
        .get_daily_stats(days)
        .await
        .map_err(|e| {
            tracing::error!("Error consultando stats: {:?}", e);
            ApiError::Internal
        })?;

    Ok(Json(stats))
}
