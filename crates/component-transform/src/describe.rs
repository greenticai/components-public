//! Schema IR for the transform component's `describe()` payload.
//!
//! Mirrors `component-http`'s shape — the runner reads the same `SchemaIr`
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
    Array {
        title: I18nText,
        description: I18nText,
        items: Box<SchemaIr>,
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

/// `json_pick`'s input: the object to project, and the keys to keep.
///
/// `data` is `additional_properties: true` because it is arbitrary caller data
/// — constraining it would reject exactly the payloads this operation exists to
/// narrow.
pub fn input_schema() -> SchemaIr {
    let mut fields = BTreeMap::new();
    fields.insert(
        "data".to_string(),
        SchemaField {
            required: true,
            schema: SchemaIr::Object {
                title: i18n("transform.schema.input.data.title"),
                description: i18n("transform.schema.input.data.description"),
                fields: BTreeMap::new(),
                additional_properties: true,
            },
        },
    );
    fields.insert(
        "keys".to_string(),
        SchemaField {
            required: true,
            schema: SchemaIr::Array {
                title: i18n("transform.schema.input.keys.title"),
                description: i18n("transform.schema.input.keys.description"),
                items: Box::new(SchemaIr::String {
                    title: i18n("transform.schema.input.keys.item.title"),
                    description: i18n("transform.schema.input.keys.item.description"),
                    format: None,
                    secret: false,
                }),
            },
        },
    );
    SchemaIr::Object {
        title: i18n("transform.schema.input.title"),
        description: i18n("transform.schema.input.description"),
        fields,
        additional_properties: false,
    }
}

/// The `{ok, result}` / `{ok, error}` envelope every operation returns.
///
/// `ok` is what a flow routes on, so it is REQUIRED; `result` and `error` are
/// not, because exactly one of them is present per outcome.
pub fn output_schema() -> SchemaIr {
    let mut fields = BTreeMap::new();
    fields.insert(
        "ok".to_string(),
        SchemaField {
            required: true,
            schema: SchemaIr::Bool {
                title: i18n("transform.schema.output.ok.title"),
                description: i18n("transform.schema.output.ok.description"),
            },
        },
    );
    fields.insert(
        "result".to_string(),
        SchemaField {
            required: false,
            schema: SchemaIr::Object {
                title: i18n("transform.schema.output.result.title"),
                description: i18n("transform.schema.output.result.description"),
                fields: BTreeMap::new(),
                additional_properties: true,
            },
        },
    );
    fields.insert(
        "error".to_string(),
        SchemaField {
            required: false,
            schema: SchemaIr::String {
                title: i18n("transform.schema.output.error.title"),
                description: i18n("transform.schema.output.error.description"),
                format: None,
                secret: false,
            },
        },
    );
    SchemaIr::Object {
        title: i18n("transform.schema.output.title"),
        description: i18n("transform.schema.output.description"),
        fields,
        additional_properties: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_input_schema_requires_both_fields_the_handler_reads() {
        let SchemaIr::Object { fields, .. } = input_schema() else {
            panic!("input schema must be an object");
        };
        assert!(fields["data"].required, "`data` is read unconditionally");
        assert!(fields["keys"].required, "`keys` is read unconditionally");
    }

    /// `ok` is the field a flow routes on, so it must be required even though
    /// `result` and `error` are not — exactly one of those two appears.
    #[test]
    fn the_output_schema_requires_only_the_routable_field() {
        let SchemaIr::Object { fields, .. } = output_schema() else {
            panic!("output schema must be an object");
        };
        assert!(fields["ok"].required);
        assert!(!fields["result"].required);
        assert!(!fields["error"].required);
    }
}
