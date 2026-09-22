//! `suggest_verification` — the trigger contract v1 `verify` (and `challenge`)
//! block for a known sender, so an operator does not have to know which header
//! GitHub signs with or that Meta needs a subscription handshake.
//!
//! Every secret reference is a NAME under the provider (`threads/app_secret`);
//! the operator stores the actual value in Setup, where it is staged for the
//! runtime.

use serde_json::{Value, json};

pub fn suggest_verification(args: &Value) -> Result<String, String> {
    let provider = args
        .get("provider")
        .and_then(Value::as_str)
        .map(|p| p.trim().to_ascii_lowercase())
        .filter(|p| !p.is_empty())
        .ok_or("missing required field: provider")?;

    let preset = match provider.as_str() {
        "threads" | "instagram" | "facebook" | "whatsapp" | "meta" => {
            let ns = if provider == "meta" {
                "meta"
            } else {
                provider.as_str()
            };
            json!({
                "verify": {
                    "scheme": "hmac-sha256",
                    "header": "X-Hub-Signature-256",
                    "prefix": "sha256=",
                    "encoding": "hex",
                    "secret_ref": format!("{ns}/app_secret"),
                },
                "challenge": {
                    "scheme": "meta_hub",
                    "verify_token_ref": format!("{ns}/verify_token"),
                },
                "methods": ["POST"],
                "notes": "Meta signs the raw body with the app secret and verifies the \
                          subscription with a GET handshake; register the trigger's URL and \
                          the same verify token in the Meta app dashboard. Meta sends no \
                          delivery id, so deduplicate per event in the flow.",
            })
        }
        "github" => json!({
            "verify": {
                "scheme": "hmac-sha256",
                "header": "X-Hub-Signature-256",
                "prefix": "sha256=",
                "encoding": "hex",
                "secret_ref": "github/webhook_secret",
            },
            "idempotency": { "key": "header:X-GitHub-Delivery" },
            "methods": ["POST"],
            "notes": "GitHub sends a unique X-GitHub-Delivery per delivery, used here for \
                      deduplication of redeliveries.",
        }),
        "slack" | "stripe" => {
            return Err(format!(
                "{provider} signs a timestamp together with the body; trigger contract v1 has \
                 no timestamped scheme yet. Use the {provider} channel/provider integration \
                 instead of a generic webhook trigger."
            ));
        }
        _ => json!({
            "verify": {
                "scheme": "bearer",
                "header": "Authorization",
                "secret_ref": format!("{}/ingest_key", sanitize(&provider)),
            },
            "methods": ["POST"],
            "notes": "Unknown sender: a shared bearer token is the simplest verification the \
                      sender can be configured to send. Prefer an HMAC scheme if it signs.",
        }),
    };
    Ok(preset.to_string())
}

/// A provider name usable as the first segment of a secret reference.
fn sanitize(provider: &str) -> String {
    let s: String = provider
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if s.chars().next().is_some_and(|c| c.is_ascii_alphanumeric()) {
        s
    } else {
        format!("webhook{s}")
    }
}

#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
mod tests {
    use super::*;
    use crate::tools::validate::{is_valid_secret_ref, validate_webhook_config};

    fn preset(provider: &str) -> Value {
        serde_json::from_str(&suggest_verification(&json!({"provider": provider})).unwrap())
            .unwrap()
    }

    #[test]
    fn the_threads_preset_verifies_the_signature_and_answers_the_handshake() {
        let p = preset("Threads");
        assert_eq!(p["verify"]["header"], "X-Hub-Signature-256");
        assert_eq!(p["verify"]["secret_ref"], "threads/app_secret");
        assert_eq!(p["challenge"]["verify_token_ref"], "threads/verify_token");
    }

    #[test]
    fn every_preset_passes_the_validator() {
        for provider in [
            "threads",
            "instagram",
            "meta",
            "github",
            "acme-crm",
            "my service",
        ] {
            let p = preset(provider);
            let mut config = serde_json::Map::new();
            for key in ["verify", "challenge", "idempotency", "methods"] {
                if let Some(v) = p.get(key) {
                    config.insert(key.into(), v.clone());
                }
            }
            let (valid, diags) = validate_webhook_config(&json!({"config": config}));
            assert!(valid, "{provider}: {diags:?}");
            assert!(is_valid_secret_ref(
                p["verify"]["secret_ref"].as_str().unwrap()
            ));
        }
    }

    #[test]
    fn timestamped_senders_are_refused_with_the_reason() {
        let err = suggest_verification(&json!({"provider": "stripe"})).unwrap_err();
        assert!(err.contains("timestamp"));
    }

    #[test]
    fn a_provider_is_required() {
        assert!(suggest_verification(&json!({})).is_err());
    }
}
