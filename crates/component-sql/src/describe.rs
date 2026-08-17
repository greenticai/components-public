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

/// The operation takes two credentials and its own arguments.
///
/// Both are marked `secret: true` — that flag is what stops a value being
/// rendered or logged like an ordinary field. There are two because the node
/// talks to two different services: the SQL gateway and the LLM that writes the
/// query.
///
/// The remaining arguments are left open. The authoritative, field-level schema
/// an operator authors against is the node type's `config_schema` in
/// `greentic.sql`'s describe.json. A second hand-built schema here would be a
/// rival source of truth with nothing detecting drift.
pub fn input_schema() -> SchemaIr {
    let mut fields = BTreeMap::new();
    for name in ["gateway_token", "llm_api_key"] {
        fields.insert(
            name.to_string(),
            SchemaField {
                required: true,
                schema: SchemaIr::String {
                    title: i18n(&format!("sql.schema.input.{name}.title")),
                    description: i18n(&format!("sql.schema.input.{name}.description")),
                    format: None,
                    secret: true,
                },
            },
        );
    }
    SchemaIr::Object {
        title: i18n("sql.schema.input.title"),
        description: i18n("sql.schema.input.description"),
        fields,
        additional_properties: true,
    }
}

pub fn output_schema() -> SchemaIr {
    let mut fields = BTreeMap::new();
    fields.insert(
        "ok".to_string(),
        SchemaField {
            required: true,
            schema: SchemaIr::Bool {
                title: i18n("sql.schema.output.ok.title"),
                description: i18n("sql.schema.output.ok.description"),
            },
        },
    );
    fields.insert(
        "error".to_string(),
        SchemaField {
            required: false,
            schema: SchemaIr::String {
                title: i18n("sql.schema.output.error.title"),
                description: i18n("sql.schema.output.error.description"),
                format: None,
                secret: false,
            },
        },
    );
    SchemaIr::Object {
        title: i18n("sql.schema.output.title"),
        description: i18n("sql.schema.output.description"),
        fields,
        additional_properties: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Both credentials must be flagged, or they are rendered and logged like
    /// any other string. There are two because the node talks to two services.
    #[test]
    fn both_credential_fields_are_marked_secret_and_required() {
        let SchemaIr::Object { fields, .. } = input_schema() else {
            panic!("input schema must be an object");
        };
        for name in ["gateway_token", "llm_api_key"] {
            let field = fields
                .get(name)
                .unwrap_or_else(|| panic!("`{name}` must be declared"));
            assert!(field.required, "`{name}` must be required");
            match &field.schema {
                SchemaIr::String { secret, .. } => assert!(*secret, "`{name}` must be secret"),
                _ => panic!("`{name}` must be a string"),
            }
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
