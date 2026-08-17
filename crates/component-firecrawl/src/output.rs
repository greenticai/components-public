//! Pure response-mapping logic for the Firecrawl web tools. Parses the Firecrawl
//! `{success,data,error}` envelope and returns each tool's stable output JSON.
//! Permissive: unknown fields ignored; missing optional fields become null.

// Copied verbatim from the design extension. The only edit is this attribute:
// the tool-metadata tables and several input structs exist for the TOOL surface
// and are unused by the node surface, and `HttpReq`'s fields are read only on
// the wasm target. Silencing it here keeps the rest of the file diffable
// against its source.
#![allow(dead_code)]
use serde::Deserialize;
use serde_json::{Value, json};

/// Firecrawl's standard response envelope.
#[derive(Debug, Deserialize)]
struct Envelope {
    #[serde(default)]
    #[allow(dead_code)]
    success: bool,
    #[serde(default)]
    data: Option<Value>,
    #[serde(default)]
    error: Option<String>,
    // /v2/crawl start returns a top-level `id`; keep it accessible.
    #[serde(default)]
    id: Option<String>,
    // /v2/crawl/{id} returns status/completed/total at the top level.
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    completed: Option<u64>,
    #[serde(default)]
    total: Option<u64>,
}

fn parse_envelope(body: &[u8], ctx: &str) -> Result<Envelope, String> {
    serde_json::from_slice::<Envelope>(body)
        .map_err(|error| format!("decode firecrawl {ctx} response: {error}"))
}

fn require_data(env: Envelope, ctx: &str) -> Result<Value, String> {
    if let Some(data) = env.data {
        return Ok(data);
    }
    Err(env
        .error
        .unwrap_or_else(|| format!("firecrawl {ctx} returned no data")))
}

/// `web_scrape`: return `{ markdown, html, links, metadata }` from `data`.
pub fn map_scrape_response(body: &[u8]) -> Result<Value, String> {
    let data = require_data(parse_envelope(body, "scrape")?, "scrape")?;
    Ok(json!({
        "markdown": data.get("markdown").cloned().unwrap_or(Value::Null),
        "html": data.get("html").cloned().unwrap_or(Value::Null),
        "links": data.get("links").cloned().unwrap_or(Value::Null),
        "metadata": data.get("metadata").cloned().unwrap_or(Value::Null)
    }))
}

/// `web_extract`: return `{ json }` from `data.json`.
pub fn map_extract_response(body: &[u8]) -> Result<Value, String> {
    let data = require_data(parse_envelope(body, "extract")?, "extract")?;
    let extracted = data.get("json").cloned().unwrap_or(Value::Null);
    Ok(json!({ "json": extracted }))
}

/// `web_screenshot`: return `{ screenshot_url }` from `data.screenshot`.
pub fn map_screenshot_response(body: &[u8]) -> Result<Value, String> {
    let data = require_data(parse_envelope(body, "screenshot")?, "screenshot")?;
    let url = data
        .get("screenshot")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if url.is_empty() {
        return Err("firecrawl screenshot response had no screenshot url".to_string());
    }
    Ok(json!({ "screenshot_url": url }))
}

/// `web_search`: normalize `data.web[]` (or a bare `data[]`) into `{ results }`.
pub fn map_search_response(body: &[u8]) -> Result<Value, String> {
    let data = require_data(parse_envelope(body, "search")?, "search")?;
    let items = if let Some(web) = data.get("web").and_then(Value::as_array) {
        web.clone()
    } else if let Some(arr) = data.as_array() {
        arr.clone()
    } else {
        Vec::new()
    };
    let results: Vec<Value> = items
        .iter()
        .map(|item| {
            json!({
                "url": item.get("url").cloned().unwrap_or(Value::Null),
                "title": item.get("title").cloned().unwrap_or(Value::Null),
                "description": item
                    .get("description")
                    .or_else(|| item.get("markdown"))
                    .cloned()
                    .unwrap_or(Value::Null)
            })
        })
        .collect();
    Ok(json!({ "results": results }))
}

