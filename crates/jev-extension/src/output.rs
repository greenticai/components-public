//! Pure response-mapping for the Jev `decide` tool. No WIT imports.
//!
//! The answers are passed through as the API sent them, because a worker that
//! routes on `confidence` or `probabilities` must see exactly those numbers.
//! The mapper only insists that the answers object exists, so a malformed or
//! empty reply is an error and never reads as "the model decided nothing".

use serde::Deserialize;
use serde_json::{Map, Value, json};

#[derive(Debug, Deserialize)]
struct SystemOneResponse {
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    answers: Option<Map<String, Value>>,
    #[serde(default)]
    usage: Option<Value>,
    // Permissive error envelope: the docs do not publish one.
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    error: Option<Value>,
}

/// Parse a `POST /v1/systemone` reply into `{ model, answers, usage? }`.
pub fn map_response(body: &[u8]) -> Result<Value, String> {
    let parsed: SystemOneResponse =
        serde_json::from_slice(body).map_err(|error| format!("decode jev response: {error}"))?;
    match parsed.answers {
        Some(answers) if !answers.is_empty() => {
            let mut out = Map::new();
            out.insert("model".into(), json!(parsed.model));
            out.insert("answers".into(), Value::Object(answers));
            if let Some(usage) = parsed.usage {
                out.insert("usage".into(), usage);
            }
            Ok(Value::Object(out))
        }
        _ => Err(parsed
            .message
            .or_else(|| parsed.error.map(|e| e.to_string()))
            .unwrap_or_else(|| "jev returned no answers".to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passes_answers_through_untouched() {
        let raw = br#"{"model":"jev-1.13.0","answers":{
            "intent":{"type":"choice","choice":"refund","confidence":0.91,"probabilities":{"refund":0.91,"other":0.09}},
            "angry":{"type":"noul","noul":0.2}},"usage":{"input_tokens":4,"output_tokens":2}}"#;
        let out = map_response(raw).unwrap();
        assert_eq!(out["model"], "jev-1.13.0");
        assert_eq!(out["answers"]["intent"]["confidence"], 0.91);
        assert_eq!(out["answers"]["angry"]["noul"], 0.2);
        assert_eq!(out["usage"]["input_tokens"], 4);
    }

    #[test]
    fn a_reply_without_answers_is_an_error() {
        assert_eq!(
            map_response(br#"{"message":"bad state"}"#).unwrap_err(),
            "bad state"
        );
        assert!(map_response(br#"{"answers":{}}"#).is_err());
        assert!(map_response(br"{}").is_err());
    }

    #[test]
    fn malformed_json_is_an_error() {
        assert!(map_response(b"{bad").is_err());
    }
}
