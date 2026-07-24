use anyhow::{Context, Result};
use trampantojo_core::NotificationEvent;
use redis::{
    streams::{StreamReadOptions, StreamReadReply},
    AsyncCommands, RedisResult,
};
use std::env;
use std::time::Duration;
use tracing::{error, info, warn};

const STREAM_KEY: &str = "notifications_stream";
const GROUP_NAME: &str = "notifier_group";
const CONSUMER_NAME: &str = "notifier_consumer_1";
const MAX_RETRIES: usize = 3;
const PENDING_IDLE_TIME_MS: usize = 10_000; // 10 segundos

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt::init();

    info!("Iniciando consumer de notificaciones...");

    let redis_url = env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".into());
    let client = redis::Client::open(redis_url).context("Failed to connect to Redis")?;
    let mut con = client.get_multiplexed_tokio_connection().await?;

    // 1. Asegurarse de que el grupo y el stream existan
    // Creamos el stream (si no existe) y el grupo apuntando al final ("$")
    // MKSTREAM es vital en Redis 6+ para que funcione si el stream no existe
    let group_created: RedisResult<()> = redis::cmd("XGROUP")
        .arg("CREATE")
        .arg(STREAM_KEY)
        .arg(GROUP_NAME)
        .arg("$")
        .arg("MKSTREAM")
        .query_async(&mut con)
        .await;

    if let Err(e) = group_created {
        if !e.to_string().contains("BUSYGROUP") {
            error!("Error creando XGROUP: {:?}", e);
            return Err(e.into());
        }
    }

    let http_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
        .context("Failed to build HTTP client")?;
    let webhook_url = env::var("WEBHOOK_URL").expect("WEBHOOK_URL debe estar configurada (MVP fijo)");

    // Tarea separada o loop intercalado. Lo haremos intercalado para simplicidad del MVP.
    let mut last_pending_check = tokio::time::Instant::now();

    loop {
        // Chequear mensajes pendientes estancados cada 10 segundos
        if last_pending_check.elapsed() > Duration::from_secs(10) {
            check_and_process_pending(&mut con, &http_client, &webhook_url).await;
            last_pending_check = tokio::time::Instant::now();
        }

        // Lectura bloqueante (long-polling)
        let opts = StreamReadOptions::default()
            .group(GROUP_NAME, CONSUMER_NAME)
            .block(5000) // 5 segundos, para poder chequear PEL regularmente
            .count(10);

        let reply: RedisResult<StreamReadReply> = con.xread_options(&[STREAM_KEY], &[">"], &opts).await;

        match reply {
            Ok(stream_reply) => {
                for stream in stream_reply.keys {
                    for message in stream.ids {
                        let id = message.id.clone();
                        process_message(&mut con, &http_client, &webhook_url, id, message.map).await;
                    }
                }
            }
            Err(e) => {
                error!("Error leyendo stream: {:?}", e);
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
}

async fn check_and_process_pending(
    con: &mut redis::aio::MultiplexedConnection,
    http_client: &reqwest::Client,
    webhook_url: &str,
) {
    // Revisar XPENDING usando el formato extendido que retorna: [id, consumer, idle_time_ms, times_delivered]
    let pending: RedisResult<Vec<(String, String, usize, usize)>> = redis::cmd("XPENDING")
        .arg(STREAM_KEY)
        .arg(GROUP_NAME)
        .arg("-")
        .arg("+")
        .arg(100) // Procesar de a batches
        .query_async(con)
        .await;

    match pending {
        Ok(reply) => {
            for (id, _consumer, idle, times_delivered) in reply {
                if idle > PENDING_IDLE_TIME_MS {
                    // Decisión 3: límite de intentos
                    if times_delivered >= MAX_RETRIES {
                        error!(
                            "Mensaje {} superó el límite de intentos (entregado {} veces). Se descarta para no trabar la cola.",
                            id, times_delivered
                        );
                        let _: RedisResult<()> = con.xack(STREAM_KEY, GROUP_NAME, &[&id]).await;
                        continue;
                    }

                    // Intentar reclamarlo usando XCLAIM para nuestro consumidor
                    // Esto incrementará su times_delivered y reseteará su idle time
                    let claimed: RedisResult<redis::streams::StreamClaimReply> = redis::cmd("XCLAIM")
                        .arg(STREAM_KEY)
                        .arg(GROUP_NAME)
                        .arg(CONSUMER_NAME)
                        .arg(PENDING_IDLE_TIME_MS)
                        .arg(&id)
                        .query_async(con)
                        .await;

                    match claimed {
                        Ok(claim_reply) => {
                            for message in claim_reply.ids {
                                warn!("Reclamando y reintentando mensaje {} (intento {})", message.id, times_delivered + 1);
                                process_message(con, http_client, webhook_url, message.id, message.map).await;
                            }
                        }
                        Err(e) => {
                            error!("Error al hacer XCLAIM del mensaje {}: {:?}", id, e);
                        }
                    }
                }
            }
        }
        Err(e) => {
            error!("Error al chequear XPENDING: {:?}", e);
        }
    }
}

async fn process_message(
    con: &mut redis::aio::MultiplexedConnection,
    http_client: &reqwest::Client,
    webhook_url: &str,
    id: String,
    map: std::collections::HashMap<String, redis::Value>,
) {
    let payload_str: Option<String> = map.get("payload").and_then(|v| {
        redis::FromRedisValue::from_redis_value(v).ok()
    });

    if let Some(p) = payload_str {
        match serde_json::from_str::<NotificationEvent>(&p) {
            Ok(event) => {
                // Decisión 2: Idempotencia con SETNX y TTL
                let notified_key = format!("notified:{}", id);
                let is_new: RedisResult<bool> = redis::cmd("SETNX")
                    .arg(&notified_key)
                    .arg("1")
                    .query_async(con)
                    .await;
                    
                match is_new {
                    Ok(true) => {
                        // Expirar la key de idempotencia (por ejemplo, en 24hs)
                        let _: RedisResult<()> = con.expire(&notified_key, 86400).await;

                        info!("Procesando notificación para {}: {} (Score: {})", id, event.ioc_value, event.trust_value);
                        
                        // Decisión 4: Enviar notificación al destino fijo
                        let res = http_client.post(webhook_url)
                            .json(&event)
                            .send()
                            .await;
                            
                        match res {
                            Ok(r) if r.status().is_success() => {
                                info!("Notificación enviada exitosamente ({})", id);
                                let _: RedisResult<()> = con.xack(STREAM_KEY, GROUP_NAME, &[&id]).await;
                            }
                            Ok(r) => {
                                error!("Error al enviar notificación (HTTP {}): {:?}", r.status(), r.text().await);
                                // Borrar llave de idempotencia para permitir un reintento limpio
                                let _: RedisResult<()> = con.del(&notified_key).await;
                                // No damos XACK, queda en la PEL para ser reintentado por XPENDING
                            }
                            Err(e) => {
                                error!("Error de red al enviar notificación: {:?}", e);
                                let _: RedisResult<()> = con.del(&notified_key).await;
                                // No damos XACK
                            }
                        }
                    }
                    Ok(false) => {
                        warn!("Mensaje {} ya fue notificado (idempotencia). Acking...", id);
                        let _: RedisResult<()> = con.xack(STREAM_KEY, GROUP_NAME, &[&id]).await;
                    }
                    Err(e) => {
                        error!("Error revisando idempotencia para {}: {:?}", id, e);
                    }
                }
            }
            Err(e) => {
                error!("Error parseando NotificationEvent para {}: {:?}", id, e);
                // Ack para no trabar el stream con poison pills
                let _: RedisResult<()> = con.xack(STREAM_KEY, GROUP_NAME, &[&id]).await;
            }
        }
    } else {
        error!("Mensaje {} no tiene campo 'payload'. Acking...", id);
        let _: RedisResult<()> = con.xack(STREAM_KEY, GROUP_NAME, &[&id]).await;
    }
}
