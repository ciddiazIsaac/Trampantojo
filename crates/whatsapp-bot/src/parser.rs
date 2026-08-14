use std::sync::OnceLock;
use regex::Regex;

/// Tipos de indicadores de compromiso (IoCs) extraídos de un mensaje.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtractedItem {
    Url(String),
    Ip(String),
    Hash(String),
}

static URL_REGEX: OnceLock<Regex> = OnceLock::new();
static IP_REGEX: OnceLock<Regex> = OnceLock::new();
static HASH_REGEX: OnceLock<Regex> = OnceLock::new();

fn url_regex() -> &'static Regex {
    URL_REGEX.get_or_init(|| {
        // Regex simplificada para extraer URLs (http, https, ftp, etc. o dominio)
        Regex::new(r"(?i)\b(?:https?://|www\.)[^\s()<>]+(?:\([\w\d]+\)|([^[:punct:]\s]|/))").unwrap()
    })
}

fn ip_regex() -> &'static Regex {
    IP_REGEX.get_or_init(|| {
        // Regex para IPv4 (ignora validación estricta, pero funciona para contexto de seguridad)
        Regex::new(r"\b(?:(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\.){3}(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\b").unwrap()
    })
}

fn hash_regex() -> &'static Regex {
    HASH_REGEX.get_or_init(|| {
        // MD5 (32 hex), SHA1 (40 hex), SHA256 (64 hex)
        Regex::new(r"\b([a-fA-F0-9]{32}|[a-fA-F0-9]{40}|[a-fA-F0-9]{64})\b").unwrap()
    })
}

/// Extrae URLs, IPs y Hashes de un texto usando expresiones regulares.
pub fn parse_message(text: &str) -> Vec<ExtractedItem> {
    let mut items = Vec::new();

    for mat in url_regex().find_iter(text) {
        items.push(ExtractedItem::Url(mat.as_str().to_string()));
    }

    for mat in ip_regex().find_iter(text) {
        items.push(ExtractedItem::Ip(mat.as_str().to_string()));
    }

    for mat in hash_regex().find_iter(text) {
        items.push(ExtractedItem::Hash(mat.as_str().to_string()));
    }

    // Filtrar posibles solapamientos si es necesario en un futuro.
    // Por ahora, se recogen todos.

    items
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_message() {
        let text = "Sospechoso: http://phishing-site.com/login y la IP es 192.168.1.1. El archivo tiene hash d41d8cd98f00b204e9800998ecf8427e.";
        let items = parse_message(text);
        
        assert_eq!(items.len(), 3);
        assert!(items.contains(&ExtractedItem::Url("http://phishing-site.com/login".to_string())));
        assert!(items.contains(&ExtractedItem::Ip("192.168.1.1".to_string())));
        assert!(items.contains(&ExtractedItem::Hash("d41d8cd98f00b204e9800998ecf8427e".to_string())));
    }
}
