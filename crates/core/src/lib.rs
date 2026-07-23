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
