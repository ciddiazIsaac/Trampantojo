use sqlx::postgres::PgPoolOptions;
use std::env;
use std::time::Duration;
use storage::{clickhouse::ClickHouseIocEventStore, postgres::PgIocRepository};
use tracing::info;

pub mod calculator;
pub mod worker;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt::init();

    info!("Iniciando Motor de Calificación (Scoring Engine)...");

    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL debe estar seteado (ver .env)");
    let clickhouse_url = env::var("CLICKHOUSE_URL").unwrap_or_else(|_| "http://localhost:8123".to_string());

    info!("Conectando a PostgreSQL...");
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(Duration::from_secs(2))
        .connect(&database_url)
        .await?;
    
    let _pg_repo = PgIocRepository::new(pool.clone());

    info!("Conectando a ClickHouse...");
    let _ch_store = ClickHouseIocEventStore::new(&clickhouse_url);

    let ch_client = clickhouse::Client::default().with_url(&clickhouse_url);

    info!("Motor de Calificación inicializado correctamente.");
    
    // Iniciar el loop del worker (bloqueante)
    worker::run_worker(_pg_repo, pool, ch_client, _ch_store).await;

    Ok(())
}
