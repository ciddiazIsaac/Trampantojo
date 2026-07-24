//! trampantojo-core
//!
//! Tipos de dominio compartidos entre todos los binarios del workspace
//! (ingestion, scoring, api, notifier, whatsapp-bot). Ningún binario debe
//! redefinir estos structs — si un campo cambia, cambia acá y todo el
//! workspace lo hereda al compilar.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ---------------------------------------------------------------------
// Indicator of Compromise
// ---------------------------------------------------------------------

/// El tipo de dato que representa el indicador. Cada variante normaliza
/// su propio formato (ej: un dominio siempre en minúsculas sin protocolo).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum IndicatorType {
    Domain,
    Url,
    IpAddress,
    PhoneNumber,
    FileHash,
}

/// Estado operativo del indicador. Un IoC no se borra cuando expira —
/// se marca, porque el historial es tan valioso como el estado actual
/// (ej: para el dashboard de tendencias en ClickHouse).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IocStatus {
    Active,
    Expired,
    Disputed,
}

/// Un indicador de compromiso normalizado, listo para ser scoreado
/// y consultado desde la API de verificación.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ioc {
    pub id: Uuid,
    pub indicator_type: IndicatorType,
    /// Valor ya normalizado (sin protocolo, sin mayúsculas, etc.)
    pub value: String,
    pub source: Source,
    pub trust_score: TrustScore,
    pub status: IocStatus,
    /// A qué institución suplanta esta campaña, si se sabe (ej: "Banco de Chile")
    pub impersonates: Option<String>,
    pub first_seen: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
}

// ---------------------------------------------------------------------
// Source — de dónde vino el indicador
// ---------------------------------------------------------------------

/// Un IoC puede venir de una fuente oficial (CSIRT/ANCI, con autoridad
/// inmediata) o de la comunidad (que necesita corroboración antes de
/// subir de confianza). El motor de scoring trata cada variante distinto.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Source {
    Official {
        issuer: String,
        advisory_url: Option<String>,
    },
    Community {
        /// Cuántos reportes independientes corroboran este mismo indicador
        corroborations: u32,
    },
}

// ---------------------------------------------------------------------
// TrustScore — el "PDP invertido": no decide en quién confiar,
// decide en qué indicador confiar.
// ---------------------------------------------------------------------

/// Un factor individual que contribuyó al score final. Guardar esto
/// (en vez de solo el número) es lo que te permite mostrar "por qué"
/// un indicador tiene 0.92 y no 0.4 — explicabilidad, no caja negra.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoreFactor {
    pub reason: String,
    pub weight: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustScore {
    /// 0.0 a 1.0. Por convención: >0.8 dispara notificación inmediata,
    /// 0.4-0.8 queda "en observación", <0.4 no llega a los consumidores.
    pub value: f32,
    pub factors: Vec<ScoreFactor>,
}

impl TrustScore {
    /// Umbral por encima del cual un indicador dispara notificación inmediata.
    /// Constante única para que `is_actionable`, `crossed_actionable_threshold`
    /// y cualquier futura regla compartan el mismo valor — un solo lugar para
    /// cambiarlo si los datos reales demuestran que 0.8 no es el corte correcto.
    pub const NOTIFY_THRESHOLD: f32 = 0.8;

    pub fn is_actionable(&self) -> bool {
        self.value > Self::NOTIFY_THRESHOLD
    }
}

// ---------------------------------------------------------------------
// MergeOutcome
// ---------------------------------------------------------------------

/// Resultado de una fusión de IoC bajo lock. Devuelve el indicador final
/// y el trust score que tenía ANTES del merge. Esto es crucial para
/// alimentar a ClickHouse sin condiciones de carrera (TOCTOU).
#[derive(Debug)]
pub struct MergeOutcome {
    pub ioc: Ioc,
    pub trust_before: Option<f32>,
    /// Indica si el estado realmente se alteró (true). Si es false, significa
    /// que el reporte fue descartado por deduplicación (ej: misma IP).
    pub was_merged: bool,
}

// ---------------------------------------------------------------------
// Repository traits — cada binario implementa el backend que necesita,
// pero todos hablan el mismo lenguaje de dominio.
// ---------------------------------------------------------------------

