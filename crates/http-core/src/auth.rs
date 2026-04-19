//! Stub — real implementation in Task 3.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum AuthType {
    None,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct AuthHeader {
    pub name: String,
    pub value: String,
}

#[allow(dead_code)]
pub fn build_auth_header(
    _t: AuthType,
    _token: Option<&str>,
    _header: Option<&str>,
) -> Option<AuthHeader> {
    None
}
