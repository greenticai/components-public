//! Stub — real implementation in Task 2.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[allow(dead_code)]
pub struct ComponentConfig {}

#[derive(Debug, thiserror::Error)]
#[allow(dead_code)]
pub enum ConfigError {
    #[error("todo")]
    Todo,
}

#[allow(dead_code)]
pub fn apply_answers(
    _cfg: ComponentConfig,
    _answers: &serde_json::Value,
) -> Result<ComponentConfig, ConfigError> {
    Ok(ComponentConfig::default())
}
