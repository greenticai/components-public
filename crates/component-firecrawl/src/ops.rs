//! One handler per operation: parse the node input, build the request body,
//! send it, map the response.
//!
//! The body builders (`input`) and response mappers (`output`) are the design
//! extension's WIT-free modules, verbatim. Only marshalling, the URL table and
//! the `{ok, …}` envelope are new — so a Firecrawl call means the same thing
//! whether an agentic worker makes it as a TOOL or a flow runs it as a NODE.
//!
//! Every failure is a VALUE. A flow routes on `ok == false`; a trap takes the
//! run down with a message no operator can act on.

use serde_json::Value;

use crate::input::{
    BrowserTaskInput, CrawlInput, CrawlResultInput, ExtractInput, ScrapeInput, ScreenshotInput,
    SearchInput,
};
use crate::transport::{HttpReq, check, resolve_secret, send};
use crate::{input, output};

const BASE: &str = "https://api.firecrawl.dev";

pub fn ok(result: Value) -> Value {
    serde_json::json!({ "ok": true, "result": result })
}

pub fn err(message: impl std::fmt::Display) -> Value {
    serde_json::json!({ "ok": false, "error": message.to_string() })
}

macro_rules! tri {
    ($e:expr) => {
        match $e {
            Ok(v) => v,
            Err(response) => return response,
        }
    };
}

/// Resolve the API key and build the headers every call carries.
///
/// The key is read per call rather than cached: a component instance may
/// outlive a credential rotation, and a stale key fails as an opaque 401.
fn headers(input: &Value) -> Result<Vec<(String, String)>, Value> {
    let raw = input
        .get("api_key")
        .and_then(Value::as_str)
        .ok_or_else(|| err("missing required field `api_key` (a value, or `secret:NAME`)"))?;
    let key = resolve_secret(raw).map_err(err)?;
    Ok(vec![
        ("content-type".to_string(), "application/json".to_string()),
        ("authorization".to_string(), format!("Bearer {key}")),
    ])
}

fn parse<T: serde::de::DeserializeOwned>(input: &Value, what: &str) -> Result<T, Value> {
    serde_json::from_value(input.clone()).map_err(|e| err(format!("invalid input for {what}: {e}")))
}

fn post(url: String, headers: Vec<(String, String)>, body: &Value) -> Result<Vec<u8>, Value> {
    let bytes = serde_json::to_vec(body).map_err(|e| err(format!("encode body: {e}")))?;
    let resp = send(HttpReq {
        method: "POST".into(),
        url,
        headers,
        body: Some(bytes),
    })
    .map_err(err)?;
    check(resp).map_err(err)
}

fn get(url: String, headers: Vec<(String, String)>) -> Result<Vec<u8>, Value> {
    let resp = send(HttpReq {
        method: "GET".into(),
        url,
        headers,
        body: None,
    })
    .map_err(err)?;
    check(resp).map_err(err)
}

/// Every scrape-family operation posts to the same endpoint and differs only in
/// the body it builds and the mapper it reads the response with.
macro_rules! scrape_family {
    ($fn_name:ident, $ty:ty, $what:literal, $build:path, $map:path, $path:literal) => {
        pub fn $fn_name(input: &Value) -> Value {
            let headers = tri!(headers(input));
            let parsed: $ty = tri!(parse(input, $what));
            let body = match $build(&parsed) {
                Ok(b) => b,
                Err(e) => return err(e),
            };
            let raw = tri!(post(format!("{BASE}{}", $path), headers, &body));
            match $map(&raw) {
                Ok(v) => ok(v),
                Err(e) => err(e),
            }
        }
    };
}

scrape_family!(
    web_scrape,
    ScrapeInput,
    "web_scrape",
    input::build_scrape_body,
    output::map_scrape_response,
    "/v2/scrape"
);
scrape_family!(
    web_extract,
    ExtractInput,
    "web_extract",
    input::build_extract_body,
    output::map_extract_response,
    "/v2/scrape"
);
scrape_family!(
    web_screenshot,
    ScreenshotInput,
    "web_screenshot",
    input::build_screenshot_body,
    output::map_screenshot_response,
    "/v2/scrape"
);
scrape_family!(
    web_search,
    SearchInput,
    "web_search",
    input::build_search_body,
    output::map_search_response,
    "/v2/search"
);
scrape_family!(
    web_crawl_start,
    CrawlInput,
    "web_crawl_start",
    input::build_crawl_body,
    output::map_crawl_start_response,
    "/v2/crawl"
);
scrape_family!(
    browser_task,
    BrowserTaskInput,
    "browser_task",
    input::build_browser_task_body,
    output::map_browser_task_response,
    "/v2/scrape"
);

/// The one GET. A blank `job_id` is rejected here rather than becoming a
/// request to `/v2/crawl/`, which Firecrawl answers with an unhelpful 404.
pub fn web_crawl_result(input: &Value) -> Value {
    let headers = tri!(headers(input));
    let parsed: CrawlResultInput = tri!(parse(input, "web_crawl_result"));
    let job_id = parsed.job_id.trim();
    if job_id.is_empty() {
        return err("job_id must not be empty");
    }
    let raw = tri!(get(format!("{BASE}/v2/crawl/{job_id}"), headers));
    match output::map_crawl_result_response(&raw) {
        Ok(v) => ok(v),
        Err(e) => err(e),
    }
}
