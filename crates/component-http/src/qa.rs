use greentic_types::i18n_text::I18nText as CanonicalI18nText;
use greentic_types::schemas::component::v0_6_0::{
    ComponentQaSpec, QaMode as CanonicalQaMode, Question as CanonicalQuestion,
    QuestionKind as CanonicalQuestionKind,
};
use http_core::config::DEFAULT_TIMEOUT_MS;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

// ============================================================================
// Internal types for QA spec builder
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct I18nText {
    pub key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct QaQuestionSpec {
    id: String,
    label: I18nText,
    help: Option<I18nText>,
    error: Option<I18nText>,
    kind: String,
    required: bool,
    default: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct QaSpec {
    mode: String,
    title: I18nText,
    description: Option<I18nText>,
    questions: Vec<QaQuestionSpec>,
    defaults: Value,
}

// ============================================================================
// Public QA spec builder
// ============================================================================

pub fn canonical_qa_spec(mode: &str) -> ComponentQaSpec {
    qa_spec_to_canonical(&build_qa_spec_json(mode))
}

fn build_qa_spec_json(mode: &str) -> QaSpec {
    match mode {
        "default" => QaSpec {
            mode: "default".to_string(),
            title: i18n("http.qa.default.title"),
            description: Some(i18n("http.qa.default.description")),
            questions: Vec::new(),
            defaults: json!({}),
        },
        "setup" => QaSpec {
            mode: "setup".to_string(),
            title: i18n("http.qa.setup.title"),
            description: Some(i18n("http.qa.setup.description")),
            questions: vec![
                qa_q("base_url", "http.qa.setup.base_url", false),
                qa_q("auth_type", "http.qa.setup.auth_type", false),
                qa_q("auth_token", "http.qa.setup.auth_token", false),
                QaQuestionSpec {
                    id: "timeout_ms".to_string(),
                    label: i18n("http.qa.setup.timeout_ms"),
                    help: None,
                    error: None,
                    kind: "number".to_string(),
                    required: false,
                    default: None,
                },
                qa_q("default_headers", "http.qa.setup.default_headers", false),
            ],
            defaults: json!({
                "auth_type": "bearer",
                "timeout_ms": DEFAULT_TIMEOUT_MS,
            }),
        },
        "update" => QaSpec {
            mode: "update".to_string(),
            title: i18n("http.qa.update.title"),
            description: Some(i18n("http.qa.update.description")),
            questions: Vec::new(),
            defaults: json!({}),
        },
        "remove" => QaSpec {
            mode: "remove".to_string(),
            title: i18n("http.qa.remove.title"),
            description: Some(i18n("http.qa.remove.description")),
            questions: Vec::new(),
            defaults: json!({}),
        },
        _ => QaSpec {
            mode: mode.to_string(),
            title: i18n("http.qa.default.title"),
            description: Some(i18n("http.qa.default.description")),
            questions: Vec::new(),
            defaults: json!({}),
        },
    }
}

fn qa_spec_to_canonical(spec: &QaSpec) -> ComponentQaSpec {
    ComponentQaSpec {
        mode: qa_mode_to_canonical(&spec.mode),
        title: i18n_to_canonical(&spec.title),
        description: spec.description.as_ref().map(i18n_to_canonical),
        questions: spec
            .questions
            .iter()
            .map(qa_question_to_canonical)
            .collect(),
        defaults: serde_json::from_value(spec.defaults.clone()).unwrap_or_default(),
    }
}

fn qa_mode_to_canonical(mode: &str) -> CanonicalQaMode {
    match mode {
        "setup" => CanonicalQaMode::Setup,
        "update" => CanonicalQaMode::Update,
        "remove" => CanonicalQaMode::Remove,
        _ => CanonicalQaMode::Default,
    }
}

fn qa_question_to_canonical(question: &QaQuestionSpec) -> CanonicalQuestion {
    CanonicalQuestion {
        id: question.id.clone(),
        label: i18n_to_canonical(&question.label),
        help: question.help.as_ref().map(i18n_to_canonical),
        error: question.error.as_ref().map(i18n_to_canonical),
        kind: qa_kind_to_canonical(&question.kind),
        required: question.required,
        default: question
            .default
            .clone()
            .and_then(|value| serde_json::from_value(value).ok()),
        skip_if: None,
    }
}

fn qa_kind_to_canonical(kind: &str) -> CanonicalQuestionKind {
    match kind {
        "number" | "int" | "integer" | "float" => CanonicalQuestionKind::Number,
        "bool" | "boolean" => CanonicalQuestionKind::Bool,
        _ => CanonicalQuestionKind::Text,
    }
}

fn i18n_to_canonical(text: &I18nText) -> CanonicalI18nText {
    CanonicalI18nText::new(text.key.clone(), None)
}

fn i18n(key: &str) -> I18nText {
    I18nText {
        key: key.to_string(),
    }
}

fn qa_q(key: &str, text: &str, required: bool) -> QaQuestionSpec {
    QaQuestionSpec {
        id: key.to_string(),
        label: i18n(text),
        help: None,
        error: None,
        kind: "text".to_string(),
        required,
        default: None,
    }
}
