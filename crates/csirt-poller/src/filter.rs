//! Funciones de filtrado puras (sin I/O) sobre AlertSchema.
//! Todas las decisiones de "¿procesamos esta alerta/este IoC?" viven aquí,
//! lo que las hace trivialmente testeables sin mocks de HTTP ni DB.

use trampantojo_core::IndicatorType;

/// Retorna true solo si la alerta es de distribución pública.
/// El CSIRT usa "TLP:CLEAR" (estándar TLP 2.0 en inglés).
/// Se acepta también "TLP:WHITE" por compatibilidad con alertas antiguas.
pub fn is_tlp_public(tlp: &str) -> bool {
    let t = tlp.trim().to_uppercase();
    t == "TLP:CLEAR" || t == "TLP:WHITE"
}

/// Retorna true si la alerta tiene el tag "phishing".
/// Se usa el array `tags` como criterio primario (es el campo diseñado para
/// categorización); `incident_type` se guarda como metadato pero no filtra.
pub fn is_phishing(tags: &[String]) -> bool {
    tags.iter().any(|t| t.eq_ignore_ascii_case("phishing"))
}

/// Mapea el `ioc_type` de la API al `IndicatorType` del dominio.
/// Retorna None para tipos que no son IoC accionables en nuestro contexto
/// (ej: técnicas MITRE ATT&CK, que son categorías, no indicadores de red).
pub fn map_ioc_type(ioc_type: &str) -> Option<IndicatorType> {
    match ioc_type.to_lowercase().as_str() {
        "url"    => Some(IndicatorType::Url),
        "domain" => Some(IndicatorType::Domain),
        "ipv4" | "ipv6" | "ip" => Some(IndicatorType::IpAddress),
        "md5" | "sha1" | "sha256" | "sha512" => Some(IndicatorType::FileHash),
        // Técnicas MITRE ATT&CK — valiosas para análisis pero no son IoC de red
        "mitre-attck" | "mitre-attack" => None,
        // Cualquier tipo desconocido se descarta con warn en el llamador
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Tests unitarios — sin fixtures, sin mocks
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- is_tlp_public ---

    #[test]
    fn tlp_clear_es_publico() {
        assert!(is_tlp_public("TLP:CLEAR"));
    }

    #[test]
    fn tlp_white_legacy_es_publico() {
        assert!(is_tlp_public("TLP:WHITE"));
    }

    #[test]
    fn tlp_amber_no_es_publico() {
        assert!(!is_tlp_public("TLP:AMBER"));
    }

    #[test]
    fn tlp_red_no_es_publico() {
        assert!(!is_tlp_public("TLP:RED"));
    }

    #[test]
    fn tlp_green_no_es_publico() {
        assert!(!is_tlp_public("TLP:GREEN"));
    }

    // --- is_phishing ---

    #[test]
    fn tag_phishing_detectado() {
        let tags = vec!["fraude".to_string(), "phishing".to_string()];
        assert!(is_phishing(&tags));
    }

    #[test]
    fn sin_tag_phishing() {
        let tags = vec!["vulnerabilidad".to_string(), "critica".to_string()];
        assert!(!is_phishing(&tags));
    }

    #[test]
    fn tag_phishing_case_insensitive() {
        let tags = vec!["Phishing".to_string()];
        assert!(is_phishing(&tags));
    }

    // --- map_ioc_type ---

    #[test]
    fn url_mapeado() {
        assert_eq!(map_ioc_type("url"), Some(IndicatorType::Url));
    }

    #[test]
    fn ipv4_mapeado() {
        assert_eq!(map_ioc_type("ipv4"), Some(IndicatorType::IpAddress));
    }

    #[test]
    fn sha256_mapeado_como_file_hash() {
        assert_eq!(map_ioc_type("sha256"), Some(IndicatorType::FileHash));
    }

    #[test]
    fn mitre_attck_descartado() {
        assert_eq!(map_ioc_type("mitre-attck"), None);
    }

    #[test]
    fn tipo_desconocido_descartado() {
        assert_eq!(map_ioc_type("certificado-ssl"), None);
    }
}
