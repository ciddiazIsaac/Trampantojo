// ---------------------------------------------------------------------------
// webhook.rs — Rutas del servidor webhook de WhatsApp
//
// Expone dos endpoints sobre el mismo path /webhook:
//
//   GET  /webhook  — Verificación del webhook (handshake inicial de Meta).
//                    Meta hace esta petición una sola vez al registrar el
//                    webhook. Devolvemos hub.challenge si hub.verify_token
//                    coincide con WHATSAPP_VERIFY_TOKEN del entorno.
//
//   POST /webhook  — Recepción de mensajes entrantes. Meta firma cada
//                    payload con HMAC-SHA256 usando el App Secret, que
//                    incluye en la cabecera X-Hub-Signature-256. Validamos
//                    la firma *antes* de deserializar para protegernos de
//                    payloads maliciosos o de terceros no autorizados.
//
// Diseño deliberado:
//   - La verificación de firma ocurre en un extractor (`VerifiedBody`) que
//     consume el body crudo. Axum no permite leer el body dos veces, así que
//     el extractor guarda los bytes y los pasa al handler ya verificados.
//   - No usamos `Json<T>` como extractor directo en el POST para evitar que
//     Axum deserialice antes de que podamos validar la firma.
//   - Los errores de firma devuelven 403 (no 400) para no dar pistas sobre
//     si el payload era parseable.
// ---------------------------------------------------------------------------

