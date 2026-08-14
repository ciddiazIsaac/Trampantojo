// ---------------------------------------------------------------------------
// main.rs — Punto de entrada del bot de WhatsApp
//
// Responsabilidades de este binario:
//   1. Cargar configuración desde variables de entorno (con dotenvy).
//   2. Inicializar el sistema de trazado (tracing-subscriber).
//   3. Construir el AppState con los secretos necesarios para el webhook.
//   4. Registrar las rutas de webhook y arrancar el servidor Axum.
//
// Variables de entorno esperadas:
//   WHATSAPP_VERIFY_TOKEN  — Token libre que Meta usa en el handshake GET.
//                            Debe coincidir con el que configuraste en el
//                            panel de Meta for Developers.
//   WHATSAPP_APP_SECRET    — App Secret de Meta para validar la firma
//                            X-Hub-Signature-256 de los POST entrantes.
//                            Opcional en desarrollo (se omite la validación
//                            con un warning), obligatorio en producción.
//   BOT_PORT               — Puerto en el que escucha el servidor.
//                            Default: 8081 (el 8080 lo usa la API REST).
// ---------------------------------------------------------------------------

mod parser;
mod webhook;

use axum::Router;
use tower_http::trace::TraceLayer;

// ---------------------------------------------------------------------------
// Estado compartido de la aplicación
//
// Intencionalmente minimalista en esta fase: sólo los secretos para
// autenticar los webhooks de Meta. En el Paso 2.2 se añadirá el cliente
// HTTP hacia la API de Trampantojo y, opcionalmente, el cliente de la
// Cloud API de WhatsApp para enviar respuestas.
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct AppState {
    /// Token de verificación del webhook (handshake GET de Meta).
    pub verify_token: String,
    /// App Secret para validar la firma HMAC-SHA256 del POST de Meta.
    /// Vacío en local si WHATSAPP_APP_SECRET no está definido.
    pub app_secret: String,
    // Futuro (Paso 2.2):
    // pub trampantojo_api_url: Arc<String>,
    // pub whatsapp_api_token:  Arc<String>,
    // pub http_client:         reqwest::Client,
}

// ---------------------------------------------------------------------------
// Punto de entrada
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Cargar .env si existe (no falla si no hay archivo)
    dotenvy::dotenv().ok();

    // Inicializar tracing con soporte a RUST_LOG
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "whatsapp_bot=debug,tower_http=info".parse().unwrap()),
        )
        .init();

    // --- Leer configuración ---
    let verify_token = std::env::var("WHATSAPP_VERIFY_TOKEN").unwrap_or_else(|_| {
        tracing::warn!(
            "WHATSAPP_VERIFY_TOKEN no definido — usando valor de fallback 'dev-token'. \
             Define esta variable en .env para producción."
        );
        "dev-token".to_string()
    });

    let app_secret = std::env::var("WHATSAPP_APP_SECRET").unwrap_or_else(|_| {
        tracing::warn!(
            "WHATSAPP_APP_SECRET no definido — la validación de firma estará \
             deshabilitada. Define esta variable en .env para producción."
        );
        String::new()
    });

    let port: u16 = std::env::var("BOT_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8081);

    // --- Construir estado ---
    let state = AppState {
        verify_token,
        app_secret,
    };

    // --- Construir router ---
    //
    // TraceLayer registra automáticamente método, path, status y latencia
    // de cada request, suficiente para debug sin instrumentación manual
    // en cada handler.
    let app = Router::new()
        .merge(webhook::router())
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    // --- Arrancar servidor ---
    let addr = format!("0.0.0.0:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("whatsapp-bot escuchando en :{port}");

    axum::serve(listener, app).await?;

    Ok(())
}
