-- clickhouse/schema.sql
--
-- Dos tablas físicas para dos patrones de acceso distintos sobre los
-- mismos datos. No es "tener dos copias" — es una decisión de diseño:
-- forzar un único ORDER BY que sirva bien a consultas de detalle Y a
-- agregaciones del dashboard es el compromiso clásico que empeora todo.
-- ClickHouse hace que no tengas que elegir.

-- ==========================================================================
-- TABLA DE HECHOS
-- Patrón: "dame toda la historia de este indicador específico"
--         "cuántos merges tocaron este dominio esta semana"
--         "detección de spam de corroboraciones (mismo ioc_value, rafaga de inserts)"
--
-- Una fila por cada ejecución de Ioc::merge, sin importar si el
-- trust_value cambió. Dos reportes idénticos que no mueven la aguja
-- siguen siendo información — son exactamente la señal que necesitás
-- para detectar un ataque de corroboración artificial.
-- ==========================================================================

CREATE TABLE IF NOT EXISTS ioc_score_events
(
    -- UUID generado en Rust en el momento del merge, no en CH.
    -- Razón: si el insert falla y se reintenta, queremos idempotencia
    -- eventual (aunque CH no garantiza exactly-once, el UUID ayuda a
    -- detectar duplicados en un dedup posterior si fuera necesario).
    event_id             UUID,

    ioc_value            String,

    -- LowCardinality: cardinalidad baja (5 variantes de IndicatorType),
    -- repetida millones de veces. CH almacena un diccionario y referencias
    -- de 1-2 bytes — mejor compresión y GROUP BY más rápido, sin costo
    -- en escritura ni en queries.
    ioc_type             LowCardinality(String),

    -- Denormalizado a propósito. El dashboard NUNCA debe cruzar con
    -- Postgres para saber a quién suplanta este indicador.
    -- String vacío (no NULL) cuando no aplica — así LowCardinality
    -- funciona sin excepción de nulos.
    impersonates         LowCardinality(String) DEFAULT '',

    -- Snapshot de la fuente AL MOMENTO de este evento.
    -- source_kind tiene cardinalidad 2 (official / community).
    source_kind          LowCardinality(String),
    source_issuer        Nullable(String),       -- solo para official

    -- Snapshot del contador en este momento exacto, no el valor actual
    -- en Postgres. Es lo que necesitás para detectar una ráfaga: si ves
    -- 500 filas con el mismo ioc_value y corroborations_after subiendo
    -- de a 1 en segundos, eso es spam — no requiere join, está en la tabla.
    corroborations_after UInt32,

    -- Nullable porque en el primer merge no hay estado previo.
    trust_before         Nullable(Float32),
    trust_after          Float32,

    merged_at            DateTime64(3, 'UTC') DEFAULT now64()
)
ENGINE = MergeTree()
-- ORDER BY define el índice primario de CH. (ioc_value, merged_at) es
-- óptimo para "dame la historia completa de este indicador en orden
-- cronológico" — el caso de uso de auditoría y debug.
-- Es MEDIOCRE para "dame el conteo semanal por banco" (eso lo cubre la MV).
ORDER BY (ioc_value, merged_at)
PARTITION BY toYYYYMM(merged_at);

-- ==========================================================================
-- TABLA DE AGREGADOS (destino de la vista materializada)
-- Patrón: "cuántas campañas suplantaron a qué banco esta semana"
--         "tendencia por tipo de indicador en el último mes"
--
-- El dashboard consulta ESTA tabla, nunca la de eventos crudos.
-- Es órdenes de magnitud más pequeña y ya viene resumida.
-- ==========================================================================

CREATE TABLE IF NOT EXISTS ioc_events_daily
(
    day              Date,
    impersonates     LowCardinality(String),
    ioc_type         LowCardinality(String),

    -- SummingMergeTree suma estas columnas automáticamente en el background
    -- cuando encuentra filas con el mismo ORDER BY (day, impersonates, ioc_type).
    -- El dashboard siempre consulta con GROUP BY + sum() para ser correcto
    -- antes de que el merge de fondo haya corrido — ver nota en la MV.
    event_count      UInt64,
    actionable_count UInt64  -- filas donde trust_after > 0.8
)
ENGINE = SummingMergeTree()
ORDER BY (day, impersonates, ioc_type)
PARTITION BY toYYYYMM(day);

-- ==========================================================================
-- VISTA MATERIALIZADA
--
-- No es una vista que se recalcula al consultarla.
-- Es un trigger: corre esta SELECT sobre cada bloque de filas que llega
-- a ioc_score_events y escribe el resultado incremental en ioc_events_daily.
-- El dashboard nunca vuelve a hacer GROUP BY sobre millones de eventos —
-- ya viene hecho, fila a fila, en el momento de la escritura.
--
-- Nota de query correcta en el dashboard:
--   SELECT day, impersonates, ioc_type,
--          sum(event_count) AS events,
--          sum(actionable_count) AS actionable
--   FROM ioc_events_daily
--   WHERE day >= today() - 7
--   GROUP BY day, impersonates, ioc_type
--   ORDER BY day DESC
-- El GROUP BY + sum() es necesario porque SummingMergeTree puede tener
-- filas aún no mergeadas con la misma clave — la query debe sumarlas
-- explícitamente para ser correcta en cualquier estado del merge.
-- ==========================================================================

CREATE MATERIALIZED VIEW IF NOT EXISTS ioc_events_daily_mv
TO ioc_events_daily
AS
SELECT
    toDate(merged_at)          AS day,
    impersonates,
    ioc_type,
    count()                    AS event_count,
    countIf(trust_after > 0.8) AS actionable_count
FROM ioc_score_events
GROUP BY
    day,
    impersonates,
    ioc_type;
