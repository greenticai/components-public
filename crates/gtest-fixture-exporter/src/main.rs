use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use greentic_types::cbor::canonical;
use greentic_types::i18n_text::I18nText;
use greentic_types::schemas::common::schema_ir::{AdditionalProperties, SchemaIr};
use greentic_types::schemas::component::v0_6_0::{
    ComponentDescribe, ComponentInfo, ComponentOperation, ComponentQaSpec, ComponentRunInput,
    ComponentRunOutput, QaMode, schema_hash,
};
use serde_json::{Value, json};

struct ComponentFixture {
    id: &'static str,
    version: &'static str,
    operations: &'static [&'static str],
    default_config: Value,
    setup_config: fn(&Value) -> Value,
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let mut reference = None;
    let mut out_dir = None;
    let mut default_answers = None;
    let mut setup_answers = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--reference" => reference = args.next(),
            "--out" => out_dir = args.next(),
            "--default-answers" => default_answers = args.next(),
            "--setup-answers" => setup_answers = args.next(),
            other => bail!("unexpected argument `{other}`"),
        }
    }

    let reference = reference.context("missing --reference")?;
    let out_dir = PathBuf::from(out_dir.context("missing --out")?);
    let default_answers = read_json_object(Path::new(
        &default_answers.context("missing --default-answers")?,
    ))?;
    let setup_answers = read_json_object(Path::new(
        &setup_answers.context("missing --setup-answers")?,
    ))?;

    let fixture = fixture_for_reference(&reference)
        .with_context(|| format!("unsupported component reference `{reference}`"))?;
    fs::create_dir_all(&out_dir)?;

    let describe = build_describe(&fixture);
    let describe_cbor = canonical::to_canonical_cbor_allow_floats(&describe)?;
    let qa_default_cbor = canonical::to_canonical_cbor(&qa_spec("default"))?;
    let qa_setup_cbor = canonical::to_canonical_cbor(&qa_spec("setup"))?;
    let apply_default_cbor = canonical::to_canonical_cbor_allow_floats(&merge_object_config(
        fixture.default_config.clone(),
        default_answers,
    ))?;
    let apply_setup_cbor =
        canonical::to_canonical_cbor_allow_floats(&(fixture.setup_config)(&setup_answers))?;

    let key = fixture_key(&reference);
    fs::write(out_dir.join(format!("{key}.describe.cbor")), describe_cbor)?;
    fs::write(
        out_dir.join(format!("{key}.qa-default.cbor")),
        qa_default_cbor,
    )?;
    fs::write(out_dir.join(format!("{key}.qa-setup.cbor")), qa_setup_cbor)?;
    fs::write(
        out_dir.join(format!("{key}.apply-default-config.cbor")),
        apply_default_cbor,
    )?;
    fs::write(
        out_dir.join(format!("{key}.apply-setup-config.cbor")),
        apply_setup_cbor,
    )?;
    fs::write(out_dir.join(format!("{key}.abi")), "0.6.0\n")?;

    Ok(())
}

fn read_json_object(path: &Path) -> Result<Value> {
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let value: Value =
        serde_json::from_str(&text).with_context(|| format!("parse {}", path.display()))?;
    if !value.is_object() {
        bail!("{} must contain a JSON object", path.display());
    }
    Ok(value)
}

fn fixture_key(reference: &str) -> String {
    reference
        .trim_start_matches("oci://")
        .trim_start_matches("repo://")
        .trim_start_matches("store://")
        .trim_start_matches("file://")
        .replace(['/', ':', '@'], "_")
}

fn permissive_object_schema() -> SchemaIr {
    SchemaIr::Object {
        properties: BTreeMap::new(),
        required: Vec::new(),
        additional: AdditionalProperties::Allow,
    }
}

fn build_describe(fixture: &ComponentFixture) -> ComponentDescribe {
    let config_schema = permissive_object_schema();
    let op_schema = permissive_object_schema();

    ComponentDescribe {
        info: ComponentInfo {
            id: fixture.id.to_string(),
            version: fixture.version.to_string(),
            role: "tool".to_string(),
            display_name: None,
        },
        provided_capabilities: Vec::new(),
        required_capabilities: Vec::new(),
        metadata: BTreeMap::new(),
        operations: fixture
            .operations
            .iter()
            .map(|op| ComponentOperation {
                id: (*op).to_string(),
                display_name: None,
                input: ComponentRunInput {
                    schema: op_schema.clone(),
                },
                output: ComponentRunOutput {
                    schema: op_schema.clone(),
                },
                defaults: BTreeMap::new(),
                redactions: Vec::new(),
                constraints: BTreeMap::new(),
                schema_hash: schema_hash(&op_schema, &op_schema, &config_schema)
                    .expect("schema hash"),
            })
            .collect(),
        config_schema,
    }
}

