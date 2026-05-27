use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use component_sorx_business::{
    SorxHttpError, SorxRequest, SorxResponse, execute_operation_with_sender,
};
use serde_json::Value;

fn main() {
    let mut args = std::env::args().skip(1);
    let operation = args
        .next()
        .unwrap_or_else(|| "invoke_locked_action".to_string());
    let mut input = String::new();
    if let Err(error) = std::io::stdin().read_to_string(&mut input) {
        print_error("invalid_input", format!("failed to read stdin: {error}"));
        std::process::exit(1);
    }
    let payload: Value = match serde_json::from_str(input.trim().if_empty("{}")) {
        Ok(value) => value,
        Err(error) => {
            print_error("invalid_input", format!("invalid JSON input: {error}"));
            std::process::exit(1);
        }
    };
    let output = execute_operation_with_sender(&operation, &payload, send_http_native);
    println!(
        "{}",
        serde_json::to_string_pretty(&output).unwrap_or_else(|_| output.to_string())
    );
}

trait IfEmpty {
    fn if_empty<'a>(&'a self, fallback: &'a str) -> &'a str;
}

impl IfEmpty for str {
    fn if_empty<'a>(&'a self, fallback: &'a str) -> &'a str {
        if self.is_empty() { fallback } else { self }
    }
}

fn print_error(code: &str, message: String) {
    let output = serde_json::json!({
        "ok": false,
        "error": {
            "code": code,
            "message": message,
        }
    });
    println!("{}", serde_json::to_string_pretty(&output).unwrap());
}

fn send_http_native(request: &SorxRequest) -> Result<SorxResponse, SorxHttpError> {
    let parsed = ParsedUrl::parse(&request.url)?;
    if parsed.scheme != "http" {
        return Err(SorxHttpError {
            code: "unsupported_url_scheme".to_string(),
            message: "native test CLI supports http:// Sorx URLs".to_string(),
        });
    }
    let address = format!("{}:{}", parsed.host, parsed.port);
    let mut addrs = address.to_socket_addrs().map_err(|error| SorxHttpError {
        code: "connection_failed".to_string(),
        message: format!("failed to resolve {address}: {error}"),
    })?;
    let addr = addrs.next().ok_or_else(|| SorxHttpError {
        code: "connection_failed".to_string(),
        message: format!("no address resolved for {address}"),
    })?;
    let timeout = Duration::from_millis(u64::from(request.timeout_ms.max(1)));
    let mut stream = TcpStream::connect_timeout(&addr, timeout).map_err(|error| SorxHttpError {
        code: "connection_failed".to_string(),
        message: format!("failed to connect to {address}: {error}"),
    })?;
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));

    let body = request.body.as_deref().unwrap_or_default();
    let mut wire = Vec::new();
    write!(
        wire,
        "{} {} HTTP/1.1\r\nHost: {}\r\nAccept: application/json\r\nConnection: close\r\nContent-Length: {}\r\n",
        request.method, parsed.path_and_query, parsed.host_header, body.len()
    )
    .map_err(write_error)?;
    for (name, value) in &request.headers {
        write!(wire, "{name}: {value}\r\n").map_err(write_error)?;
    }
    wire.extend_from_slice(b"\r\n");
    wire.extend_from_slice(body);
    stream.write_all(&wire).map_err(|error| SorxHttpError {
        code: "request_failed".to_string(),
        message: format!("failed to write request: {error}"),
    })?;

    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .map_err(|error| SorxHttpError {
            code: "request_failed".to_string(),
            message: format!("failed to read response: {error}"),
        })?;
    parse_http_response(&response)
}

fn write_error(error: std::io::Error) -> SorxHttpError {
    SorxHttpError {
        code: "request_failed".to_string(),
        message: format!("failed to encode request: {error}"),
    }
}

fn parse_http_response(response: &[u8]) -> Result<SorxResponse, SorxHttpError> {
    let Some(split) = response.windows(4).position(|window| window == b"\r\n\r\n") else {
        return Err(SorxHttpError {
            code: "invalid_response".to_string(),
            message: "HTTP response did not contain headers".to_string(),
        });
    };
    let (head, body_with_sep) = response.split_at(split);
    let body = body_with_sep[4..].to_vec();
    let head_text = String::from_utf8_lossy(head);
    let mut lines = head_text.lines();
    let status_line = lines.next().unwrap_or_default();
    let status = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or_else(|| SorxHttpError {
            code: "invalid_response".to_string(),
            message: format!("invalid HTTP status line: {status_line}"),
        })?;
    let headers = lines
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            Some((name.trim().to_string(), value.trim().to_string()))
        })
        .collect();
    Ok(SorxResponse {
        status,
        headers,
        body,
    })
}

