//! Schema IR for the component's `describe()` payload.
//!
//! Mirrors component-http — the runner reads the same
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

/// Every operation reads `api_key` and `base_url` (wired to the secrets done/api_key and done/base_url by the designer) and its own arguments.
///
/// `api_key` is marked `secret: true` so the value is never rendered or logged
/// like an ordinary field. The field-level schema an operator authors against is
/// the node type's `config_schema` in the extension's describe.json.
pub fn input_schema() -> SchemaIr {
    let mut fields = BTreeMap::new();
    fields.insert(
        "api_key".to_string(),
        SchemaField {
            required: true,
            schema: SchemaIr::String {
                title: i18n("done.schema.input.api_key.title"),
                description: i18n("done.schema.input.api_key.description"),
                format: None,
                secret: true,
            },
        },
    );
    SchemaIr::Object {
        title: i18n("done.schema.input.title"),
        description: i18n("done.schema.input.description"),
        fields,
        additional_properties: true,
    }
}

/// The `{ok, id, state}` / `{ok:false, error:{code, message}}` envelope.
pub fn output_schema() -> SchemaIr {
    let mut fields = BTreeMap::new();
    fields.insert(
        "ok".to_string(),
        SchemaField {
            required: true,
            schema: SchemaIr::Bool {
                title: i18n("done.schema.output.ok.title"),
                description: i18n("done.schema.output.ok.description"),
            },
        },
    );
    fields.insert(
        "error".to_string(),
        SchemaField {
            required: false,
            schema: SchemaIr::Object {
                title: i18n("done.schema.output.error.title"),
                description: i18n("done.schema.output.error.description"),
                fields: BTreeMap::new(),
                additional_properties: true,
            },
        },
    );
    SchemaIr::Object {
        title: i18n("done.schema.output.title"),
        description: i18n("done.schema.output.description"),
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
    fn the_api_key_field_is_marked_secret_and_required() {
        let SchemaIr::Object { fields, .. } = input_schema() else {
            panic!("input schema must be an object");
        };
        let token = &fields["api_key"];
        assert!(token.required);
        match &token.schema {
            SchemaIr::String { secret, .. } => assert!(*secret, "`api_key` must be secret"),
            _ => panic!("`api_key` must be a string"),
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
