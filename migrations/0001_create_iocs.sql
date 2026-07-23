-- 0001_create_iocs.sql
-- Estado actual de los indicadores de compromiso. Esta tabla respalda
-- IocRepository — es lo que consulta la API en cada verificación.
-- El historial de eventos (scoring, queries) NO vive acá, vive en ClickHouse.

CREATE TYPE indicator_type AS ENUM ('domain', 'url', 'ip_address', 'phone_number', 'file_hash');
CREATE TYPE ioc_status AS ENUM ('active', 'expired', 'disputed');
CREATE TYPE source_kind AS ENUM ('official', 'community');

CREATE TABLE iocs (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    indicator_type      indicator_type NOT NULL,
    value               TEXT NOT NULL,          -- ya normalizado antes de insertar
    status              ioc_status NOT NULL DEFAULT 'active',
    impersonates        TEXT,                   -- ej: "Banco de Chile"

    -- Source (aplanado; el enum de Rust se reconstruye al leer)
    source_kind         source_kind NOT NULL,
    source_issuer        TEXT,                  -- solo si source_kind = 'official'
    source_advisory_url TEXT,                   -- solo si source_kind = 'official'
    corroborations      INT NOT NULL DEFAULT 0, -- solo relevante si source_kind = 'community'

    -- TrustScore (los factors detallados quedan en ClickHouse; acá solo el valor operativo)
    trust_value         REAL NOT NULL CHECK (trust_value >= 0.0 AND trust_value <= 1.0),

    first_seen          TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_seen           TIMESTAMPTZ NOT NULL DEFAULT now(),

    -- un mismo indicador (mismo tipo + valor) no debería duplicarse
    CONSTRAINT uq_iocs_type_value UNIQUE (indicator_type, value)
);

-- La ruta más caliente del sistema: "¿este valor está en la tabla?"
CREATE INDEX idx_iocs_value ON iocs (value);
CREATE INDEX idx_iocs_status ON iocs (status) WHERE status = 'active';
