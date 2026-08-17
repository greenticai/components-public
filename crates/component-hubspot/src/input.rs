//! Pure input parsing + validation for the HubSpot CRUD and associate tools.
//!
//! Free of any WIT/bindings/network imports so it unit-tests on the host via
//! `cargo test`. `lib.rs` deserializes the tool args into these structs and
//! calls [`validate_crud`] before handing off to [`crate::hubspot`].

// Copied verbatim from the design extension. The only edit is this attribute:
// several structs and tables exist for the TOOL surface and are unused by the
// node surface. Silencing it here keeps the rest of the file diffable against
// its source.
#![allow(dead_code)]
use serde::Deserialize;
use serde_json::{Map, Value};

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Operation {
    Create,
    Search,
    Update,
    Get,
}

/// Decoded input for the per-object CRUD tools (`hubspot_contacts`, etc.).
#[derive(Debug, Deserialize)]
pub struct CrudInput {
    pub operation: Operation,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub properties: Option<Map<String, Value>>,
    #[serde(default)]
    pub return_properties: Option<Vec<String>>,
    #[serde(default)]
    pub limit: Option<u32>,
}

/// Decoded input for `hubspot_pipelines`.
#[derive(Debug, Deserialize)]
pub struct PipelinesInput {
    pub object_type: String,
    #[serde(default)]
    pub pipeline_id: Option<String>,
}

/// Decoded input for `hubspot_owners`.
#[derive(Debug, Deserialize)]
pub struct OwnersInput {
    #[serde(default)]
    pub owner_id: Option<String>,
    #[serde(default)]
    pub limit: Option<u32>,
}

/// Decoded input for `hubspot_batch`.
#[derive(Debug, Deserialize)]
pub struct BatchInput {
    pub object_type: String,
    pub operation: String,
    pub inputs: serde_json::Value,
}

/// Decoded input for `hubspot_associate`.
#[derive(Debug, Deserialize)]
pub struct AssociateInput {
    pub from_object: String,
    pub from_id: String,
    pub to_object: String,
    pub to_id: String,
    #[serde(default)]
    pub association_type: Option<String>,
}

/// Enforce per-operation required fields. Returns `Ok(())` when the input is
/// usable for its declared operation.
pub fn validate_crud(input: &CrudInput) -> Result<(), String> {
    match input.operation {
        Operation::Create => require_properties(input),
        Operation::Update => {
            require_id(input)?;
            require_properties(input)
        }
        Operation::Get => require_id(input),
        Operation::Search => match &input.query {
            Some(query) if !query.trim().is_empty() => Ok(()),
            _ => Err("operation 'search' requires a non-empty 'query'".to_string()),
        },
    }
}

fn require_id(input: &CrudInput) -> Result<(), String> {
    match &input.id {
        Some(id) if !id.trim().is_empty() => Ok(()),
        _ => Err(format!(
            "operation '{:?}' requires a non-empty 'id'",
            input.operation
        )),
    }
}

fn require_properties(input: &CrudInput) -> Result<(), String> {
    match &input.properties {
        Some(map) if !map.is_empty() => Ok(()),
        _ => Err(format!(
            "operation '{:?}' requires a non-empty 'properties' object",
            input.operation
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crud(json: &str) -> CrudInput {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn parses_operation_lowercase() {
        assert_eq!(
            crud(r#"{"operation":"create"}"#).operation,
            Operation::Create
        );
        assert_eq!(
            crud(r#"{"operation":"search"}"#).operation,
            Operation::Search
        );
        assert_eq!(
            crud(r#"{"operation":"update"}"#).operation,
            Operation::Update
        );
        assert_eq!(crud(r#"{"operation":"get"}"#).operation, Operation::Get);
    }

    #[test]
    fn create_requires_properties() {
        assert!(validate_crud(&crud(r#"{"operation":"create"}"#)).is_err());
        assert!(
            validate_crud(&crud(
                r#"{"operation":"create","properties":{"email":"a@b.com"}}"#
            ))
            .is_ok()
        );
    }

    #[test]
    fn update_requires_id_and_properties() {
        assert!(validate_crud(&crud(r#"{"operation":"update","id":"1"}"#)).is_err());
        assert!(validate_crud(&crud(r#"{"operation":"update","properties":{"x":1}}"#)).is_err());
        assert!(
            validate_crud(&crud(
                r#"{"operation":"update","id":"1","properties":{"x":1}}"#
            ))
            .is_ok()
        );
    }

    #[test]
    fn get_requires_id() {
        assert!(validate_crud(&crud(r#"{"operation":"get"}"#)).is_err());
        assert!(validate_crud(&crud(r#"{"operation":"get","id":"42"}"#)).is_ok());
    }

    #[test]
    fn search_requires_nonempty_query() {
        assert!(validate_crud(&crud(r#"{"operation":"search"}"#)).is_err());
        assert!(validate_crud(&crud(r#"{"operation":"search","query":"  "}"#)).is_err());
        assert!(validate_crud(&crud(r#"{"operation":"search","query":"acme"}"#)).is_ok());
    }

    #[test]
    fn associate_decodes_all_fields() {
        let input: AssociateInput = serde_json::from_str(
            r#"{"from_object":"contacts","from_id":"1","to_object":"companies","to_id":"2"}"#,
        )
        .unwrap();
        assert_eq!(input.from_object, "contacts");
        assert_eq!(input.to_id, "2");
        assert!(input.association_type.is_none());
    }

    #[test]
    fn pipelines_input_decodes_with_and_without_pipeline_id() {
        let without: PipelinesInput = serde_json::from_str(r#"{"object_type":"deals"}"#).unwrap();
        assert_eq!(without.object_type, "deals");
        assert!(without.pipeline_id.is_none());

        let with_id: PipelinesInput =
            serde_json::from_str(r#"{"object_type":"tickets","pipeline_id":"p-99"}"#).unwrap();
        assert_eq!(with_id.object_type, "tickets");
        assert_eq!(with_id.pipeline_id.as_deref(), Some("p-99"));
    }

    #[test]
    fn owners_input_decodes_with_and_without_owner_id() {
        let without: OwnersInput = serde_json::from_str(r"{}").unwrap();
        assert!(without.owner_id.is_none());
        assert!(without.limit.is_none());

        let with_id: OwnersInput = serde_json::from_str(r#"{"owner_id":"42","limit":50}"#).unwrap();
        assert_eq!(with_id.owner_id.as_deref(), Some("42"));
        assert_eq!(with_id.limit, Some(50));
    }

    #[test]
    fn batch_input_decodes_all_three_fields() {
        let raw =
            r#"{"object_type":"contacts","operation":"read","inputs":[{"id":"1"},{"id":"2"}]}"#;
        let input: BatchInput = serde_json::from_str(raw).unwrap();
        assert_eq!(input.object_type, "contacts");
        assert_eq!(input.operation, "read");
        assert!(input.inputs.is_array());
        assert_eq!(input.inputs.as_array().unwrap().len(), 2);
    }

    #[test]
    fn batch_input_requires_all_fields() {
        // missing inputs field → deserialization error
        assert!(
            serde_json::from_str::<BatchInput>(r#"{"object_type":"contacts","operation":"read"}"#)
                .is_err()
        );
    }
}
