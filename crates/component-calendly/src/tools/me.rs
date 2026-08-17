//! `calendly_me` tool domain — pure HTTP-call building and response
//! normalization for the current-user lookup. No WIT imports — this module
//! is fully host-testable; the actual `extension-host/http` invocation and
//! `describe()` tool metadata live in `lib.rs` / `tool_meta.rs`.
//!
//! Follows the `component-jira-ext::tools::issues` template: `MeOp` (input
//! enum) -> `build_call` (pure request builder) -> `normalize` (pure
//! response mapper), with no WIT/host types crossing the boundary. Unlike
//! the other Calendly domains, `calendly_me` has a single operation (`get`)
//! — the enum shape is kept for consistency with the other tools' dispatch
//! and `parse_operation` wiring in `lib.rs`.

// Copied verbatim from the design extension. The only edit is this attribute:
// the tool-metadata tables and several op enums exist for the TOOL surface and
// are unused by the node surface. Silencing it here keeps the rest of the file
// diffable against its source.
#![allow(dead_code)]
use serde::Deserialize;
use serde_json::{Map, Value};

use crate::client::{HttpCall, Method};

/// Calendly current-user operation selected by the `operation` input field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeOp {
    Get,
}

/// Raw `calendly_me` tool input, deserialized from the model-supplied
/// `args_json`.
#[derive(Debug, Deserialize)]
struct MeInput {
    operation: MeOp,
}

/// Build the Calendly REST v2 [`HttpCall`] for a `calendly_me` invocation.
///
/// Parses `args_json` into a [`MeInput`] and returns the resulting request.
/// On missing/malformed input, returns `Err` describing the problem.
pub fn build_call(args_json: &str) -> Result<HttpCall, String> {
    let input: MeInput =
        serde_json::from_str(args_json).map_err(|err| format!("invalid input: {err}"))?;
    match input.operation {
        MeOp::Get => Ok(HttpCall {
            method: Method::Get,
            path: "/users/me".to_string(),
            query: Vec::new(),
            body: None,
        }),
    }
}

/// Extract just the `operation` field from `args_json`, without validating
/// the other fields `build_call` requires. `lib.rs` calls this after
/// `build_call` succeeds so it knows which [`normalize`] arm to run.
pub fn parse_operation(args_json: &str) -> Result<MeOp, String> {
    #[derive(Deserialize)]
    struct OperationOnly {
        operation: MeOp,
    }
    serde_json::from_str::<OperationOnly>(args_json)
        .map(|parsed| parsed.operation)
        .map_err(|err| format!("invalid input: {err}"))
}

/// Map a raw Calendly `GET /users/me` response body to
/// `{uri,name,email,current_organization,scheduling_url}`, based on the
/// [`MeOp`] that produced it.
pub fn normalize(op: MeOp, raw: &[u8]) -> Result<Value, String> {
    match op {
        MeOp::Get => normalize_get(raw),
    }
}

/// Normalize the `GET /users/me` response, unwrapping Calendly's
/// `{"resource": {...}}` envelope, without panicking on missing fields.
fn normalize_get(raw: &[u8]) -> Result<Value, String> {
    let value: Value =
        serde_json::from_slice(raw).map_err(|err| format!("invalid user response: {err}"))?;
    let resource = value.get("resource").unwrap_or(&value);

    let mut out = Map::new();
    out.insert(
        "uri".to_string(),
        resource.get("uri").cloned().unwrap_or(Value::Null),
    );
    out.insert(
        "name".to_string(),
        resource.get("name").cloned().unwrap_or(Value::Null),
    );
    out.insert(
        "email".to_string(),
        resource.get("email").cloned().unwrap_or(Value::Null),
    );
    out.insert(
        "current_organization".to_string(),
        resource
            .get("current_organization")
            .cloned()
            .unwrap_or(Value::Null),
    );
    out.insert(
        "scheduling_url".to_string(),
        resource
            .get("scheduling_url")
            .cloned()
            .unwrap_or(Value::Null),
    );
    Ok(Value::Object(out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn get_builds_get_with_no_query_or_body() {
        let call = build_call(r#"{"operation":"get"}"#).unwrap();
        assert!(matches!(call.method, Method::Get));
        assert_eq!(call.path, "/users/me");
        assert!(call.query.is_empty());
        assert!(call.body.is_none());
    }

    #[test]
    fn build_call_rejects_malformed_input() {
        assert!(build_call("{not json").is_err());
        assert!(build_call(r#"{"operation":"nope"}"#).is_err());
    }

    #[test]
    fn normalize_get_unwraps_resource_envelope() {
        let raw = json!({
            "resource": {
                "uri": "https://api.calendly.com/users/AAAA",
                "name": "Jane Doe",
                "email": "jane@example.com",
                "current_organization": "https://api.calendly.com/organizations/BBBB",
                "scheduling_url": "https://calendly.com/jane"
            }
        })
        .to_string();
        let out = normalize(MeOp::Get, raw.as_bytes()).unwrap();
        assert_eq!(out["uri"], "https://api.calendly.com/users/AAAA");
        assert_eq!(out["name"], "Jane Doe");
        assert_eq!(out["email"], "jane@example.com");
        assert_eq!(
            out["current_organization"],
            "https://api.calendly.com/organizations/BBBB"
        );
        assert_eq!(out["scheduling_url"], "https://calendly.com/jane");
    }

    #[test]
    fn normalize_get_handles_missing_resource_fields_without_panicking() {
        let raw = json!({ "resource": {} }).to_string();
        let out = normalize(MeOp::Get, raw.as_bytes()).unwrap();
        assert_eq!(out["uri"], Value::Null);
        assert_eq!(out["name"], Value::Null);
        assert_eq!(out["email"], Value::Null);
        assert_eq!(out["current_organization"], Value::Null);
        assert_eq!(out["scheduling_url"], Value::Null);
    }

    #[test]
    fn normalize_get_rejects_invalid_json() {
        assert!(normalize(MeOp::Get, b"not json").is_err());
    }

    #[test]
    fn parse_operation_extracts_op_ignoring_other_fields() {
        assert_eq!(parse_operation(r#"{"operation":"get"}"#), Ok(MeOp::Get));
        assert!(parse_operation(r#"{"operation":"nope"}"#).is_err());
        assert!(parse_operation("{not json").is_err());
    }
}
