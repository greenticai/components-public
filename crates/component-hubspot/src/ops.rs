//! One handler per HubSpot tool.
//!
//! `hubspot`, `input`, `output` and `auth` are the design extension's WIT-free
//! modules, verbatim. Only marshalling, the host seam and the `{ok, …}`
//! envelope are new.
//!
//! Nine of the thirteen tools are the SAME CRUD shape over a different object,
//! resolved through the extension's own `object_for_tool` table — so they come
//! from one macro. The other four (`associate`, `pipelines`, `owners`, `batch`)
//! each have their own builder and are written out.

use serde_json::Value;

use crate::hubspot::{self, HttpCall};
use crate::input::{AssociateInput, BatchInput, CrudInput, Operation, OwnersInput, PipelinesInput};
use crate::transport::{HttpReq, check, resolve_secret, send};
use crate::{output, tool_meta};

const BASE: &str = "https://api.hubapi.com";
const TOKEN_URL: &str = "https://api.hubapi.com/oauth/v1/token";

pub fn ok(result: Value) -> Value {
    serde_json::json!({ "ok": true, "result": result })
}

pub fn err(message: impl std::fmt::Display) -> Value {
    serde_json::json!({ "ok": false, "error": message.to_string() })
}

fn opt(input: &Value, name: &str) -> Option<String> {
    input
        .get(name)
        .and_then(Value::as_str)
        .and_then(|raw| resolve_secret(raw).ok())
}

/// Resolve the bearer token, mirroring the extension's own precedence minus the
/// broker arm a component cannot import.
///
/// A brokerless refresh IS carried over — HubSpot's refresh-token grant needs
/// only HTTP and secrets, and both halves of it (`build_refresh_form`,
/// `extract_refreshed_token`) are pure functions in the copied `auth` module.
/// So a node supports both of the extension's real paths: a Private App token,
/// or a stored refresh grant. Only the broker fallback is absent.
fn access_token(input: &Value) -> Result<String, Value> {
    if let (Some(refresh), Some(id), Some(secret)) = (
        opt(input, "oauth_refresh_token"),
        opt(input, "oauth_client_id"),
        opt(input, "oauth_client_secret"),
    ) && !refresh.is_empty()
    {
        let form = crate::auth::build_refresh_form(&id, &secret, &refresh);
        let resp = send(HttpReq {
            method: "POST".into(),
            url: TOKEN_URL.into(),
            headers: vec![(
                "content-type".into(),
                "application/x-www-form-urlencoded".into(),
            )],
            body: Some(form.into_bytes()),
        })
        .map_err(err)?;
        let raw = check(resp).map_err(|e| err(format!("token refresh failed: {e}")))?;
        return crate::auth::extract_refreshed_token(&String::from_utf8_lossy(&raw))
            .map_err(|e| err(format!("token refresh returned no access token: {e:?}")));
    }

    opt(input, "access_token")
        .filter(|t| !t.is_empty())
        .ok_or_else(|| {
            err(
                "missing required field `access_token` (a Private App token, or `secret:NAME`) — \
                 alternatively supply `oauth_refresh_token`, `oauth_client_id` and \
                 `oauth_client_secret`",
            )
        })
}

fn dispatch_call(call: &HttpCall, token: &str) -> Result<Vec<u8>, Value> {
    let body = match &call.body {
        Some(value) => {
            Some(serde_json::to_vec(value).map_err(|e| err(format!("encode body: {e}")))?)
        }
        None => None,
    };
    let resp = send(HttpReq {
        method: call.method.as_str().to_string(),
        url: format!("{BASE}{}", call.path),
        headers: vec![
            ("content-type".to_string(), "application/json".to_string()),
            ("authorization".to_string(), format!("Bearer {token}")),
        ],
        body,
    })
    .map_err(err)?;
    check(resp).map_err(err)
}

fn parse<T: serde::de::DeserializeOwned>(input: &Value, what: &str) -> Result<T, Value> {
    serde_json::from_value(input.clone()).map_err(|e| err(format!("decode {what} input: {e}")))
}

