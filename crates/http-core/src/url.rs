//! Stub — real implementation in Task 4.

#[derive(Debug, thiserror::Error)]
#[allow(dead_code)]
pub enum UrlError {
    #[error("todo")]
    Todo,
}

#[allow(dead_code)]
pub fn validate_url(_u: &str) -> Result<(), UrlError> {
    Ok(())
}

#[allow(dead_code)]
pub fn extract_placeholders(_u: &str) -> Vec<String> {
    Vec::new()
}
