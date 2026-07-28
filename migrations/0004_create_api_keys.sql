-- Tabla para almacenar los hashes de las API Keys del sistema.
-- Guardamos el hash de la clave (ej: sha256) para que si la base de datos
-- se ve comprometida, no se expongan las claves en texto plano.
CREATE TABLE IF NOT EXISTS api_keys (
    key_hash VARCHAR PRIMARY KEY,
    org_id UUID NOT NULL,
    -- plan determina el rate limit asociado a esta clave ('free', 'premium', etc)
    plan VARCHAR NOT NULL,
    is_active BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_api_keys_org_id ON api_keys(org_id);
