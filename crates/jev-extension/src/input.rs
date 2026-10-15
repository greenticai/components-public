//! Pure request-shaping for the Jev `decide` tool. No WIT/network imports.
//!
//! The tool forwards the caller's `state` and `questions` to TypeSafe's
//! `POST /v1/systemone` and validates only what is certain from the published
//! docs (https://docs.typesafe.ai/introduction/quickstart): the three question
//! types and the shape of their `criteria`. Anything else is the API's to
//! judge, so its refusal is surfaced rather than second-guessed here.

use serde::Deserialize;
use serde_json::{Map, Value, json};

/// The model asked for when the caller names none.
pub const DEFAULT_MODEL: &str = "jev-latest";

#[derive(Debug, Deserialize)]
pub struct DecideInput {
    /// The data to evaluate: text or a JSON object.
    pub state: Value,
    /// Questions keyed by an id the caller chooses; answers come back under it.
    pub questions: Map<String, Value>,
    #[serde(default)]
    pub model: Option<String>,
}

fn filled(value: Option<&Value>) -> bool {
    value
        .and_then(Value::as_str)
        .is_some_and(|s| !s.trim().is_empty())
}

fn check_question(id: &str, question: &Value) -> Result<(), String> {
    let kind = question
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("question '{id}' needs a type (choice, score or noul)"))?;
    if !filled(question.get("instructions")) {
        return Err(format!("question '{id}' needs non-empty instructions"));
    }
    match kind {
        "choice" => match question.get("criteria") {
            Some(Value::Object(options)) if options.len() >= 2 => Ok(()),
            _ => Err(format!(
                "choice question '{id}' needs criteria: an object with at least two options"
            )),
        },
        "score" => match question.get("criteria") {
            Some(Value::Array(levels)) if levels.len() >= 2 => Ok(()),
            _ => Err(format!(
                "score question '{id}' needs criteria: an ordered array with at least two levels"
            )),
        },
        "noul" => Ok(()),
        other => Err(format!(
            "question '{id}' has unknown type '{other}' (use choice, score or noul)"
        )),
    }
}

/// Build the `POST /v1/systemone` body. Rejects a request the API could not
/// answer, so a malformed call fails before it spends a network round trip.
pub fn build_request_body(input: &DecideInput) -> Result<Value, String> {
    let state_empty = match &input.state {
        Value::Null => true,
        Value::String(s) => s.trim().is_empty(),
        Value::Object(o) => o.is_empty(),
        _ => false,
    };
    if state_empty {
        return Err("state must not be empty".to_string());
    }
    if input.questions.is_empty() {
        return Err("questions must contain at least one question".to_string());
    }
    for (id, question) in &input.questions {
        if id.trim().is_empty() {
            return Err("question ids must not be empty".to_string());
        }
        check_question(id, question)?;
    }
    let model = input
        .model
        .as_deref()
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .unwrap_or(DEFAULT_MODEL);
    Ok(json!({
        "state": input.state,
        "model": model,
        "questions": input.questions,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(raw: &str) -> DecideInput {
        serde_json::from_str(raw).expect("test input parses")
    }

    #[test]
    fn builds_a_mixed_request_with_the_default_model() {
        let input = parse(
            r#"{"state":"refund me","questions":{
                "intent":{"type":"choice","instructions":"Pick","criteria":{"refund":"asks for money back","other":"anything else"}},
                "urgency":{"type":"score","instructions":"How urgent","criteria":["low","mid","high"]},
                "angry":{"type":"noul","instructions":"Is the sender angry"}}}"#,
        );
        let body = build_request_body(&input).unwrap();
        assert_eq!(body["model"], DEFAULT_MODEL);
        assert_eq!(body["state"], "refund me");
        assert_eq!(body["questions"].as_object().unwrap().len(), 3);
    }

    #[test]
    fn keeps_a_structured_state_and_an_explicit_model() {
        let input = parse(
            r#"{"state":{"amount":5},"model":" jev-1.13.0 ","questions":{"q":{"type":"noul","instructions":"Is it small"}}}"#,
        );
        let body = build_request_body(&input).unwrap();
        assert_eq!(body["state"]["amount"], 5);
        assert_eq!(body["model"], "jev-1.13.0");
    }

    #[test]
    fn rejects_an_empty_state_or_no_questions() {
        assert!(
            build_request_body(&parse(
                r#"{"state":"  ","questions":{"q":{"type":"noul","instructions":"x"}}}"#
            ))
            .is_err()
        );
        assert!(build_request_body(&parse(r#"{"state":"x","questions":{}}"#)).is_err());
    }

    #[test]
    fn rejects_an_unknown_type_and_missing_instructions() {
        let unknown =
            parse(r#"{"state":"x","questions":{"q":{"type":"rank","instructions":"x"}}}"#);
        assert!(
            build_request_body(&unknown)
                .unwrap_err()
                .contains("unknown type")
        );
        let blank = parse(r#"{"state":"x","questions":{"q":{"type":"noul","instructions":" "}}}"#);
        assert!(
            build_request_body(&blank)
                .unwrap_err()
                .contains("instructions")
        );
    }

    #[test]
    fn choice_and_score_need_shaped_criteria() {
        let one_option = parse(
            r#"{"state":"x","questions":{"q":{"type":"choice","instructions":"x","criteria":{"a":"only"}}}}"#,
        );
        assert!(build_request_body(&one_option).is_err());
        let wrong_shape = parse(
            r#"{"state":"x","questions":{"q":{"type":"score","instructions":"x","criteria":{"a":"b","c":"d"}}}}"#,
        );
        assert!(build_request_body(&wrong_shape).is_err());
    }
}