/// `web_crawl_start` (start): return `{ job_id, status: "started" }`.
pub fn map_crawl_start_response(body: &[u8]) -> Result<Value, String> {
    let env = parse_envelope(body, "crawl")?;
    let job_id = env
        .id
        .clone()
        .or_else(|| {
            env.data
                .as_ref()
                .and_then(|d| d.get("id"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .ok_or_else(|| {
            env.error
                .clone()
                .unwrap_or_else(|| "firecrawl crawl start returned no job id".to_string())
        })?;
    Ok(json!({ "job_id": job_id, "status": "started" }))
}

/// `web_crawl_result`: return `{ status, completed, total, data }`.
pub fn map_crawl_result_response(body: &[u8]) -> Result<Value, String> {
    let env = parse_envelope(body, "crawl_result")?;
    let status = env.status.clone().ok_or_else(|| {
        env.error
            .clone()
            .unwrap_or_else(|| "firecrawl crawl_result missing status".to_string())
    })?;
    Ok(json!({
        "status": status,
        "completed": env.completed.unwrap_or(0),
        "total": env.total.unwrap_or(0),
        "data": env.data.unwrap_or(Value::Null)
    }))
}

/// `browser_task`: return the final page content from `data`.
pub fn map_browser_task_response(body: &[u8]) -> Result<Value, String> {
    let data = require_data(parse_envelope(body, "browser_task")?, "browser_task")?;
    Ok(json!({
        "markdown": data.get("markdown").cloned().unwrap_or(Value::Null),
        "html": data.get("html").cloned().unwrap_or(Value::Null),
        "screenshot": data.get("screenshot").cloned().unwrap_or(Value::Null),
        "metadata": data.get("metadata").cloned().unwrap_or(Value::Null)
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrape_maps_markdown_and_links() {
        let raw = br#"{"success":true,"data":{"markdown":"heading","links":["https://a"],"metadata":{"title":"Test"}}}"#;
        let out = map_scrape_response(raw).unwrap();
        assert_eq!(out["markdown"], "heading");
        assert_eq!(out["links"], json!(["https://a"]));
        assert_eq!(out["html"], Value::Null);
    }

    #[test]
    fn scrape_surfaces_firecrawl_error() {
        let raw = br#"{"success":false,"error":"blocked"}"#;
        assert_eq!(map_scrape_response(raw).unwrap_err(), "blocked");
    }

    #[test]
    fn extract_pulls_json_object() {
        let raw = br#"{"success":true,"data":{"json":{"price":9.99}}}"#;
        let out = map_extract_response(raw).unwrap();
        assert_eq!(out["json"]["price"], 9.99);
    }

    #[test]
    fn screenshot_requires_url() {
        let ok = br#"{"success":true,"data":{"screenshot":"https://img/1.png"}}"#;
        assert_eq!(
            map_screenshot_response(ok).unwrap()["screenshot_url"],
            "https://img/1.png"
        );
        let bad = br#"{"success":true,"data":{}}"#;
        assert!(map_screenshot_response(bad).is_err());
    }

    #[test]
    fn search_normalizes_web_array() {
        let raw = br#"{"success":true,"data":{"web":[{"url":"https://a","title":"A","description":"d"}]}}"#;
        let out = map_search_response(raw).unwrap();
        assert_eq!(out["results"][0]["url"], "https://a");
        assert_eq!(out["results"][0]["description"], "d");
    }

    #[test]
    fn search_normalizes_bare_array() {
        let raw = br#"{"success":true,"data":[{"url":"https://a","title":"A"}]}"#;
        let out = map_search_response(raw).unwrap();
        assert_eq!(out["results"][0]["url"], "https://a");
    }

    #[test]
    fn crawl_start_reads_top_level_id() {
        let raw = br#"{"success":true,"id":"job-1","url":"https://x"}"#;
        let out = map_crawl_start_response(raw).unwrap();
        assert_eq!(out["job_id"], "job-1");
        assert_eq!(out["status"], "started");
    }

    #[test]
    fn crawl_result_reads_status_and_counts() {
        let raw = br#"{"status":"scraping","completed":3,"total":10,"data":[{"markdown":"p"}]}"#;
        let out = map_crawl_result_response(raw).unwrap();
        assert_eq!(out["status"], "scraping");
        assert_eq!(out["completed"], 3);
        assert_eq!(out["total"], 10);
        assert_eq!(out["data"][0]["markdown"], "p");
    }

    #[test]
    fn browser_task_maps_final_content() {
        let raw =
            br#"{"success":true,"data":{"markdown":"done","screenshot":"https://img/x.png"}}"#;
        let out = map_browser_task_response(raw).unwrap();
        assert_eq!(out["markdown"], "done");
        assert_eq!(out["screenshot"], "https://img/x.png");
    }

    #[test]
    fn malformed_json_is_err() {
        assert!(map_scrape_response(b"{bad").is_err());
    }

    #[test]
    fn crawl_start_falls_back_to_data_id() {
        let raw = br#"{"success":true,"data":{"id":"job-42"}}"#;
        let out = map_crawl_start_response(raw).unwrap();
        assert_eq!(out["job_id"], "job-42");
        assert_eq!(out["status"], "started");
    }

    #[test]
    fn crawl_result_missing_status_is_err() {
        let raw = br#"{"error":"rate limited"}"#;
        assert!(map_crawl_result_response(raw).is_err());
    }

    #[test]
    fn search_description_falls_back_to_markdown() {
        let raw = br#"{"success":true,"data":{"web":[{"url":"https://a","title":"A","markdown":"body text"}]}}"#;
        let out = map_search_response(raw).unwrap();
        assert_eq!(out["results"][0]["description"], "body text");
    }
}
