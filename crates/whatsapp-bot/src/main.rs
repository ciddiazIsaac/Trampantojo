mod webhook;

use axum::{routing::get, Router};
use std::net::SocketAddr;
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Inicializar variables de entorno
    dotenvy::dotenv().ok();

    // Configurar observabilidad (tracing)
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "whatsapp_bot=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("Iniciando whatsapp-bot...");

    // Construir la aplicación con las rutas del webhook
    let app = Router::new()
        // Meta requiere validación por GET y recibe eventos por POST
        .route("/webhook", get(webhook::verify_webhook).post(webhook::handle_webhook))
        .layer(TraceLayer::new_for_http());

    // Configurar la dirección del servidor (p. ej., puerto 3000 por defecto)
    let port = std::env::var("PORT").unwrap_or_else(|_| "3000".to_string());
    let addr: SocketAddr = format!("0.0.0.0:{}", port).parse()?;

    tracing::info!("Servidor escuchando en {}", addr);

    // Iniciar el servidor
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
