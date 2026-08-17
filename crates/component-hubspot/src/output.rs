//! Pure response mapping: HubSpot CRM JSON → the stable shape the tools return
//! to the agent. Free of WIT/bindings imports so it unit-tests on the host.
//!
//! Single-record ops return [`OutRecord`]; search returns [`SearchOut`].
//! HubSpot's camelCase timestamps are renamed to snake_case on the way out and
//! an explicit `object` field is added so the agent always knows the record type.

// Copied verbatim from the design extension. The only edit is this attribute:
// several structs and tables exist for the TOOL surface and are unused by the
// node surface. Silencing it here keeps the rest of the file diffable against
// its source.
#![allow(dead_code)]
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::input::AssociateInput;

/// HubSpot's raw record shape (permissive: unknown fields ignored).
#[derive(Debug, Deserialize)]
struct HsRecord {
    #[serde(default)]
    id: String,
    #[serde(default)]
    properties: Map<String, Value>,
    #[serde(rename = "createdAt", default)]
    created_at: Option<String>,
    #[serde(rename = "updatedAt", default)]
    updated_at: Option<String>,
}

/// Normalized single-record output.
#[derive(Debug, Serialize, PartialEq)]
pub struct OutRecord {
    pub object: String,
    pub id: String,
    pub properties: Map<String, Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

impl OutRecord {
    fn from_hs(object: &str, record: HsRecord) -> Self {
        OutRecord {
            object: object.to_string(),
            id: record.id,
            properties: record.properties,
            created_at: record.created_at,
            updated_at: record.updated_at,
        }
    }
}

/// Normalized search output.
#[derive(Debug, Serialize, PartialEq)]
pub struct SearchOut {
    pub results: Vec<OutRecord>,
    pub total: u64,
}

#[derive(Debug, Deserialize)]
struct HsSearch {
    #[serde(default)]
    total: u64,
    #[serde(default)]
    results: Vec<HsRecord>,
}

/// Parse a single-object HubSpot response (create/update/get) into [`OutRecord`].
pub fn map_record(object: &str, body: &[u8]) -> Result<OutRecord, String> {
    let record: HsRecord = serde_json::from_slice(body)
        .map_err(|error| format!("decode hubspot record response: {error}"))?;
    Ok(OutRecord::from_hs(object, record))
}

/// Parse a HubSpot search response into [`SearchOut`].
pub fn map_search(object: &str, body: &[u8]) -> Result<SearchOut, String> {
    let search: HsSearch = serde_json::from_slice(body)
        .map_err(|error| format!("decode hubspot search response: {error}"))?;
    Ok(SearchOut {
        total: search.total,
        results: search
            .results
            .into_iter()
            .map(|record| OutRecord::from_hs(object, record))
            .collect(),
    })
}

/// Build the success payload for `hubspot_associate` (the v4 default-association
/// PUT returns 204/200 with no useful body, so we synthesize a confirmation).
#[must_use]
pub fn associate_result(input: &AssociateInput) -> Value {
    json!({
        "ok": true,
        "from": { "object": input.from_object, "id": input.from_id },
        "to": { "object": input.to_object, "id": input.to_id }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_single_record_with_timestamps() {
        let raw = br#"{
          "id": "501",
          "properties": { "email": "a@b.com", "firstname": "Ada" },
          "createdAt": "2026-06-27T00:00:00Z",
          "updatedAt": "2026-06-27T01:00:00Z",
          "archived": false
        }"#;
        let out = map_record("contacts", raw).unwrap();
        assert_eq!(out.object, "contacts");
        assert_eq!(out.id, "501");
        assert_eq!(out.properties["email"], "a@b.com");
        assert_eq!(out.created_at.as_deref(), Some("2026-06-27T00:00:00Z"));
        assert_eq!(out.updated_at.as_deref(), Some("2026-06-27T01:00:00Z"));
    }

    #[test]
    fn map_record_without_timestamps_skips_them_on_output() {
        let raw = br#"{"id":"1","properties":{}}"#;
        let out = map_record("deals", raw).unwrap();
        let json = serde_json::to_value(&out).unwrap();
        assert!(json.get("created_at").is_none());
        assert!(json.get("updated_at").is_none());
    }

    #[test]
    fn map_record_malformed_is_err() {
        assert!(map_record("contacts", b"{not json").is_err());
    }

    #[test]
    fn maps_search_results_and_total() {
        let raw = br#"{
          "total": 2,
          "results": [
            {"id":"1","properties":{"name":"Acme"}},
            {"id":"2","properties":{"name":"Globex"}}
          ],
          "paging": {"next": {"after": "2"}}
        }"#;
        let out = map_search("companies", raw).unwrap();
        assert_eq!(out.total, 2);
        assert_eq!(out.results.len(), 2);
        assert_eq!(out.results[0].object, "companies");
        assert_eq!(out.results[1].id, "2");
    }

    #[test]
    fn map_search_empty_defaults() {
        let out = map_search("tickets", br#"{"results":[]}"#).unwrap();
        assert_eq!(out.total, 0);
        assert!(out.results.is_empty());
    }

    #[test]
    fn associate_result_shape() {
        let input: AssociateInput = serde_json::from_str(
            r#"{"from_object":"contacts","from_id":"1","to_object":"companies","to_id":"2"}"#,
        )
        .unwrap();
        let out = associate_result(&input);
        assert_eq!(out["ok"], true);
        assert_eq!(out["from"]["object"], "contacts");
        assert_eq!(out["to"]["id"], "2");
    }
}
