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
    pub fn is_actionable(&self) -> bool {
        self.value > 0.8
    }
}

// ---------------------------------------------------------------------
// Repository traits — cada binario implementa el backend que necesita,
// pero todos hablan el mismo lenguaje de dominio.
// ---------------------------------------------------------------------

#[async_trait::async_trait]
pub trait IocRepository: Send + Sync {
    /// Estado actual — respaldado por Postgres, es lo que consulta la API.
    async fn upsert(&self, ioc: &Ioc) -> anyhow::Result<()>;
    async fn find_by_value(&self, value: &str) -> anyhow::Result<Option<Ioc>>;
}

#[async_trait::async_trait]
pub trait IocEventStore: Send + Sync {
    /// Log de eventos append-only — respaldado por ClickHouse, es lo que
    /// alimenta el dashboard de tendencias. Nunca se actualiza, solo se agrega.
    async fn record_scoring_event(&self, ioc: &Ioc) -> anyhow::Result<()>;
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
    fn community_score(n: u32) -> f32 {
        (1.0 - 0.7_f32.powi(n as i32)).min(0.78)
    }
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
