//! Minimal curl command parser covering the subset devs typically paste:
//! method via `-X`, URL, `-H` headers, `-d` / `--data` / `--data-raw` body.
//! Other flags are recorded in `unsupported_flags`.

use std::collections::BTreeMap;

#[derive(Debug, Clone, Default)]
pub struct ParsedCurl {
    pub method: Option<String>,
    pub url: Option<String>,
    pub headers: BTreeMap<String, String>,
    pub body: Option<String>,
    pub unsupported_flags: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum CurlParseError {
    #[error("input is empty or does not start with `curl`")]
    NotCurl,
    #[error("failed to tokenize: {0}")]
    Tokenize(String),
}

pub fn parse_curl(cmd: &str) -> Result<ParsedCurl, CurlParseError> {
    let tokens = tokenize(cmd)?;
    let mut it = tokens.into_iter();
    match it.next().as_deref() {
        Some("curl") => {}
        _ => return Err(CurlParseError::NotCurl),
    }
    let mut p = ParsedCurl::default();
    while let Some(tok) = it.next() {
        match tok.as_str() {
            "-X" | "--request" => p.method = it.next(),
            "-H" | "--header" => {
                if let Some(h) = it.next()
                    && let Some((name, val)) = h.split_once(':')
                {
                    p.headers
                        .insert(name.trim().to_string(), val.trim().to_string());
                }
            }
            "-d" | "--data" | "--data-raw" | "--data-binary" => p.body = it.next(),
            flag if flag.starts_with('-') => p.unsupported_flags.push(flag.to_string()),
            other => {
                if p.url.is_none()
                    && (other.starts_with("http://") || other.starts_with("https://"))
                {
                    p.url = Some(other.to_string());
                }
            }
        }
    }
    if p.method.is_none() {
        p.method = Some(if p.body.is_some() {
            "POST".into()
        } else {
            "GET".into()
        });
    }
    Ok(p)
}

/// Shell-ish tokenizer handling `'...'`, `"..."`, and backslash-newline line continuations.
/// Not POSIX-complete, but covers standard curl copy-pastes.
fn tokenize(input: &str) -> Result<Vec<String>, CurlParseError> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut chars = input.chars().peekable();
    let mut in_s = false;
    let mut in_d = false;
    while let Some(c) = chars.next() {
        match c {
            '\\' if matches!(chars.peek(), Some('\n')) => {
                chars.next();
            }
            '\'' if !in_d => in_s = !in_s,
            '"' if !in_s => in_d = !in_d,
            c if c.is_whitespace() && !in_s && !in_d => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if in_s || in_d {
        return Err(CurlParseError::Tokenize("unterminated quote".into()));
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    Ok(out)
}