#[async_trait::async_trait]
pub trait IocRepository: Send + Sync {
    /// Estado actual — respaldado por Postgres, es lo que consulta la API.
    async fn upsert(&self, ioc: &Ioc, deduplication_id: Option<&str>) -> anyhow::Result<MergeOutcome>;
    async fn find_by_value(&self, value: &str) -> anyhow::Result<Option<Ioc>>;
}

#[async_trait::async_trait]
pub trait IocEventStore: Send + Sync {
    /// Log de eventos append-only — respaldado por ClickHouse, es lo que
    /// alimenta el dashboard de tendencias. Nunca se actualiza, solo se agrega.
    async fn record_scoring_event(&self, ioc: &Ioc, trust_before: Option<f32>) -> anyhow::Result<()>;
    async fn record_verification_query(&self, value: &str, matched: bool) -> anyhow::Result<()>;
}

// ---------------------------------------------------------------------
// Merge Logic — reglas de negocio de scoring
// ---------------------------------------------------------------------

impl Ioc {
    /// Fusiona el estado existente de un IoC con uno entrante del mismo
    /// (indicator_type, value). Reglas:
    ///
    /// 1. Si el existente ya es oficial, un reporte comunitario nuevo NO
    ///    lo degrada — solo se refresca `last_seen`.
    /// 2. Si el entrante es oficial, siempre pisa el estado anterior
    ///    (sea cual sea), porque una fuente oficial es autoridad inmediata.
    /// 3. Comunidad + comunidad: se acumulan corroboraciones y el score
    ///    sube con rendimientos decrecientes, con un tope deliberadamente
    ///    bajo el umbral de auto-notificación (0.8), para que la sola
    ///    acumulación comunitaria nunca dispare una alerta sin que un
    ///    humano o el CSIRT la revise — a menos que decidas subir el tope
    ///    más adelante con datos reales de cuántas corroboraciones
    ///    correlacionan con verdaderos positivos.
    pub fn merge(existing: Option<Ioc>, incoming: Ioc) -> Ioc {
        let Some(mut current) = existing else {
            return incoming;
        };

        match (&current.source, &incoming.source) {
            (Source::Official { .. }, _) => {
                current.last_seen = incoming.last_seen.max(current.last_seen);
                current
            }
            (_, Source::Official { .. }) => incoming,
            (Source::Community { corroborations }, Source::Community { .. }) => {
                let n = corroborations + 1;
                current.source = Source::Community { corroborations: n };
                current.trust_score = TrustScore {
                    value: Self::community_score(n),
                    factors: vec![ScoreFactor {
                        reason: format!("{n} reportes comunitarios corroborados"),
                        weight: 1.0,
                    }],
                };
                current.last_seen = incoming.last_seen.max(current.last_seen);
                current
            }
        }
    }

    /// Curva de saturación: cada corroboración adicional pesa menos que
    /// la anterior. Con n=1 → ~0.30, n=5 → ~0.66, n=15 → ~0.77, nunca
    /// cruza 0.78. Ajustable, pero el tope bajo 0.8 es una decisión
    /// deliberada, no un número al azar.
    pub fn community_score(n: u32) -> f32 {
        (1.0 - 0.7_f32.powi(n as i32)).min(0.78)
    }
}

/// Normaliza un valor entrante (convierte a minúsculas, quita espacios extra,
/// y elimina prefijos como http/https si es aplicable).
/// Esta función vive en el dominio porque es una regla de negocio que 
/// tanto la API como la capa de ingestión deben compartir.
pub fn normalize_ioc_value(value: &str) -> String {
    let mut val = value.trim().to_lowercase();
    if val.starts_with("http://") {
        val = val.replacen("http://", "", 1);
    } else if val.starts_with("https://") {
        val = val.replacen("https://", "", 1);
    }
    // Opcional: remover www.
    if val.starts_with("www.") {
        val = val.replacen("www.", "", 1);
    }
    // Remover el trailing slash si existe
    if val.ends_with('/') {
        val.pop();
    }
    val
}

// ---------------------------------------------------------------------
// Notificación — umbral y payload
// ---------------------------------------------------------------------