fn qa_spec(mode: &str) -> ComponentQaSpec {
    ComponentQaSpec {
        mode: match mode {
            "setup" => QaMode::Setup,
            "update" => QaMode::Update,
            "remove" => QaMode::Remove,
            _ => QaMode::Default,
        },
        title: I18nText::new(
            format!("gtest.{mode}.title"),
            Some(match mode {
                "setup" => "Setup".to_string(),
                "update" => "Update".to_string(),
                "remove" => "Remove".to_string(),
                _ => "Default".to_string(),
            }),
        ),
        description: Some(I18nText::new(
            format!("gtest.{mode}.description"),
            Some(format!("Fixture QA for {mode} mode")),
        )),
        questions: Vec::new(),
        defaults: BTreeMap::new(),
    }
}

fn merge_object_config(base: Value, overlay: Value) -> Value {
    match (base, overlay) {
        (Value::Object(mut base_map), Value::Object(overlay_map)) => {
            for (key, value) in overlay_map {
                base_map.insert(key, value);
            }
            Value::Object(base_map)
        }
        (_, overlay_value) => overlay_value,
    }
}

fn string_field(answers: &Value, key: &str) -> Option<String> {
    answers
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn u64_field(answers: &Value, key: &str) -> Option<u64> {
    answers.get(key).and_then(Value::as_u64)
}

fn object_field(answers: &Value, key: &str) -> Option<Value> {
    answers.get(key).filter(|value| value.is_object()).cloned()
}

fn passthrough(answers: &Value) -> Value {
    answers.clone()
}

fn renderer_setup(answers: &Value) -> Value {
    merge_object_config(
        json!({
            "max_list_items": 5,
            "max_table_columns": 4,
            "theme": {
                "accent_color": "#005A9C",
                "background_color": "#F5F9FF"
            }
        }),
        answers.clone(),
    )
}

fn translator_setup(answers: &Value) -> Value {
    merge_object_config(
        json!({
            "default_max_rows": 250,
            "github_api_url": "https://ghe.example/api/v3"
        }),
        answers.clone(),
    )
}

fn validator_setup(answers: &Value) -> Value {
    merge_object_config(json!({ "strict_mode": false }), answers.clone())
}

fn llm_setup(answers: &Value) -> Value {
    let mut config = serde_json::Map::new();
    if let Some(value) = string_field(answers, "api_key") {
        config.insert("api_key".to_string(), Value::String(value));
    }
    config.insert(
        "model".to_string(),
        Value::String(string_field(answers, "model").unwrap_or_else(|| "gpt-4.1-mini".to_string())),
    );
    config.insert(
        "temperature".to_string(),
        answers
            .get("temperature")
            .cloned()
            .unwrap_or_else(|| json!(0.2)),
    );
    config.insert(
        "timeout_ms".to_string(),
        Value::Number(u64_field(answers, "timeout_ms").unwrap_or(45_000).into()),
    );
    Value::Object(config)
}

fn executor_setup(answers: &Value) -> Value {
    let mut config = serde_json::Map::new();
    if let Some(value) = string_field(answers, "github_token") {
        config.insert("github_token".to_string(), Value::String(value));
    }
    if let Some(value) = string_field(answers, "github_api_url") {
        config.insert("github_api_url".to_string(), Value::String(value));
    }
    if let Some(value) = u64_field(answers, "timeout_ms") {
        config.insert("timeout_ms".to_string(), Value::Number(value.into()));
    }
    Value::Object(config)
}

fn events2msg_setup(answers: &Value) -> Value {
    let mut config = serde_json::Map::new();
    if let Some(value) = string_field(answers, "default_provider") {
        config.insert("default_provider".to_string(), Value::String(value));
    }
    if let Some(value) = string_field(answers, "default_channel") {
        config.insert("default_channel".to_string(), Value::String(value));
    }
    Value::Object(config)
}

fn msg2events_setup(answers: &Value) -> Value {
    let mut config = serde_json::Map::new();
    if let Some(value) = string_field(answers, "default_flow") {
        config.insert("default_flow".to_string(), Value::String(value));
    }
    if let Some(value) = string_field(answers, "default_event_type") {
        config.insert("default_event_type".to_string(), Value::String(value));
    }
    Value::Object(config)
}

fn http_setup(answers: &Value) -> Value {
    let mut config = serde_json::Map::new();
    if let Some(value) = string_field(answers, "base_url") {
        config.insert("base_url".to_string(), Value::String(value));
    }
    if let Some(value) = string_field(answers, "auth_type") {
        config.insert("auth_type".to_string(), Value::String(value));
    }
    if let Some(value) = string_field(answers, "auth_token") {
        config.insert("auth_token".to_string(), Value::String(value));
    }
    if let Some(value) = u64_field(answers, "timeout_ms") {
        config.insert("timeout_ms".to_string(), Value::Number(value.into()));
    }
    if let Some(value) = object_field(answers, "default_headers") {
        config.insert("default_headers".to_string(), value);
    }
    Value::Object(config)
}

fn fixture_for_reference(reference: &str) -> Option<ComponentFixture> {
    match reference {
        "repo://local/component-chat2data-llm" | "ai.greentic.component-chat2data-llm" => {
            Some(ComponentFixture {
                id: "ai.greentic.component-chat2data-llm",
                version: "0.1.0",
                operations: &["parse", "parse_multi", "select_renderer"],
                default_config: json!({}),
                setup_config: llm_setup,
            })
        }
        "repo://local/component-chat2data-executor"
        | "ai.greentic.component-chat2data-executor" => Some(ComponentFixture {
            id: "ai.greentic.component-chat2data-executor",
            version: "0.1.0",
            operations: &["execute", "execute_github", "execute_mcp"],
            default_config: json!({}),
            setup_config: executor_setup,
        }),
        "repo://local/component-chat2data-renderer"
        | "ai.greentic.component-chat2data-renderer" => Some(ComponentFixture {
            id: "ai.greentic.component-chat2data-renderer",
            version: "0.1.0",
            operations: &["render", "auto_select"],
            default_config: json!({}),
            setup_config: renderer_setup,
        }),
        "repo://local/component-chat2data-translator"
        | "ai.greentic.component-chat2data-translator" => Some(ComponentFixture {
            id: "ai.greentic.component-chat2data-translator",
            version: "0.1.0",
            operations: &["translate", "translate_batch"],
            default_config: json!({}),
            setup_config: translator_setup,
        }),
        "repo://local/component-chat2data-validator"
        | "ai.greentic.component-chat2data-validator" => Some(ComponentFixture {
            id: "ai.greentic.component-chat2data-validator",
            version: "0.1.0",
            operations: &["validate", "load_whitelist"],
            default_config: json!({}),
            setup_config: validator_setup,
        }),
        "repo://local/component-events2msg" | "ai.greentic.component-events2msg" => {
            Some(ComponentFixture {
                id: "ai.greentic.component-events2msg",
                version: "0.1.0",
                operations: &["route", "validate"],
                default_config: json!({}),
                setup_config: events2msg_setup,
            })
        }
        "repo://local/component-http" | "ai.greentic.component-http" => Some(ComponentFixture {
            id: "ai.greentic.component-http",
            version: "0.1.0",
            operations: &["request", "stream"],
            default_config: json!({}),
            setup_config: http_setup,
        }),
        "repo://local/component-msg2events" | "ai.greentic.component-msg2events" => {
            Some(ComponentFixture {
                id: "ai.greentic.component-msg2events",
                version: "0.1.0",
                operations: &["route", "extract", "validate"],
                default_config: json!({}),
                setup_config: msg2events_setup,
            })
        }
        "repo://local/component-pack2flow" | "ai.greentic.component-pack2flow" => {
            Some(ComponentFixture {
                id: "ai.greentic.component-pack2flow",
                version: "0.1.0",
                operations: &["handle_message"],
                default_config: json!({}),
                setup_config: passthrough,
            })
        }
        _ => None,
    }
}
