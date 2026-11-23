//! Fournisseurs de modèles. Appel bloquant : à exécuter hors du thread d'interface.

use std::time::Duration;

use serde_json::{json, Value};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProviderKind {
    Anthropic,
    /// API « chat completions » : OpenAI, mais aussi les modèles locaux (Ollama, LM Studio…).
    OpenAiCompatible,
}

impl ProviderKind {
    pub const ALL: [ProviderKind; 2] = [ProviderKind::Anthropic, ProviderKind::OpenAiCompatible];

    pub fn code(self) -> &'static str {
        match self {
            Self::Anthropic => "anthropic",
            Self::OpenAiCompatible => "openai",
        }
    }
    pub fn parse(code: &str) -> Self {
        if code == "openai" {
            Self::OpenAiCompatible
        } else {
            Self::Anthropic
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Anthropic => "Anthropic (Claude)",
            Self::OpenAiCompatible => "OpenAI / modèle local (API compatible)",
        }
    }
    pub fn default_base_url(self) -> &'static str {
        match self {
            Self::Anthropic => "https://api.anthropic.com",
            Self::OpenAiCompatible => "https://api.openai.com/v1",
        }
    }
    pub fn default_model(self) -> &'static str {
        match self {
            Self::Anthropic => "claude-sonnet-5-5",
            Self::OpenAiCompatible => "gpt-4o",
        }
    }
    /// Variable d'environnement utilisée si aucune clé n'est enregistrée dans l'application.
    pub fn env_key(self) -> &'static str {
        match self {
            Self::Anthropic => "ANTHROPIC_API_KEY",
            Self::OpenAiCompatible => "OPENAI_API_KEY",
        }
    }
}

#[derive(Clone, Debug)]
pub struct ProviderConfig {
    pub kind: ProviderKind,
    pub base_url: String,
    pub model: String,
    pub api_key: String,
}

fn endpoint(cfg: &ProviderConfig) -> String {
    let base = cfg.base_url.trim().trim_end_matches('/');
    match cfg.kind {
        ProviderKind::Anthropic => format!("{base}/v1/messages"),
        ProviderKind::OpenAiCompatible => format!("{base}/chat/completions"),
    }
}

pub fn request_body(cfg: &ProviderConfig, system: &str, user: &str) -> Value {
    match cfg.kind {
        ProviderKind::Anthropic => json!({
            "model": cfg.model,
            "max_tokens": 4096,
            "system": system,
            "messages": [{ "role": "user", "content": user }],
        }),
        ProviderKind::OpenAiCompatible => json!({
            "model": cfg.model,
            "messages": [
                { "role": "system", "content": system },
                { "role": "user", "content": user },
            ],
        }),
    }
}

