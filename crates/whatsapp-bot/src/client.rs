use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub struct CheckResponse {
    pub value: String,
    pub is_known_threat: bool,
    pub trust_value: Option<f32>,
    pub impersonates: Option<String>,
}

/// Consulta la API interna (/v1/check) para verificar un IoC.
pub async fn check_ioc(ioc: &str) -> anyhow::Result<CheckResponse> {
    let api_url = std::env::var("INTERNAL_API_URL").unwrap_or_else(|_| "http://localhost:8080".to_string());
    let api_key = std::env::var("INTERNAL_API_KEY").unwrap_or_default();
    
    let url = format!("{}/v1/check", api_url);
    let client = reqwest::Client::new();
    let res = client.get(&url)
        .query(&[("value", ioc)])
        .header("X-Api-Key", api_key)
        .send()
        .await?;
        
    if !res.status().is_success() {
        let status = res.status();
        let text = res.text().await.unwrap_or_default();
        anyhow::bail!("Error de API interna ({}): {}", status, text);
    }
        
    let response: CheckResponse = res.json().await?;
    Ok(response)
}

/// Formatea el resultado de la verificación en un mensaje amigable para WhatsApp.
pub fn format_response(check: &CheckResponse) -> String {
    if check.is_known_threat {
        if let Some(impersonates) = &check.impersonates {
            format!("🚨 *Peligro*: El dato '{}' ha sido identificado como malicioso. Suplanta a: {}.", check.value, impersonates)
        } else {
            format!("🚨 *Peligro*: El dato '{}' ha sido identificado como malicioso. ¡No interactúes con él!", check.value)
        }
    } else if let Some(score) = check.trust_value {
        if score > 0.4 {
            format!("⚠️ *Precaución*: El dato '{}' es sospechoso, pero aún no se confirma como amenaza (score: {:.2}).", check.value, score)
        } else {
            format!("✅ *Seguro*: El dato '{}' no parece ser una amenaza conocida por ahora.", check.value)
        }
    } else {
         format!("✅ *Seguro*: El dato '{}' no parece ser una amenaza conocida por ahora.", check.value)
    }
}

/// Envía un mensaje de texto a través de la API de WhatsApp de Meta.
pub async fn send_whatsapp_message(to: &str, message: &str) -> anyhow::Result<()> {
    let token = std::env::var("WHATSAPP_API_TOKEN").unwrap_or_default();
    let phone_number_id = std::env::var("WHATSAPP_PHONE_NUMBER_ID").unwrap_or_default();
    
    if token.is_empty() || phone_number_id.is_empty() {
        tracing::warn!("Faltan credenciales de WhatsApp, no se enviará mensaje a {}", to);
        return Ok(());
    }
    
    let url = format!("https://graph.facebook.com/v17.0/{}/messages", phone_number_id);
    
    let client = reqwest::Client::new();
    
    let payload = serde_json::json!({
        "messaging_product": "whatsapp",
        "to": to,
        "type": "text",
        "text": {
            "body": message
        }
    });
    
    let res = client.post(&url)
        .bearer_auth(token)
        .json(&payload)
        .send()
        .await?;
        
    if !res.status().is_success() {
        let text = res.text().await?;
        anyhow::bail!("Error enviando mensaje WhatsApp: {}", text);
    }
    
    Ok(())
}