/// The nine CRUD tools. `object_for_tool` is the extension's own table, so the
/// node and the tool agree on which HubSpot object a tool name means.
macro_rules! crud {
    ($fn_name:ident, $tool:expr) => {
        pub fn $fn_name(node_input: &Value) -> Value {
            let token = match access_token(node_input) {
                Ok(t) => t,
                Err(e) => return e,
            };
            let Some(object) = tool_meta::object_for_tool($tool) else {
                return err(concat!("unknown tool: ", $tool));
            };
            let parsed: CrudInput = match parse(node_input, $tool) {
                Ok(p) => p,
                Err(e) => return e,
            };
            let call = match hubspot::build_crud_call(object, &parsed) {
                Ok(c) => c,
                Err(e) => return err(e),
            };
            let raw = match dispatch_call(&call, &token) {
                Ok(r) => r,
                Err(e) => return e,
            };
            // Search returns a page of records; everything else returns one.
            let mapped = if parsed.operation == Operation::Search {
                output::map_search(object, &raw)
                    .map(|v| serde_json::to_value(v).unwrap_or(Value::Null))
            } else {
                output::map_record(object, &raw)
                    .map(|v| serde_json::to_value(v).unwrap_or(Value::Null))
            };
            match mapped {
                Ok(v) => ok(v),
                Err(e) => err(e),
            }
        }
    };
}

crud!(hubspot_contacts, "hubspot_contacts");
crud!(hubspot_deals, "hubspot_deals");
crud!(hubspot_companies, "hubspot_companies");
crud!(hubspot_tickets, "hubspot_tickets");
crud!(hubspot_notes, "hubspot_notes");
crud!(hubspot_tasks, "hubspot_tasks");
crud!(hubspot_calls, "hubspot_calls");
crud!(hubspot_meetings, "hubspot_meetings");
crud!(hubspot_emails, "hubspot_emails");

/// Associate returns the extension's own synthesised acknowledgement rather
/// than HubSpot's body — kept, because HubSpot answers an association with an
/// empty 204 that carries nothing a downstream node could reference.
pub fn hubspot_associate(node_input: &Value) -> Value {
    let token = match access_token(node_input) {
        Ok(t) => t,
        Err(e) => return e,
    };
    let parsed: AssociateInput = match parse(node_input, "hubspot_associate") {
        Ok(p) => p,
        Err(e) => return e,
    };
    let call = match hubspot::build_associate_call(&parsed) {
        Ok(c) => c,
        Err(e) => return err(e),
    };
    if let Err(e) = dispatch_call(&call, &token) {
        return e;
    }
    ok(output::associate_result(&parsed))
}

macro_rules! passthrough {
    ($fn_name:ident, $ty:ty, $what:literal, $build:path) => {
        pub fn $fn_name(node_input: &Value) -> Value {
            let token = match access_token(node_input) {
                Ok(t) => t,
                Err(e) => return e,
            };
            let parsed: $ty = match parse(node_input, $what) {
                Ok(p) => p,
                Err(e) => return e,
            };
            let call = match $build(&parsed) {
                Ok(c) => c,
                Err(e) => return err(e),
            };
            let raw = match dispatch_call(&call, &token) {
                Ok(r) => r,
                Err(e) => return e,
            };
            match serde_json::from_slice::<Value>(&raw) {
                Ok(v) => ok(v),
                Err(e) => err(format!("HubSpot returned unparseable JSON: {e}")),
            }
        }
    };
}

passthrough!(
    hubspot_pipelines,
    PipelinesInput,
    "hubspot_pipelines",
    hubspot::build_pipelines_call
);
passthrough!(
    hubspot_owners,
    OwnersInput,
    "hubspot_owners",
    hubspot::build_owners_call
);
passthrough!(
    hubspot_batch,
    BatchInput,
    "hubspot_batch",
    hubspot::build_batch_call
);