#[derive(Debug)]
struct ParsedUrl {
    scheme: String,
    host: String,
    host_header: String,
    port: u16,
    path_and_query: String,
}

impl ParsedUrl {
    fn parse(url: &str) -> Result<Self, SorxHttpError> {
        let (scheme, rest) = url.split_once("://").ok_or_else(|| SorxHttpError {
            code: "invalid_url".to_string(),
            message: format!("invalid URL: {url}"),
        })?;
        let (authority, path) = match rest.split_once('/') {
            Some((authority, path)) => (authority, format!("/{path}")),
            None => (rest, "/".to_string()),
        };
        let (host, port) = match authority.rsplit_once(':') {
            Some((host, port)) if port.chars().all(|ch| ch.is_ascii_digit()) => {
                (host.to_string(), port.parse().unwrap_or(80))
            }
            _ => (authority.to_string(), 80),
        };
        if host.is_empty() {
            return Err(SorxHttpError {
                code: "invalid_url".to_string(),
                message: format!("URL is missing host: {url}"),
            });
        }
        Ok(Self {
            scheme: scheme.to_string(),
            host: host.clone(),
            host_header: authority.to_string(),
            port,
            path_and_query: path,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn parses_http_urls_with_default_and_explicit_ports() {
        let default_port = ParsedUrl::parse("http://127.0.0.1/v1/sorx/tools").unwrap();
        assert_eq!(default_port.scheme, "http");
        assert_eq!(default_port.host, "127.0.0.1");
        assert_eq!(default_port.host_header, "127.0.0.1");
        assert_eq!(default_port.port, 80);
        assert_eq!(default_port.path_and_query, "/v1/sorx/tools");

        let explicit_port =
            ParsedUrl::parse("http://localhost:8787/v1/sorx/metrics/foo/query").unwrap();
        assert_eq!(explicit_port.host, "localhost");
        assert_eq!(explicit_port.host_header, "localhost:8787");
        assert_eq!(explicit_port.port, 8787);
        assert_eq!(explicit_port.path_and_query, "/v1/sorx/metrics/foo/query");
    }

    #[test]
    fn rejects_invalid_urls() {
        let error = ParsedUrl::parse("127.0.0.1:8787/v1/sorx/tools").unwrap_err();
        assert_eq!(error.code, "invalid_url");

        let error = ParsedUrl::parse("http:///v1/sorx/tools").unwrap_err();
        assert_eq!(error.code, "invalid_url");
    }

    #[test]
    fn parses_http_response_status_headers_and_body() {
        let response = parse_http_response(
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nX-Test: yes\r\n\r\n{\"ok\":true}",
        )
        .unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(
            response.headers,
            vec![
                ("Content-Type".to_string(), "application/json".to_string()),
                ("X-Test".to_string(), "yes".to_string()),
            ]
        );
        assert_eq!(response.body, br#"{"ok":true}"#);
    }

    #[test]
    fn rejects_malformed_http_responses() {
        let error = parse_http_response(b"HTTP/1.1 200 OK").unwrap_err();
        assert_eq!(error.code, "invalid_response");

        let error = parse_http_response(b"not-http\r\n\r\nbody").unwrap_err();
        assert_eq!(error.code, "invalid_response");
    }

    #[test]
    fn native_sender_round_trips_local_http() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 1024];
            let read = stream.read(&mut request).unwrap();
            let request = String::from_utf8_lossy(&request[..read]);
            assert!(request.starts_with("POST /v1/sorx/tools HTTP/1.1"));
            assert!(request.contains("Content-Type: application/json"));
            assert!(request.ends_with(r#"{"hello":"world"}"#));
            stream
                .write_all(
                    b"HTTP/1.1 201 Created\r\nContent-Type: application/json\r\n\r\n{\"ok\":true}",
                )
                .unwrap();
        });

        let response = send_http_native(&SorxRequest {
            method: "POST".to_string(),
            url: format!("http://{addr}/v1/sorx/tools"),
            headers: vec![("Content-Type".to_string(), "application/json".to_string())],
            body: Some(br#"{"hello":"world"}"#.to_vec()),
            timeout_ms: 1_000,
            strict_tls: true,
        })
        .unwrap();
        server.join().unwrap();

        assert_eq!(response.status, 201);
        assert_eq!(response.body, br#"{"ok":true}"#);
    }

    #[test]
    fn native_sender_rejects_https_for_test_cli() {
        let error = send_http_native(&SorxRequest {
            method: "GET".to_string(),
            url: "https://example.test/v1/sorx/tools".to_string(),
            headers: Vec::new(),
            body: None,
            timeout_ms: 1_000,
            strict_tls: true,
        })
        .unwrap_err();
        assert_eq!(error.code, "unsupported_url_scheme");
    }
}
