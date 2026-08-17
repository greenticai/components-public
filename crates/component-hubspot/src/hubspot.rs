//! Pure REST mapping for the HubSpot CRM v3/v4 API.
//!
//! Turns a validated tool input into the `(method, path, body)` triple that
//! `lib.rs` sends to `https://api.hubapi.com` via the host `http` capability.
//! No WIT/network imports — fully host-testable. Paths are absolute and begin
//! with `/`; `lib.rs` prefixes the `https://api.hubapi.com` origin.

// Copied verbatim from the design extension. The only edit is this attribute:
// several structs and tables exist for the TOOL surface and are unused by the
// node surface. Silencing it here keeps the rest of the file diffable against
// its source.
#![allow(dead_code)]
use std::fmt::Write as _;

use serde_json::{Value, json};

use crate::input::{
    AssociateInput, BatchInput, CrudInput, Operation, OwnersInput, PipelinesInput, validate_crud,
};

#[derive(Debug, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    Patch,
    Put,
}

impl Method {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Patch => "PATCH",
            Method::Put => "PUT",
        }
    }
}

/// A fully-resolved HubSpot REST call: HTTP method, absolute path, optional
/// JSON request body.
#[derive(Debug, PartialEq)]
pub struct HttpCall {
    pub method: Method,
    pub path: String,
    pub body: Option<Value>,
}

/// CRM object types this extension supports.
pub const VALID_OBJECTS: [&str; 9] = [
    "contacts",
    "deals",
    "companies",
    "tickets",
    "notes",
    "tasks",
    "calls",
    "meetings",
    "emails",
];

fn ensure_object(object: &str) -> Result<(), String> {
    if VALID_OBJECTS.contains(&object) {
        Ok(())
    } else {
        Err(format!(
            "unsupported object '{object}'; expected one of {VALID_OBJECTS:?}"
        ))
    }
}

/// Default search page size when the caller omits `limit`.
const DEFAULT_SEARCH_LIMIT: u32 = 10;

/// Build the REST call for a per-object CRUD tool. Validates the input first.
pub fn build_crud_call(object: &str, input: &CrudInput) -> Result<HttpCall, String> {
    ensure_object(object)?;
    validate_crud(input)?;

    match input.operation {
        Operation::Create => Ok(HttpCall {
            method: Method::Post,
            path: format!("/crm/v3/objects/{object}"),
            body: Some(json!({ "properties": properties(input) })),
        }),
        Operation::Update => {
            let id = input.id.as_deref().unwrap_or_default();
            Ok(HttpCall {
                method: Method::Patch,
                path: format!("/crm/v3/objects/{object}/{id}"),
                body: Some(json!({ "properties": properties(input) })),
            })
        }
        Operation::Get => {
            let id = input.id.as_deref().unwrap_or_default();
            let mut path = format!("/crm/v3/objects/{object}/{id}");
            if let Some(props) = &input.return_properties
                && !props.is_empty()
            {
                let _ = write!(path, "?properties={}", props.join(","));
            }
            Ok(HttpCall {
                method: Method::Get,
                path,
                body: None,
            })
        }
        Operation::Search => {
            let query = input.query.as_deref().unwrap_or_default();
            let limit = input.limit.unwrap_or(DEFAULT_SEARCH_LIMIT);
            let mut body = json!({ "query": query, "limit": limit });
            if let Some(props) = &input.return_properties
                && !props.is_empty()
            {
                body["properties"] = json!(props);
            }
            Ok(HttpCall {
                method: Method::Post,
                path: format!("/crm/v3/objects/{object}/search"),
                body: Some(body),
            })
        }
    }
}

fn properties(input: &CrudInput) -> Value {
    input
        .properties
        .clone()
        .map_or_else(|| json!({}), Value::Object)
}

/// Build the REST call that creates a default association between two records
/// (HubSpot Associations v4). `association_type` is reserved for custom labels
/// (a follow-up); v0.1.0 always creates the default association.
pub fn build_associate_call(input: &AssociateInput) -> Result<HttpCall, String> {
    ensure_object(&input.from_object)?;
    ensure_object(&input.to_object)?;
    if input.from_id.trim().is_empty() || input.to_id.trim().is_empty() {
        return Err("associate requires non-empty 'from_id' and 'to_id'".to_string());
    }
    Ok(HttpCall {
        method: Method::Put,
        path: format!(
            "/crm/v4/objects/{}/{}/associations/default/{}/{}",
            input.from_object, input.from_id, input.to_object, input.to_id
        ),
        body: None,
    })
}

/// Build the REST call for the pipeline-stages lookup tool.
///
/// Only `"deals"` and `"tickets"` have HubSpot-managed pipelines; other
/// object types are rejected. When `pipeline_id` is `Some`, returns a single
/// pipeline; otherwise lists all pipelines for the object type.
pub fn build_pipelines_call(input: &PipelinesInput) -> Result<HttpCall, String> {
    if !["deals", "tickets"].contains(&input.object_type.as_str()) {
        return Err(format!(
            "unsupported object_type '{}'; expected 'deals' or 'tickets'",
            input.object_type
        ));
    }
    let path = match &input.pipeline_id {
        Some(id) => format!("/crm/v3/pipelines/{}/{}", input.object_type, id),
        None => format!("/crm/v3/pipelines/{}", input.object_type),
    };
    Ok(HttpCall {
        method: Method::Get,
        path,
        body: None,
    })
}

