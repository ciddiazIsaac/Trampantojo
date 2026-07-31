use redis::{aio::ConnectionManager, Script};

pub struct RedisRateLimiter {
    redis_conn: ConnectionManager,
}

impl RedisRateLimiter {
    pub fn new(redis_conn: ConnectionManager) -> Self {
        Self { redis_conn }
    }

    /// Implementa un algoritmo Token Bucket atómico en Redis vía Lua.
    /// Retorna (permitido, tokens_restantes).
    pub async fn check_limit(
        &self,
        identifier: &str,
        capacity: u32,
        refill_rate_per_sec: u32,
    ) -> anyhow::Result<(bool, u32)> {
        let key = format!("ratelimit:{}", identifier);

        let script = Script::new(
            r#"
            local key = KEYS[1]
            local capacity = tonumber(ARGV[1])
            local refill_rate = tonumber(ARGV[2])
            local now = tonumber(ARGV[3])
            
            local bucket = redis.call("HMGET", key, "tokens", "last_refill")
            local tokens = tonumber(bucket[1])
            local last_refill = tonumber(bucket[2])
            
            if not tokens then
                tokens = capacity
                last_refill = now
            else
                local elapsed = math.max(0, now - last_refill)
                local refill = math.floor(elapsed * refill_rate)
                if refill > 0 then
                    tokens = math.min(capacity, tokens + refill)
                    last_refill = now
                end
            end
            
            local allowed = 0
            if tokens >= 1 then
                allowed = 1
                tokens = tokens - 1
            end
            
            redis.call("HMSET", key, "tokens", tokens, "last_refill", last_refill)
            -- Expirar la key si nadie la usa (capacity / refill_rate = segundos para llenar)
            redis.call("EXPIRE", key, math.ceil(capacity / refill_rate) + 10)
            
            return {allowed, tokens}
        "#,
        );

        let now_sec = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Necesitamos mutabilidad en el con manager porque invoke_async lo requiere
        let mut conn = self.redis_conn.clone();

        let result: (i64, i64) = script
            .key(key)
            .arg(capacity)
            .arg(refill_rate_per_sec)
            .arg(now_sec)
            .invoke_async(&mut conn)
            .await?;

        let allowed = result.0 == 1;
        let tokens_remaining = result.1 as u32;

        Ok((allowed, tokens_remaining))
    }
}