pub fn parse_response(kind: ProviderKind, v: &Value) -> Result<String, String> {
    let text = match kind {
        ProviderKind::Anthropic => v["content"]
            .as_array()
            .map(|blocks| {
                blocks
                    .iter()
                    .filter(|b| b["type"] == "text")
                    .filter_map(|b| b["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default(),
        ProviderKind::OpenAiCompatible => v["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
    };
    if text.trim().is_empty() {
        Err("Réponse vide du modèle.".to_string())
    } else {
        Ok(text)
    }
}

/// Message d'erreur lisible à partir du corps d'une réponse HTTP en erreur.
pub fn error_message(body: &str) -> String {
    let parsed: Option<Value> = serde_json::from_str(body).ok();
    let msg = parsed
        .as_ref()
        .and_then(|v| {
            v["error"]["message"]
                .as_str()
                .or_else(|| v["error"].as_str())
        })
        .map(str::to_string)
        .unwrap_or_else(|| body.to_string());
    msg.chars().take(300).collect()
}

pub fn complete(cfg: &ProviderConfig, system: &str, user: &str) -> Result<String, String> {
    if cfg.kind == ProviderKind::Anthropic && cfg.api_key.trim().is_empty() {
        return Err(format!(
            "Clé API manquante : renseigne-la dans la configuration ou dans la variable {}.",
            cfg.kind.env_key()
        ));
    }
    if cfg.model.trim().is_empty() {
        return Err("Nom du modèle manquant.".to_string());
    }
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(180))
        .build();
    let mut req = agent.post(&endpoint(cfg));
    match cfg.kind {
        ProviderKind::Anthropic => {
            req = req
                .set("x-api-key", cfg.api_key.trim())
                .set("anthropic-version", "2023-06-01");
        }
        ProviderKind::OpenAiCompatible => {
            if !cfg.api_key.trim().is_empty() {
                req = req.set("Authorization", &format!("Bearer {}", cfg.api_key.trim()));
            }
        }
    }
    match req.send_json(request_body(cfg, system, user)) {
        Ok(resp) => {
            let v: Value = resp
                .into_json()
                .map_err(|e| format!("Réponse illisible : {e}"))?;
            parse_response(cfg.kind, &v)
        }
        Err(ureq::Error::Status(code, resp)) => Err(format!(
            "Erreur {code} : {}",
            error_message(&resp.into_string().unwrap_or_default())
        )),
        Err(e) => Err(format!("Connexion impossible : {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(kind: ProviderKind) -> ProviderConfig {
        ProviderConfig {
            kind,
            base_url: "http://x/".into(),
            model: "m".into(),
            api_key: "k".into(),
        }
    }

    #[test]
    fn endpoints_and_bodies_match_each_api() {
        assert_eq!(
            endpoint(&cfg(ProviderKind::Anthropic)),
            "http://x/v1/messages"
        );
        assert_eq!(
            endpoint(&cfg(ProviderKind::OpenAiCompatible)),
            "http://x/chat/completions"
        );
        let a = request_body(&cfg(ProviderKind::Anthropic), "SYS", "USR");
        assert_eq!(
            (a["system"].as_str(), a["messages"][0]["content"].as_str()),
            (Some("SYS"), Some("USR"))
        );
        let o = request_body(&cfg(ProviderKind::OpenAiCompatible), "SYS", "USR");
        assert_eq!(
            (
                o["messages"][0]["role"].as_str(),
                o["messages"][1]["content"].as_str()
            ),
            (Some("system"), Some("USR"))
        );
    }

    #[test]
    fn responses_are_parsed() {
        let a = json!({"content":[{"type":"text","text":"Bonjour"},{"type":"text","text":"[#1]"}]});
        assert_eq!(
            parse_response(ProviderKind::Anthropic, &a).unwrap(),
            "Bonjour\n[#1]"
        );
        let o = json!({"choices":[{"message":{"content":"Salut"}}]});
        assert_eq!(
            parse_response(ProviderKind::OpenAiCompatible, &o).unwrap(),
            "Salut"
        );
        assert!(parse_response(ProviderKind::Anthropic, &json!({"content":[]})).is_err());
        assert!(parse_response(ProviderKind::OpenAiCompatible, &json!({})).is_err());
    }

    #[test]
    fn error_bodies_become_readable() {
        assert_eq!(
            error_message(r#"{"error":{"message":"clé invalide"}}"#),
            "clé invalide"
        );
        assert_eq!(error_message(r#"{"error":"nope"}"#), "nope");
        assert_eq!(error_message("pas du json"), "pas du json");
        assert_eq!(error_message(&"x".repeat(1000)).chars().count(), 300);
    }

    #[test]
    fn missing_key_or_model_is_reported_without_any_network_call() {
        let mut c = cfg(ProviderKind::Anthropic);
        c.api_key.clear();
        assert!(complete(&c, "s", "u")
            .unwrap_err()
            .contains("ANTHROPIC_API_KEY"));
        let mut c = cfg(ProviderKind::OpenAiCompatible);
        c.model.clear();
        assert!(complete(&c, "s", "u").is_err());
    }
}
