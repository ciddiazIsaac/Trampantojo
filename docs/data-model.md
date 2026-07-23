# Modelo de Datos y Decisiones Arquitectónicas

## 1. Separación del TrustScore (Postgres vs ClickHouse)
El modelo `TrustScore` se encuentra separado intencionalmente en dos sistemas físicos:
- **Postgres (Estado Operativo):** Almacena únicamente el `trust_value`, es decir, el valor final necesario para tomar decisiones rápidas en la API (un `REAL` entre 0.0 y 1.0).
- **ClickHouse (Historial Analítico):** Guarda los `factors` o razones que llevaron al cálculo de dicho _score_. 

**Justificación:** Insertar un payload JSONB completo (`Vec<ScoreFactor>`) en cada fila de Postgres añadiría un overhead innecesario en la ruta crítica, que solo demanda el número frío para bloquear o aceptar peticiones. El "porqué" del score es una consulta asíncrona y analítica, y por ende, tiene su hogar correcto en ClickHouse.

## 2. Aplanado del Enum `Source`
En la tabla `iocs`, la estructura del trait Rust `Source` (que diferencia entre `Official` y `Community`) se encuentra aplanada o desnormalizada a nivel base de datos utilizando columnas anulables (`source_issuer`, `source_advisory_url`, `corroborations`) condicionadas por `source_kind`.

**Justificación:** 
Para la escala inicial, forzar un _join_ con dos tablas satélites independientes no aporta una ventaja medible en desempeño ni escalabilidad y solo introduce latencia. Aplanar la estructura es la decisión pragmática.

**Evolución a Futuro:**
Si a mediano/largo plazo la fuente comunitaria ('community') demanda recolectar un extenso set de campos accesorios (ej. reputación histórica del contribuyente, score del nodo que reporta, telemetría o IPs), el costo del aplanado aumentará, y se deberá aplicar una normalización desprendiendo la info del Source a tablas secundarias con relación `1:1`.
