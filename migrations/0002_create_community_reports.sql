-- 0002_create_community_reports.sql
--
-- Tabla de deduplicación para reportes comunitarios.
-- Garantiza que una misma identidad (ej. hash de IP) no pueda inflar
-- el contador de corroboraciones votando múltiples veces por el mismo indicador.

CREATE TABLE community_reports (
    indicator_type      indicator_type NOT NULL,
    value               TEXT NOT NULL,
    reporter_hash       TEXT NOT NULL,
    reported_at         TIMESTAMPTZ NOT NULL DEFAULT now(),

    -- Llave primaria compuesta: un reportante solo puede reportar
    -- un indicador específico una única vez.
    PRIMARY KEY (indicator_type, value, reporter_hash),

    -- Opcional pero recomendado para integridad referencial,
    -- si se borra el IoC, se borran sus reportes.
    FOREIGN KEY (indicator_type, value) REFERENCES iocs (indicator_type, value) ON DELETE CASCADE
);