/// Retorna `true` solo cuando un indicador **acaba de cruzar** el umbral de
/// notificación por primera vez (edge-triggered, no level-triggered).
///
/// La distinción es crítica para no renotificar en cada corroboración adicional
/// sobre un indicador ya confirmado:
/// - `trust_before = None`    → indicador nuevo; si trust_after > umbral, notificar.
/// - `trust_before = Some(v)` → notificar solo si `v ≤ umbral` y `trust_after > umbral`.
/// - El empate exacto (`trust_before == 0.8`) cuenta como "abajo", porque el umbral
///   es estricto (`> 0.8`), así que 0.8 no activaba notificación antes.
///
/// Esta función es pura y testeada — misma filosofía que `Ioc::merge`.
pub fn crossed_actionable_threshold(trust_before: Option<f32>, trust_after: f32) -> bool {
    let was_below = trust_before.map_or(true, |v| v <= TrustScore::NOTIFY_THRESHOLD);
    trust_after > TrustScore::NOTIFY_THRESHOLD && was_below
}

/// Payload completo que se empuja a Redis Streams cuando un IoC cruza el umbral.
///
/// Lleva todo lo que el notifier necesita para actuar — sin que tenga que volver
/// a consultar Postgres. Desacoplamiento deliberado: un fallo en Postgres después
/// de este punto no bloquea la notificación (el mensaje ya está en la cola).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationEvent {
    /// Valor normalizado del indicador (ej: "banco-falso.cl").
    pub ioc_value: String,
    /// Tipo del indicador — para que el mensaje diga "dominio" / "URL" / "IP".
    pub indicator_type: IndicatorType,
    /// Entidad suplantada, si se conoce (ej: "Banco Santander").
    pub impersonates: Option<String>,
    /// Score final después del merge.
    pub trust_value: f32,
    /// Fuente — para que el mensaje diga "confirmado por CSIRT" vs
    /// "corroborado por N reportes comunitarios".
    pub source: Source,
}

/// Contrato que debe implementar cualquier cola de notificaciones.
/// Vive en `core` para que `ingestion` dependa de la abstracción,
/// no de la implementación concreta (Redis).
#[async_trait::async_trait]
pub trait NotificationQueue: Send + Sync {
    async fn enqueue(&self, event: &NotificationEvent) -> anyhow::Result<()>;
}

/// Convierte notación "defanged" de threat intel al valor real, para que los
/// IoC publicados por el CSIRT (y otras fuentes) puedan compararse y
/// almacenarse en formato canónico antes de pasar por `normalize_ioc_value`.
///
/// Convenciones soportadas:
/// - `[.]`  → `.`  (el más común — evita que parsers de correo/web resuelvan el dominio)
/// - `[:]`  → `:`  (usado en puertos y esquemas)
/// - `hxxp://`  → `http://`
/// - `hxxps://` → `https://`
///
/// El orden importa: primero se reemplazan esquemas enteros, luego caracteres
/// individuales, para no producir cadenas intermedias incoherentes.
pub fn refang(value: &str) -> String {
    let mut s = value.trim().to_string();
    // Esquemas defangeados (case-insensitive en la práctica, pero el CSIRT
    // los publica en minúsculas — aplicamos el reemplazo sobre la copia original).
    s = s.replace("hxxps://", "https://");
    s = s.replace("hxxp://",  "http://");
    // Caracteres individuales entre corchetes
    s = s.replace("[.]", ".");
    s = s.replace("[:]", ":");
    s
}


#[cfg(test)]
mod merge_tests {
    use super::*;
    use chrono::Utc;

    fn base_ioc(source: Source, trust_value: f32) -> Ioc {
        Ioc {
            id: Uuid::new_v4(),
            indicator_type: IndicatorType::Domain,
            value: "banco-de-chile-verificacion.cl".into(),
            source,
            trust_score: TrustScore { value: trust_value, factors: vec![] },
            status: IocStatus::Active,
            impersonates: Some("Banco de Chile".into()),
            first_seen: Utc::now(),
            last_seen: Utc::now(),
        }
    }

    #[test]
    fn official_confirmado_no_se_degrada_por_comunidad_posterior() {
        let existing = base_ioc(Source::Official { issuer: "CSIRT".into(), advisory_url: None }, 1.0);
        let incoming = base_ioc(Source::Community { corroborations: 0 }, 0.3);
        let merged = Ioc::merge(Some(existing), incoming);
        assert!(matches!(merged.source, Source::Official { .. }));
        assert_eq!(merged.trust_score.value, 1.0);
    }