/// Build the REST call for the owners lookup tool.
///
/// When `owner_id` is `Some`, returns a single owner by id; otherwise lists
/// owners up to `limit` (default 100).
pub fn build_owners_call(input: &OwnersInput) -> Result<HttpCall, String> {
    let path = match &input.owner_id {
        Some(id) => format!("/crm/v3/owners/{id}"),
        None => format!("/crm/v3/owners?limit={}", input.limit.unwrap_or(100)),
    };
    Ok(HttpCall {
        method: Method::Get,
        path,
        body: None,
    })
}

/// Build the REST call for the batch read/create/update tool.
///
/// `operation` must be one of `"read"`, `"create"`, or `"update"`. `inputs`
/// must be a non-empty JSON array whose element shape depends on the operation:
/// read → `[{id}]`, create → `[{properties}]`, update → `[{id, properties}]`.
/// The caller supplies the correct shape; this function only validates that
/// the array is present and non-empty.
pub fn build_batch_call(input: &BatchInput) -> Result<HttpCall, String> {
    ensure_object(&input.object_type)?;
    if !["read", "create", "update"].contains(&input.operation.as_str()) {
        return Err(format!(
            "unsupported batch operation '{}'; expected read/create/update",
            input.operation
        ));
    }
    let arr = input
        .inputs
        .as_array()
        .ok_or_else(|| "batch requires an 'inputs' array".to_string())?;
    if arr.is_empty() {
        return Err("batch requires a non-empty 'inputs' array".to_string());
    }
    Ok(HttpCall {
        method: Method::Post,
        path: format!(
            "/crm/v3/objects/{}/batch/{}",
            input.object_type, input.operation
        ),
        body: Some(json!({ "inputs": input.inputs.clone() })),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::{AssociateInput, BatchInput, CrudInput, OwnersInput, PipelinesInput};

    fn crud(json: &str) -> CrudInput {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn create_maps_to_post_with_properties() {
        let call = build_crud_call(
            "contacts",
            &crud(r#"{"operation":"create","properties":{"email":"a@b.com"}}"#),
        )
        .unwrap();
        assert_eq!(call.method, Method::Post);
        assert_eq!(call.path, "/crm/v3/objects/contacts");
        assert_eq!(call.body.unwrap()["properties"]["email"], "a@b.com");
    }

    #[test]
    fn update_maps_to_patch_with_id() {
        let call = build_crud_call(
            "deals",
            &crud(r#"{"operation":"update","id":"99","properties":{"amount":"500"}}"#),
        )
        .unwrap();
        assert_eq!(call.method, Method::Patch);
        assert_eq!(call.path, "/crm/v3/objects/deals/99");
        assert_eq!(call.body.unwrap()["properties"]["amount"], "500");
    }

    #[test]
    fn get_maps_to_get_with_optional_properties_query() {
        let plain = build_crud_call("companies", &crud(r#"{"operation":"get","id":"7"}"#)).unwrap();
        assert_eq!(plain.method, Method::Get);
        assert_eq!(plain.path, "/crm/v3/objects/companies/7");
        assert!(plain.body.is_none());

        let with_props = build_crud_call(
            "companies",
            &crud(r#"{"operation":"get","id":"7","return_properties":["name","domain"]}"#),
        )
        .unwrap();
        assert_eq!(
            with_props.path,
            "/crm/v3/objects/companies/7?properties=name,domain"
        );
    }

    #[test]
    fn search_maps_to_search_endpoint_with_query_and_default_limit() {
        let call = build_crud_call(
            "tickets",
            &crud(r#"{"operation":"search","query":"refund"}"#),
        )
        .unwrap();
        assert_eq!(call.method, Method::Post);
        assert_eq!(call.path, "/crm/v3/objects/tickets/search");
        let body = call.body.unwrap();
        assert_eq!(body["query"], "refund");
        assert_eq!(body["limit"], 10);
        assert!(body.get("properties").is_none());
    }

    #[test]
    fn search_passes_limit_and_return_properties() {
        let call = build_crud_call(
            "contacts",
            &crud(
                r#"{"operation":"search","query":"acme","limit":3,"return_properties":["email"]}"#,
            ),
        )
        .unwrap();
        let body = call.body.unwrap();
        assert_eq!(body["limit"], 3);
        assert_eq!(body["properties"], serde_json::json!(["email"]));
    }

    #[test]
    fn rejects_unknown_object() {
        assert!(build_crud_call("widgets", &crud(r#"{"operation":"get","id":"1"}"#)).is_err());
    }

    #[test]
    fn invalid_input_is_rejected_before_mapping() {
        // get without id
        assert!(build_crud_call("contacts", &crud(r#"{"operation":"get"}"#)).is_err());
    }

    #[test]
    fn associate_maps_to_v4_default_endpoint() {
        let input: AssociateInput = serde_json::from_str(
            r#"{"from_object":"contacts","from_id":"1","to_object":"companies","to_id":"2"}"#,
        )
        .unwrap();
        let call = build_associate_call(&input).unwrap();
        assert_eq!(call.method, Method::Put);
        assert_eq!(
            call.path,
            "/crm/v4/objects/contacts/1/associations/default/companies/2"
        );
        assert!(call.body.is_none());
    }

    #[test]
    fn associate_rejects_unknown_object_and_empty_ids() {
        let bad_obj: AssociateInput = serde_json::from_str(
            r#"{"from_object":"foo","from_id":"1","to_object":"companies","to_id":"2"}"#,
        )
        .unwrap();
        assert!(build_associate_call(&bad_obj).is_err());

        let empty_id: AssociateInput = serde_json::from_str(
            r#"{"from_object":"contacts","from_id":"","to_object":"companies","to_id":"2"}"#,
        )
        .unwrap();
        assert!(build_associate_call(&empty_id).is_err());
    }

    #[test]
    fn pipelines_list_returns_get_with_object_path() {
        let input: PipelinesInput = serde_json::from_str(r#"{"object_type":"deals"}"#).unwrap();
        let call = build_pipelines_call(&input).unwrap();
        assert_eq!(call.method, Method::Get);
        assert_eq!(call.path, "/crm/v3/pipelines/deals");
        assert!(call.body.is_none());
    }

    #[test]
    fn pipelines_single_appends_pipeline_id() {
        let input: PipelinesInput =
            serde_json::from_str(r#"{"object_type":"deals","pipeline_id":"123"}"#).unwrap();
        let call = build_pipelines_call(&input).unwrap();
        assert_eq!(call.path, "/crm/v3/pipelines/deals/123");
    }

    #[test]
    fn pipelines_rejects_unsupported_object_type() {
        let input: PipelinesInput = serde_json::from_str(r#"{"object_type":"contacts"}"#).unwrap();
        assert!(build_pipelines_call(&input).is_err());
    }

    #[test]
    fn owners_list_uses_default_limit_100() {
        let input: OwnersInput = serde_json::from_str(r"{}").unwrap();
        let call = build_owners_call(&input).unwrap();
        assert_eq!(call.method, Method::Get);
        assert_eq!(call.path, "/crm/v3/owners?limit=100");
        assert!(call.body.is_none());
    }

    #[test]
    fn owners_single_uses_owner_id() {
        let input: OwnersInput = serde_json::from_str(r#"{"owner_id":"42"}"#).unwrap();
        let call = build_owners_call(&input).unwrap();
        assert_eq!(call.path, "/crm/v3/owners/42");
    }

    #[test]
    fn owners_list_respects_custom_limit() {
        let input: OwnersInput = serde_json::from_str(r#"{"limit":25}"#).unwrap();
        let call = build_owners_call(&input).unwrap();
        assert_eq!(call.path, "/crm/v3/owners?limit=25");
    }

    fn batch(json: &str) -> BatchInput {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn batch_read_maps_to_correct_path() {
        let call = build_batch_call(&batch(
            r#"{"object_type":"contacts","operation":"read","inputs":[{"id":"1"},{"id":"2"}]}"#,
        ))
        .unwrap();
        assert_eq!(call.method, Method::Post);
        assert_eq!(call.path, "/crm/v3/objects/contacts/batch/read");
        let body = call.body.unwrap();
        assert_eq!(body["inputs"][0]["id"], "1");
    }

    #[test]
    fn batch_create_maps_to_correct_path() {
        let call = build_batch_call(&batch(
            r#"{"object_type":"deals","operation":"create","inputs":[{"properties":{"dealname":"Acme"}}]}"#,
        ))
        .unwrap();
        assert_eq!(call.path, "/crm/v3/objects/deals/batch/create");
    }

    #[test]
    fn batch_update_maps_to_correct_path() {
        let call = build_batch_call(&batch(
            r#"{"object_type":"tickets","operation":"update","inputs":[{"id":"5","properties":{"subject":"Fixed"}}]}"#,
        ))
        .unwrap();
        assert_eq!(call.path, "/crm/v3/objects/tickets/batch/update");
    }

    #[test]
    fn batch_rejects_unknown_object_type() {
        assert!(
            build_batch_call(&batch(
                r#"{"object_type":"widgets","operation":"read","inputs":[{"id":"1"}]}"#
            ))
            .is_err()
        );
    }

    #[test]
    fn batch_rejects_unsupported_operation() {
        assert!(
            build_batch_call(&batch(
                r#"{"object_type":"contacts","operation":"delete","inputs":[{"id":"1"}]}"#
            ))
            .is_err()
        );
    }

    #[test]
    fn batch_rejects_empty_inputs_array() {
        assert!(
            build_batch_call(&batch(
                r#"{"object_type":"contacts","operation":"read","inputs":[]}"#
            ))
            .is_err()
        );
    }

    #[test]
    fn batch_rejects_non_array_inputs() {
        assert!(
            build_batch_call(&batch(
                r#"{"object_type":"contacts","operation":"read","inputs":{"id":"1"}}"#
            ))
            .is_err()
        );
    }
}
