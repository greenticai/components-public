//! Stub — real implementation in Task 6.

#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub struct ParsedCurl {}

#[derive(Debug, thiserror::Error)]
#[allow(dead_code)]
pub enum CurlParseError {
    #[error("todo")]
    Todo,
}

#[allow(dead_code)]
pub fn parse_curl(_cmd: &str) -> Result<ParsedCurl, CurlParseError> {
    Ok(ParsedCurl::default())
}
