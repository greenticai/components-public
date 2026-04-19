//! Schema IR types and describe payload builder for the HTTP component.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// ============================================================================
// Internal serializable types
// ============================================================================

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
    Number {
        title: I18nText,
        description: I18nText,
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

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationDescriptor {
    pub name: String,
    pub title: I18nText,
    pub description: I18nText,
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedactionRule {
    pub path: String,
    pub strategy: String,
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DescribePayload {
    pub provider: String,
    pub world: String,
    pub operations: Vec<OperationDescriptor>,
    pub input_schema: SchemaIr,
    pub output_schema: SchemaIr,
    pub config_schema: SchemaIr,
    pub redactions: Vec<RedactionRule>,
    pub schema_hash: String,
}

// ============================================================================
// Schema builders
// ============================================================================

pub fn input_schema() -> SchemaIr {
    let mut fields = BTreeMap::new();
    fields.insert(
        "url".to_string(),
        SchemaField {
            required: true,
            schema: SchemaIr::String {
                title: i18n("http.schema.input.url.title"),
                description: i18n("http.schema.input.url.description"),
                format: Some("uri".to_string()),
                secret: false,
            },
        },
    );
    fields.insert(
        "method".to_string(),
        SchemaField {
            required: false,
            schema: SchemaIr::String {
                title: i18n("http.schema.input.method.title"),
                description: i18n("http.schema.input.method.description"),
                format: None,
                secret: false,
            },
        },
    );
    fields.insert(
        "body".to_string(),
        SchemaField {
            required: false,
            schema: SchemaIr::String {
                title: i18n("http.schema.input.body.title"),
                description: i18n("http.schema.input.body.description"),
                format: None,
                secret: false,
            },
        },
    );

    SchemaIr::Object {
        title: i18n("http.schema.input.title"),
        description: i18n("http.schema.input.description"),
        fields,
        additional_properties: true,
    }
}

pub fn output_schema() -> SchemaIr {
    let mut fields = BTreeMap::new();
    fields.insert(
        "status".to_string(),
        SchemaField {
            required: true,
            schema: SchemaIr::Number {
                title: i18n("http.schema.output.status.title"),
                description: i18n("http.schema.output.status.description"),
            },
        },
    );
    fields.insert(
        "body".to_string(),
        SchemaField {
            required: true,
            schema: SchemaIr::String {
                title: i18n("http.schema.output.body.title"),
                description: i18n("http.schema.output.body.description"),
                format: None,
                secret: false,
            },
        },
    );

    SchemaIr::Object {
        title: i18n("http.schema.output.title"),
        description: i18n("http.schema.output.description"),
        fields,
        additional_properties: true,
    }
}

pub fn config_schema() -> SchemaIr {
    let mut fields = BTreeMap::new();
    fields.insert(
        "base_url".to_string(),
        SchemaField {
            required: false,
            schema: SchemaIr::String {
                title: i18n("http.schema.config.base_url.title"),
                description: i18n("http.schema.config.base_url.description"),
                format: Some("uri".to_string()),
                secret: false,
            },
        },
    );
    fields.insert(
        "auth_type".to_string(),
        SchemaField {
            required: false,
            schema: SchemaIr::String {
                title: i18n("http.schema.config.auth_type.title"),
                description: i18n("http.schema.config.auth_type.description"),
                format: None,
                secret: false,
            },
        },
    );
    fields.insert(
        "auth_token".to_string(),
        SchemaField {
            required: false,
            schema: SchemaIr::String {
                title: i18n("http.schema.config.auth_token.title"),
                description: i18n("http.schema.config.auth_token.description"),
                format: None,
                secret: true,
            },
        },
    );
    fields.insert(
        "timeout_ms".to_string(),
        SchemaField {
            required: false,
            schema: SchemaIr::Number {
                title: i18n("http.schema.config.timeout_ms.title"),
                description: i18n("http.schema.config.timeout_ms.description"),
            },
        },
    );

    SchemaIr::Object {
        title: i18n("http.schema.config.title"),
        description: i18n("http.schema.config.description"),
        fields,
        additional_properties: false,
    }
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub fn op(name: &str, title_key: &str, description_key: &str) -> OperationDescriptor {
    OperationDescriptor {
        name: name.to_string(),
        title: i18n(title_key),
        description: i18n(description_key),
    }
}

fn i18n(key: &str) -> I18nText {
    I18nText {
        key: key.to_string(),
    }
}
