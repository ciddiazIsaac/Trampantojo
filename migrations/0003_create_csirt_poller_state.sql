-- Estado del poller del CSIRT. Tabla singleton (siempre una sola fila, id=1).
--
-- last_polled_at: el cursor de tiempo que se pasa como from_date en cada ciclo.
--   Se inicializa 30 días atrás para capturar alertas recientes en el primer run.
-- seen_codes: array de códigos de alerta ya procesados (ej: 'ACF26-01128').
--   Sirve como segundo nivel de deduplicación: si el CSIRT actualiza una alerta
--   (latest_revision_created_at cambia pero code es el mismo), no la reingresamos.
--   Crece ~1 elemento/alerta real de phishing procesada — inocuo a esta escala.
CREATE TABLE IF NOT EXISTS csirt_poller_state (
    id              INTEGER PRIMARY KEY DEFAULT 1,
    last_polled_at  TIMESTAMPTZ NOT NULL DEFAULT (NOW() - INTERVAL '30 days'),
    seen_codes      TEXT[]      NOT NULL DEFAULT '{}'
);

-- Garantizamos la fila singleton en cada migración (idempotente).
INSERT INTO csirt_poller_state DEFAULT VALUES ON CONFLICT DO NOTHING;
