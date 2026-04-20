//! `suggest_auth` — recommend auth configuration for a known or described API.

use serde_json::{Value, json};

struct Pattern {
    keywords: &'static [&'static str],
    auth_type: &'static str,
    token_name: &'static str,
    api_key_header: Option<&'static str>,
    default_headers: Option<fn() -> Value>,
    rationale: &'static str,
}

const PATTERNS: &[Pattern] = &[
    Pattern {
        keywords: &["github"],
        auth_type: "bearer",
        token_name: "GITHUB_TOKEN",
        api_key_header: None,
        default_headers: Some(|| json!({"Accept": "application/vnd.github+json"})),
        rationale: "GitHub REST v3 uses Personal Access Tokens via Bearer auth. Accept header for version pinning.",
    },
    Pattern {
        keywords: &["openai", "anthropic", "cohere"],
        auth_type: "bearer",
        token_name: "LLM_API_TOKEN",
        api_key_header: None,
        default_headers: None,
        rationale: "Major LLM APIs use Bearer token auth.",
    },
    Pattern {
        keywords: &["airtable"],
        auth_type: "bearer",
        token_name: "AIRTABLE_TOKEN",
        api_key_header: None,
        default_headers: None,
        rationale: "Airtable uses Bearer Personal Access Tokens.",
    },
    Pattern {
        keywords: &["slack", "discord"],
        auth_type: "bearer",
        token_name: "WEBHOOK_TOKEN",
        api_key_header: None,
        default_headers: None,
        rationale: "Chat platform REST APIs use Bearer tokens.",
    },
];

pub fn suggest_auth(args: &Value) -> Result<String, String> {
    let desc = args
        .get("api_description")
        .and_then(Value::as_str)
        .ok_or("missing required field: api_description")?;
    let lower = desc.to_lowercase();
    for pat in PATTERNS {
        if pat.keywords.iter().any(|k| lower.contains(k)) {
            let out = json!({
                "auth_type": pat.auth_type,
                "auth_token": format!("secret:{}", pat.token_name),
                "api_key_header": pat.api_key_header,
                "default_headers": pat.default_headers.map(|f| f()).unwrap_or(Value::Null),
                "rationale": pat.rationale,
                "confidence": "high"
            });
            return serde_json::to_string(&out).map_err(|e| e.to_string());
        }
    }
    let out = json!({
        "auth_type": "unknown",
        "auth_token": null,
        "api_key_header": null,
        "default_headers": null,
        "rationale": "API not in known-patterns list; ask user which auth type to use.",
        "confidence": "low"
    });
    serde_json::to_string(&out).map_err(|e| e.to_string())
}

#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
mod tests {
    use super::*;

    #[test]
    fn github_api_suggests_bearer_pat() {
        let args = json!({ "api_description": "GitHub REST API v3" });
        let out = suggest_auth(&args).expect("ok");
        let j: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(j["auth_type"], "bearer");
        assert_eq!(j["confidence"], "high");
        assert_eq!(
            j["default_headers"]["Accept"],
            "application/vnd.github+json"
        );
    }

    #[test]
    fn openai_suggests_bearer() {
        let args = json!({ "api_description": "OpenAI API chat completions" });
        let out = suggest_auth(&args).expect("ok");
        let j: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(j["auth_type"], "bearer");
        assert_eq!(j["confidence"], "high");
    }

    #[test]
    fn unknown_api_returns_low_confidence() {
        let args = json!({ "api_description": "internal custom ERP system" });
        let out = suggest_auth(&args).expect("ok");
        let j: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(j["auth_type"], "unknown");
        assert_eq!(j["confidence"], "low");
    }
}
