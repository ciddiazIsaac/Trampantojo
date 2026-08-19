use axum::{
    extract::Query,
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use tracing::{info, warn};

#[derive(Deserialize)]
pub struct VerifyQuery {
    #[serde(rename = "hub.mode")]
    pub mode: Option<String>,
    #[serde(rename = "hub.verify_token")]
    pub verify_token: Option<String>,
    #[serde(rename = "hub.challenge")]
    pub challenge: Option<String>,
}

/// Manejador para validar el webhook de Meta (GET /webhook)
pub async fn verify_webhook(Query(params): Query<VerifyQuery>) -> impl IntoResponse {
    let expected_token = std::env::var("WHATSAPP_VERIFY_TOKEN")
        .unwrap_or_else(|_| "secret_token".to_string());

    if let (Some(mode), Some(token), Some(challenge)) = (params.mode, params.verify_token, params.challenge) {
        if mode == "subscribe" && token == expected_token {
            info!("Webhook verificado exitosamente");
            return (StatusCode::OK, challenge);
        } else {
            warn!("Fallo al verificar webhook: token o modo incorrecto");
            return (StatusCode::FORBIDDEN, "Forbidden".to_string());
        }
    }

    (StatusCode::BAD_REQUEST, "Bad Request".to_string())
}

/// Manejador para recibir mensajes/eventos de WhatsApp (POST /webhook)
pub async fn handle_webhook(Json(payload): Json<serde_json::Value>) -> impl IntoResponse {
    info!("Recibido evento de webhook: {}", payload);
    
    // Aquí se procesaría el payload (extraer mensajes, notificar a Kafka/Redis, etc.)

    (StatusCode::OK, "EVENT_RECEIVED")
}
