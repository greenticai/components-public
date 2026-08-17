//! Output types returned by the JSON transform tools.

use serde::Serialize;

/// Result of a `json_validate` tool call.
///
/// `valid` is `true` when `data` conforms to `schema`; `errors` is populated
/// with one human-readable message per validation error when `valid` is `false`.
#[derive(Debug, PartialEq, Serialize)]
pub struct ValidateResult {
    /// `true` when the data document passes schema validation.
    pub valid: bool,
    /// Validation error messages (empty when `valid` is `true`).
    pub errors: Vec<String>,
}

/// Result of a `jsonpath_query` tool call.
///
/// `matches` holds the JSON values selected by the path expression (owned
/// clones); `count` equals `matches.len()`.
#[derive(Debug, PartialEq, Serialize)]
#[allow(dead_code)]
pub struct QueryResult {
    /// Values selected by the JSONPath expression.
    pub matches: Vec<serde_json::Value>,
    /// Number of selected values (== `matches.len()`).
    pub count: usize,
}

/// Result of a `json_merge_patch` or `json_reshape` tool call.
///
/// `result` holds the output JSON value produced by the operation.
#[derive(Debug, PartialEq, Serialize)]
#[allow(dead_code)]
pub struct JsonResult {
    /// The output JSON value from the operation.
    pub result: serde_json::Value,
}

/// Result of a `json_dedup` tool call.
///
/// `result` holds the de-duplicated array (first-occurrence order preserved).
/// `removed` is the number of duplicate elements that were dropped.
#[derive(Debug, PartialEq, Serialize)]
pub struct DedupResult {
    /// The de-duplicated array of JSON values.
    pub result: Vec<serde_json::Value>,
    /// Number of elements removed (original length − result length).
    pub removed: usize,
}

/// Result of a `json_diff` tool call.
///
/// `patch` is the RFC 6902 JSON Patch (an array of op objects) that transforms
/// `from` into `to`. `changed` is `true` when the patch is non-empty.
#[derive(Debug, PartialEq, Serialize)]
pub struct DiffResult {
    /// RFC 6902 JSON Patch: an array of op objects (`{"op","path","value?"}`).
    /// Empty array (`[]`) when `from` and `to` are identical.
    pub patch: serde_json::Value,
    /// `true` when `from` and `to` differ (i.e. `patch` is non-empty).
    pub changed: bool,
}

/// Result of a `json_sort` tool call.
///
/// `sorted` is the sorted JSON array. `count` equals `sorted.as_array().len()`.
#[derive(Debug, PartialEq, Serialize)]
pub struct SortResult {
    /// The sorted JSON array.
    pub sorted: serde_json::Value,
    /// Number of elements in the sorted array (== original length).
    pub count: usize,
}
