//! Schema IR for the component's `describe()` payload.
//!
//! Mirrors component-http / component-transform — the runner reads the same
//! encoding from every component, so this is a shared wire format rather than a
//! per-component choice.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct I18nText {
    pub key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaField {
    pub required: bool,
    pub schema: SchemaIr,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SchemaIr {
    String {
        title: I18nText,
        description: I18nText,
        #[serde(skip_serializing_if = "Option::is_none")]
        format: Option<String>,
        secret: bool,
    },
    Bool {
        title: I18nText,
        description: I18nText,
    },
    Object {
        title: I18nText,
        description: I18nText,
        fields: BTreeMap<String, SchemaField>,
        additional_properties: bool,
    },
}

fn i18n(key: &str) -> I18nText {
    I18nText {
        key: key.to_string(),
    }
}

/// Every operation takes a `token` and its own arguments.
///
/// `token` is marked `secret: true` — that flag is what stops the value being
/// rendered or logged like an ordinary field, and every operation here carries
/// a Notion credential.
///
/// The remaining arguments are left open. The authoritative, field-level schema
/// an operator authors against is the node type's `config_schema` in
/// `greentic.notion`'s describe.json, which is the TOOL's own `input_schema`.
/// A second hand-built schema per operation would be a rival source of truth for
/// the same eight argument shapes with nothing detecting drift.
pub fn input_schema() -> SchemaIr {
    let mut fields = BTreeMap::new();
    fields.insert(
        "token".to_string(),
        SchemaField {
            required: true,
            schema: SchemaIr::String {
                title: i18n("twilio.schema.input.token.title"),
                description: i18n("twilio.schema.input.token.description"),
                format: None,
                secret: true,
            },
        },
    );
    SchemaIr::Object {
        title: i18n("twilio.schema.input.title"),
        description: i18n("twilio.schema.input.description"),
        fields,
        additional_properties: true,
    }
}

/// The `{ok, result}` / `{ok, error}` envelope every operation returns.
pub fn output_schema() -> SchemaIr {
    let mut fields = BTreeMap::new();
    fields.insert(
        "ok".to_string(),
        SchemaField {
            required: true,
            schema: SchemaIr::Bool {
                title: i18n("twilio.schema.output.ok.title"),
                description: i18n("twilio.schema.output.ok.description"),
            },
        },
    );
    fields.insert(
        "error".to_string(),
        SchemaField {
            required: false,
            schema: SchemaIr::String {
                title: i18n("twilio.schema.output.error.title"),
                description: i18n("twilio.schema.output.error.description"),
                format: None,
                secret: false,
            },
        },
    );
    SchemaIr::Object {
        title: i18n("twilio.schema.output.title"),
        description: i18n("twilio.schema.output.description"),
        fields,
        additional_properties: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The credential must be flagged, or it is rendered and logged like any
    /// other string.
    #[test]
    fn the_token_field_is_marked_secret_and_required() {
        let SchemaIr::Object { fields, .. } = input_schema() else {
            panic!("input schema must be an object");
        };
        let token = &fields["token"];
        assert!(token.required);
        match &token.schema {
            SchemaIr::String { secret, .. } => assert!(*secret, "`token` must be secret"),
            _ => panic!("`token` must be a string"),
        }
    }

    #[test]
    fn the_output_schema_requires_only_the_routable_field() {
        let SchemaIr::Object { fields, .. } = output_schema() else {
            panic!("output schema must be an object");
        };
        assert!(fields["ok"].required);
        assert!(!fields["error"].required);
    }
}
