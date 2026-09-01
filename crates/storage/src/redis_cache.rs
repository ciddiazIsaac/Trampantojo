//! Caché Redis para consultas de lectura de IoC.
//!
//! # Estrategia: Cache-Aside (read-through manual)
//!
//! 1. El caller consulta [`RedisIocCache::get`] primero.
//! 2. Si hay hit, usa el resultado cacheado directamente.
//! 3. Si hay miss, consulta Postgres y llama a [`RedisIocCache::set`]
//!    en un `tokio::spawn` para no añadir latencia al camino crítico.
//!
//! # Negative caching
//!
//! Se cachean tanto `Some(Ioc)` como `None` (valor no encontrado). El TTL
//! para los negativos es deliberadamente más corto (`ttl / 6`) para que un
//! IoC recién ingestado aparezca relativamente pronto sin martillar Postgres
//! en cada request de un valor inexistente.
//!
//! # Clave Redis
//!
//! `ioc:cache:<valor_normalizado>` — el valor ya debe llegar normalizado
//! (lowercase, sin protocolo, sin www.) igual que en Postgres.

use redis::{AsyncCommands, aio::ConnectionManager};
use trampantojo_core::Ioc;

/// Prefijo de todas las claves de caché de IoC.
const KEY_PREFIX: &str = "ioc:cache:";

/// TTL para resultados negativos (valor no encontrado), expresado como
/// fracción del TTL positivo. Usar 1/6 mantiene los falsos-negativos
/// fuera de la caché el tiempo suficiente para que la ingestión propague,
/// sin abrir una ventana de stampede demasiado grande.
const NEGATIVE_TTL_DIVISOR: u64 = 6;

/// Capa de caché Redis para el endpoint `/v1/check`.
///
/// `ConnectionManager` es barato de clonar — internamente comparte un Arc
/// sobre la conexión multiplexada. Se puede distribuir libremente entre tareas.
#[derive(Clone)]
pub struct RedisIocCache {
    conn: ConnectionManager,
}

impl RedisIocCache {
    /// Construye una instancia reutilizando un `ConnectionManager` ya abierto.
    pub fn new(conn: ConnectionManager) -> Self {
        Self { conn }
    }

    /// Intenta obtener un IoC cacheado por su valor normalizado.
    ///
    /// # Retorno
    ///
    /// - `None`           → cache miss; el caller debe consultar Postgres.
    /// - `Some(None)`     → negative cache hit; el valor fue buscado antes y no existe.
    /// - `Some(Some(ioc))` → cache hit; se puede devolver directamente al cliente.
    pub async fn get(&self, value: &str) -> Option<Option<Ioc>> {
        let key = cache_key(value);
        let mut conn = self.conn.clone();

        let raw: Option<String> = conn.get(&key).await.unwrap_or_else(|e| {
            // Un fallo de Redis nunca debe romper el flujo principal —
            // simplemente tratamos el error como un miss y dejamos que
            // Postgres responda. El error se loguea en el caller.
            tracing::warn!(error = %e, key = %key, "redis get falló; tratando como miss");
            None
        });

        let json = raw?;

        // El payload es un `Option<Ioc>` serializado como JSON.
        // `null` → negative cache, `{...}` → Ioc real.
        match serde_json::from_str::<Option<Ioc>>(&json) {
            Ok(ioc) => Some(ioc),
            Err(e) => {
                tracing::warn!(error = %e, key = %key, "fallo al deserializar la entrada de caché; ignorando");
                None // tratamos como miss si el JSON está corrupto
            }
        }
    }

    /// Escribe un resultado en la caché con SETEX.
    ///
    /// - `ioc = Some(ioc)` → guarda el IoC con `ttl_secs`.
    /// - `ioc = None`      → guarda un negative entry con `ttl_secs / 6`.
    ///
    /// Los errores de Redis se loguean pero no se propagan — la caché es
    /// best-effort; un fallo aquí no debe romper la respuesta al cliente.
    pub async fn set(&self, value: &str, ioc: &Option<Ioc>, ttl_secs: u64) {
        let key = cache_key(value);
        let effective_ttl = if ioc.is_some() {
            ttl_secs
        } else {
            (ttl_secs / NEGATIVE_TTL_DIVISOR).max(1)
        };

        let json = match serde_json::to_string(ioc) {
            Ok(j) => j,
            Err(e) => {
                tracing::warn!(error = %e, key = %key, "fallo al serializar Ioc para caché; skip");
                return;
            }
        };

        let mut conn = self.conn.clone();
        if let Err(e) = conn
            .set_ex::<_, _, ()>(&key, json, effective_ttl)
            .await
        {
            tracing::warn!(error = %e, key = %key, ttl = effective_ttl, "redis set_ex falló");
        }
    }
}

/// Construye la clave Redis para un valor normalizado de IoC.
#[inline]
fn cache_key(value: &str) -> String {
    format!("{KEY_PREFIX}{value}")
}