use axum::{
    Router,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use hmac::{Hmac, Mac};
use serde::Deserialize;
use sha2::Sha256;

use crate::AppState;

// ---------------------------------------------------------------------------
// Tipos del protocolo de Meta
// ---------------------------------------------------------------------------

/// Parámetros que Meta envía en el GET de verificación del webhook.
#[derive(Deserialize)]
pub struct VerifyParams {
    #[serde(rename = "hub.mode")]
    pub mode: Option<String>,
    #[serde(rename = "hub.verify_token")]
    pub verify_token: Option<String>,
    #[serde(rename = "hub.challenge")]
    pub challenge: Option<String>,
}

/// Estructura del payload que Meta envía en cada POST.
/// Sólo modelamos los campos que necesitamos para el flujo conversacional;
/// el resto se ignora con `#[serde(default)]` para tolerar cambios de API.
#[derive(Debug, Deserialize)]
pub struct WhatsAppPayload {
    pub object: String,
    #[serde(default)]
    pub entry: Vec<Entry>,
}

#[derive(Debug, Deserialize)]
pub struct Entry {
    #[allow(dead_code)]
    pub id: String,
    #[serde(default)]
    pub changes: Vec<Change>,
}

#[derive(Debug, Deserialize)]
pub struct Change {
    pub field: String,
    pub value: ChangeValue,
}

#[derive(Debug, Deserialize)]
pub struct ChangeValue {
    #[serde(default)]
    pub messages: Vec<InboundMessage>,
    /// Metadata del número de teléfono de negocio (phone_number_id, etc.)
    #[allow(dead_code)]
    pub metadata: Option<PhoneMetadata>,
}

#[derive(Debug, Deserialize)]
pub struct PhoneMetadata {
    #[allow(dead_code)]
    pub display_phone_number: String,
    #[allow(dead_code)]
    pub phone_number_id: String,
}

#[derive(Debug, Deserialize)]
pub struct InboundMessage {
    pub from: String,
    pub id: String,
    #[serde(rename = "type")]
    pub message_type: String,
    pub text: Option<TextBody>,
    #[allow(dead_code)]
    pub timestamp: String,
}

#[derive(Debug, Deserialize)]
pub struct TextBody {
    pub body: String,
}

// ---------------------------------------------------------------------------
// Validación de firma HMAC-SHA256 (X-Hub-Signature-256)
//
// Meta calcula: sha256(app_secret, raw_body) y lo prefija con "sha256=".
// Usamos `constant_time_eq` para la comparación final para evitar timing
// attacks, aunque en la práctica el riesgo sea bajo en este contexto.
// ---------------------------------------------------------------------------

fn verify_meta_signature(secret: &str, raw_body: &[u8], header_value: &str) -> bool {
    // El header tiene la forma "sha256=<hex_digest>"
    let Some(hex_sig) = header_value.strip_prefix("sha256=") else {
        return false;
    };

    let Ok(sig_bytes) = hex::decode(hex_sig) else {
        return false;
    };

    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
        .expect("HMAC acepta cualquier longitud de clave");
    mac.update(raw_body);
    let computed = mac.finalize().into_bytes();

    constant_time_eq::constant_time_eq(&computed, &sig_bytes)
}

// ---------------------------------------------------------------------------
// Handler GET /webhook — Verificación del webhook (handshake de Meta)
// ---------------------------------------------------------------------------

pub async fn verify_webhook(
    State(state): State<AppState>,
    Query(params): Query<VerifyParams>,
) -> Response {
    let mode = params.mode.as_deref().unwrap_or("");
    let token = params.verify_token.as_deref().unwrap_or("");
    let challenge = params.challenge.as_deref().unwrap_or("");

    if mode != "subscribe" {
        tracing::warn!(mode, "verificación de webhook: modo inesperado");
        return (StatusCode::BAD_REQUEST, "modo inválido").into_response();
    }

    if token != state.verify_token {
        tracing::warn!("verificación de webhook: verify_token incorrecto");
        return (StatusCode::FORBIDDEN, "token inválido").into_response();
    }

    tracing::info!("webhook verificado con éxito — respondiendo con challenge");
    (StatusCode::OK, challenge.to_string()).into_response()
}

// ---------------------------------------------------------------------------
// Handler POST /webhook — Recepción de mensajes entrantes
//
// Flujo:
//   1. Leer el body como bytes crudos.
//   2. Validar la firma X-Hub-Signature-256.
//   3. Deserializar el payload JSON.
//   4. Iterar sobre los mensajes y despachar al manejador de lógica.
//
// El handler devuelve siempre 200 OK si la firma es válida, incluso si no
// hay mensajes que procesar. Meta reintenta el POST si no recibe 200 en ~20 s,
// lo que crearía duplicados — responder siempre 200 a payloads válidos evita
// eso.
// ---------------------------------------------------------------------------

pub async fn receive_message(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    // --- 1. Validar firma ---
    let signature = headers
        .get("x-hub-signature-256")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if state.app_secret.is_empty() {
        // En desarrollo se puede omitir el secret, pero se avisa en el log.
        tracing::warn!("WHATSAPP_APP_SECRET no configurado — saltando validación de firma");
    } else if !verify_meta_signature(&state.app_secret, &body, signature) {
        tracing::warn!("firma X-Hub-Signature-256 inválida — request rechazado");
        return (StatusCode::FORBIDDEN, "firma inválida").into_response();
    }

    // --- 2. Deserializar ---
    let payload: WhatsAppPayload = match serde_json::from_slice(&body) {
        Ok(p) => p,
        Err(e) => {
            tracing::error!(error = %e, "no se pudo deserializar el payload de WhatsApp");
            // Aún así respondemos 200 para que Meta no reintente.
            return StatusCode::OK.into_response();
        }
    };

    // --- 3. Procesar mensajes ---
    if payload.object != "whatsapp_business_account" {
        tracing::debug!(object = %payload.object, "objeto ignorado — no es whatsapp_business_account");
        return StatusCode::OK.into_response();
    }

    for entry in &payload.entry {
        for change in &entry.changes {
            if change.field != "messages" {
                continue;
            }
            for msg in &change.value.messages {
                handle_inbound_message(msg, &state).await;
            }
        }
    }

    StatusCode::OK.into_response()
}

// ---------------------------------------------------------------------------
// Lógica de despacho de mensajes individuales
//
// Por ahora sólo loguea el mensaje recibido. En el Paso 2.2 se añadirá
// la integración con la API de Trampantojo para analizar indicadores y
// la lógica de respuesta vía la API de Cloud API de WhatsApp.
// ---------------------------------------------------------------------------

async fn handle_inbound_message(msg: &InboundMessage, _state: &AppState) {
    match msg.message_type.as_str() {
        "text" => {
            let text = msg
                .text
                .as_ref()
                .map(|t| t.body.as_str())
                .unwrap_or("<sin cuerpo>");
            tracing::info!(
                from   = %msg.from,
                msg_id = %msg.id,
                text   = text,
                "mensaje de texto recibido"
            );
            // TODO (Paso 2.2): extraer indicadores del texto y llamar al
            // endpoint /v1/check de la API de Trampantojo.
        }
        other => {
            tracing::debug!(
                from          = %msg.from,
                message_type  = other,
                "tipo de mensaje no manejado — ignorando"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Router del módulo
// ---------------------------------------------------------------------------

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/webhook", get(verify_webhook))
        .route("/webhook", post(receive_message))
}
