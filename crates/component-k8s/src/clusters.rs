//! Per-cluster credential resolution from host secrets.
use crate::host::SecretStore;
use std::fmt;

pub struct ClusterCreds {
    pub api_url: String,
    pub token: String,
    pub allow_write: bool,
}

/// Redacting `Debug` so the bearer `token` never leaks into logs or panics.
impl fmt::Debug for ClusterCreds {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClusterCreds")
            .field("api_url", &self.api_url)
            .field("token", &"[REDACTED]")
            .field("allow_write", &self.allow_write)
            .finish()
    }
}

#[derive(Debug)]
pub enum ClusterError {
    InvalidName(String),
    MissingSecret(String),
}

impl fmt::Display for ClusterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidName(name) => write!(f, "invalid cluster name: {name}"),
            Self::MissingSecret(uri) => write!(f, "missing secret: {uri}"),
        }
    }
}

/// Cluster names map into secret URIs, so restrict them to a safe charset.
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 63
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Resolve a cluster's credentials from secrets. The returned `api_url` has any
/// trailing slash trimmed so callers can join request paths without doubling `/`.
pub fn resolve_cluster(
    store: &impl SecretStore,
    cluster: &str,
) -> Result<ClusterCreds, ClusterError> {
    if !valid_name(cluster) {
        return Err(ClusterError::InvalidName(cluster.to_string()));
    }
    let api_url = store
        .get(&format!("secret://k8s/{cluster}/api_url"))
        .or_else(|_| store.get("secret://k8s/api_url"))
        .map_err(|_| ClusterError::MissingSecret("secret://k8s/api_url".into()))?;
    let token = store
        .get(&format!("secret://k8s/{cluster}/token"))
        .or_else(|_| store.get("secret://k8s/token"))
        .map_err(|_| ClusterError::MissingSecret("secret://k8s/token".into()))?;
    let allow_write = store
        .get(&format!("secret://k8s/{cluster}/allow_write"))
        .or_else(|_| store.get("secret://k8s/allow_write"))
        .is_ok_and(|v| v.trim().eq_ignore_ascii_case("true"));

    Ok(ClusterCreds {
        api_url: api_url.trim_end_matches('/').to_string(),
        token,
        allow_write,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct MapStore(HashMap<String, String>);
    impl crate::host::SecretStore for MapStore {
        fn get(&self, uri: &str) -> Result<String, String> {
            self.0
                .get(uri)
                .cloned()
                .ok_or_else(|| format!("no secret {uri}"))
        }
    }

    fn store(pairs: &[(&str, &str)]) -> MapStore {
        MapStore(
            pairs
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect(),
        )
    }

    #[test]
    fn resolves_creds_with_write_disabled_by_default() {
        let s = store(&[
            ("secret://k8s/prod/api_url", "https://api.prod.example:6443"),
            ("secret://k8s/prod/token", "tok-123"),
        ]);
        let creds = resolve_cluster(&s, "prod").unwrap();
        assert_eq!(creds.api_url, "https://api.prod.example:6443");
        assert_eq!(creds.token, "tok-123");
        assert!(!creds.allow_write);
    }

    #[test]
    fn allow_write_true_enables_writes() {
        let s = store(&[
            ("secret://k8s/prod/api_url", "https://api.prod.example:6443"),
            ("secret://k8s/prod/token", "tok-123"),
            ("secret://k8s/prod/allow_write", "true"),
        ]);
        assert!(resolve_cluster(&s, "prod").unwrap().allow_write);
    }

    #[test]
    fn rejects_bad_cluster_name() {
        let s = store(&[]);
        assert!(matches!(
            resolve_cluster(&s, "../etc").unwrap_err(),
            ClusterError::InvalidName(_)
        ));
    }

    #[test]
    fn debug_redacts_token() {
        let s = store(&[
            ("secret://k8s/prod/api_url", "https://api.prod.example:6443"),
            ("secret://k8s/prod/token", "tok-123"),
        ]);
        let creds = resolve_cluster(&s, "prod").unwrap();
        let rendered = format!("{creds:?}");
        assert!(rendered.contains("[REDACTED]"));
        assert!(!rendered.contains("tok-123"));
    }

    #[test]
    fn missing_token_is_structured_error() {
        let s = store(&[("secret://k8s/prod/api_url", "https://api.prod.example:6443")]);
        assert!(matches!(
            resolve_cluster(&s, "prod").unwrap_err(),
            ClusterError::MissingSecret(_)
        ));
    }

    #[test]
    fn resolves_from_unscoped_default_when_per_cluster_absent() {
        let s = store(&[
            ("secret://k8s/api_url", "http://127.0.0.1:8001"),
            ("secret://k8s/token", "unscoped-tok"),
        ]);
        let creds = resolve_cluster(&s, "kind-demo").unwrap();
        assert_eq!(creds.api_url, "http://127.0.0.1:8001");
        assert_eq!(creds.token, "unscoped-tok");
        assert!(!creds.allow_write);
    }
}
