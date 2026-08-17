//! Small JSON read helpers shared by tool handlers.
use serde_json::Value;

/// Read a `/`-separated path (e.g. `"status/phase"`); returns `Value::Null` if absent.
#[must_use]
pub fn at<'a>(value: &'a Value, path: &str) -> &'a Value {
    let mut current = value;
    for segment in path.split('/').filter(|s| !s.is_empty()) {
        current = &current[segment];
    }
    current
}

/// Borrow an array at `path`, or an empty slice.
pub fn array<'a>(value: &'a Value, path: &str) -> &'a [Value] {
    at(value, path).as_array().map_or(&[], Vec::as_slice)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn at_returns_nested_value() {
        let v = json!({"status": {"phase": "Running"}});
        assert_eq!(at(&v, "status/phase"), "Running");
    }

    #[test]
    fn at_returns_null_for_missing_path() {
        let v = json!({"a": 1});
        assert_eq!(at(&v, "b/c"), &Value::Null);
    }

    #[test]
    fn at_ignores_leading_and_trailing_slashes() {
        let v = json!({"status": {"phase": "Running"}});
        assert_eq!(at(&v, "/status/phase/"), "Running");
    }

    #[test]
    fn array_returns_slice_for_existing_array() {
        let v = json!({"items": [1, 2, 3]});
        assert_eq!(array(&v, "items").len(), 3);
    }

    #[test]
    fn array_returns_empty_slice_for_missing_path() {
        let v = json!({"a": 1});
        assert!(array(&v, "items").is_empty());
    }
}
