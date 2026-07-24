//! Implementación de `NotificationQueue` sobre Redis Streams.
//!
//! # Garantías de entrega
//! Redis Streams con consumer group proveen semántica `at-least-once`:
//! el mensaje persiste en el stream hasta que un consumer lo lee y ACKea
//! explícitamente. Si el proceso notifier se cae antes de hacer ACK, el
//! mensaje queda en estado "pending" y puede ser reclamado — no se pierde.
//! Es la primera pieza del sistema donde deliberadamente elegimos más
//! garantías que en el canal analítico (ClickHouse, fail-open/best-effort).
//!
//! # Formato del mensaje
//! El payload de `NotificationEvent` se serializa como JSON y se almacena
//! en el campo "data" del entry. Simple y suficiente para el MVP.

use anyhow::Result;
use async_trait::async_trait;
use redis::aio::ConnectionManager;
use redis::AsyncCommands;
use trampantojo_core::{NotificationEvent, NotificationQueue};

pub const STREAM_KEY: &str = "trampantojo:notifications";
pub const CONSUMER_GROUP: &str = "notifiers";

pub struct RedisNotificationQueue {
    conn: ConnectionManager,
}

impl RedisNotificationQueue {
    /// Conecta y, en el primer arranque, crea el consumer group si no existe.
    /// `MKSTREAM` garantiza que el stream se crea aunque no haya mensajes todavía.
    pub async fn new(redis_url: &str) -> Result<Self> {
        let client = redis::Client::open(redis_url)?;
        let conn = ConnectionManager::new(client).await?;
        let mut q = Self { conn };
        q.ensure_consumer_group().await?;
        Ok(q)
    }

    async fn ensure_consumer_group(&mut self) -> Result<()> {
        // XGROUP CREATE <stream> <group> $ MKSTREAM
        // "$" → el consumer group solo ve mensajes nuevos (no procesa historial).
        // Retorna OK si se creó, BUSYGROUP si ya existía — ambos son éxito.
        let result: redis::RedisResult<()> = redis::cmd("XGROUP")
            .arg("CREATE")
            .arg(STREAM_KEY)
            .arg(CONSUMER_GROUP)
            .arg("$")
            .arg("MKSTREAM")
            .query_async(&mut self.conn)
            .await;

        match result {
            Ok(_) => Ok(()),
            Err(e) if e.to_string().contains("BUSYGROUP") => Ok(()), // ya existe — OK
            Err(e) => Err(e.into()),
        }
    }
}

#[async_trait]
impl NotificationQueue for RedisNotificationQueue {
    async fn enqueue(&self, event: &NotificationEvent) -> Result<()> {
        let payload = serde_json::to_string(event)?;

        // XADD <stream> * data <json>
        // "*" → Redis asigna el ID automáticamente (timestamp + secuencia).
        let _: String = self
            .conn
            .clone()
            .xadd(STREAM_KEY, "*", &[("data", payload)])
            .await?;

        Ok(())
    }
}