    #[test]
    fn oficial_entrante_siempre_pisa_estado_previo() {
        let existing = base_ioc(Source::Community { corroborations: 3 }, 0.65);
        let incoming = base_ioc(Source::Official { issuer: "ANCI".into(), advisory_url: None }, 1.0);
        let merged = Ioc::merge(Some(existing), incoming);
        assert!(matches!(merged.source, Source::Official { .. }));
    }

    #[test]
    fn comunidad_acumula_corroboraciones_sin_cruzar_el_tope() {
        let existing = base_ioc(Source::Community { corroborations: 0 }, 0.3);
        let incoming = base_ioc(Source::Community { corroborations: 0 }, 0.3);
        let merged = Ioc::merge(Some(existing), incoming);
        if let Source::Community { corroborations } = merged.source {
            assert_eq!(corroborations, 1);
        } else {
            panic!("se esperaba Source::Community");
        }
        assert!(merged.trust_score.value < 0.8, "no debe cruzar el umbral de auto-notificación");
    }
}

#[cfg(test)]
mod refang_tests {
    use super::*;

    #[test]
    fn punto_entre_corchetes() {
        assert_eq!(refang("ejemplo[.]com"), "ejemplo.com");
    }

    #[test]
    fn multiples_puntos_defangeados() {
        assert_eq!(refang("sub[.]ejemplo[.]com"), "sub.ejemplo.com");
    }

    #[test]
    fn esquema_hxxps() {
        assert_eq!(refang("hxxps://ejemplo.com/login"), "https://ejemplo.com/login");
    }

    #[test]
    fn esquema_hxxp() {
        assert_eq!(refang("hxxp://ejemplo[.]com"), "http://ejemplo.com");
    }

    #[test]
    fn combinado_esquema_y_dominio() {
        // Caso típico del CSIRT: hxxps://dominio[.]com/path
        assert_eq!(
            refang("hxxps://banco-falso[.]cl/login"),
            "https://banco-falso.cl/login"
        );
    }

    #[test]
    fn dos_puntos_defangeados() {
        assert_eq!(refang("192[.]168[.]1[.]1[:]8080"), "192.168.1.1:8080");
    }

    #[test]
    fn valor_limpio_no_se_altera() {
        assert_eq!(refang("ejemplo.com"), "ejemplo.com");
        assert_eq!(refang("https://ejemplo.com"), "https://ejemplo.com");
    }

    #[test]
    fn trim_de_espacios() {
        assert_eq!(refang("  ejemplo[.]com  "), "ejemplo.com");
    }
}

#[cfg(test)]
mod threshold_tests {
    use super::*;

    // Caso 1: IoC nuevo del CSIRT — entra directo a 1.0, debe notificar.
    #[test]
    fn ioc_nuevo_oficial_notifica() {
        assert!(crossed_actionable_threshold(None, 1.0));
    }

    // Caso 2: corroboración comunitaria que cruza el umbral — debe notificar.
    #[test]
    fn comunidad_cruza_umbral_notifica() {
        assert!(crossed_actionable_threshold(Some(0.5), 0.85));
    }

    // Caso 3: ya estaba sobre el umbral — corroboración adicional NO notifica.
    #[test]
    fn ya_confirmado_no_renotifica() {
        assert!(!crossed_actionable_threshold(Some(0.82), 0.95));
    }

    // Caso 4: empate exacto en el umbral antes (0.8 ≤ 0.8 → was_below=true) — notifica.
    #[test]
    fn empate_exacto_en_umbral_cuenta_como_abajo() {
        assert!(crossed_actionable_threshold(Some(0.8), 0.81));
    }

    // Caso 5: indicador nuevo pero no llega al umbral — no notifica.
    #[test]
    fn ioc_nuevo_bajo_umbral_no_notifica() {
        assert!(!crossed_actionable_threshold(None, 0.7));
    }

    // Caso 6: sin cambio real sobre un indicador ya confirmado — no notifica.
    #[test]
    fn sin_cambio_ya_confirmado_no_notifica() {
        assert!(!crossed_actionable_threshold(Some(0.9), 0.9));
    }
}
