//! YGTc node stanza builder. Output is deterministic (sorted keys via BTreeMap).
use crate::config::ComponentConfig;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentRef {
    pub oci: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YgtcNode {
    pub node_id: String,
    pub component: String,
    pub config: ComponentConfig,
    pub inputs: BTreeMap<String, serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rationale: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mapping: Option<serde_json::Value>,
}

pub struct NodeBuilder {
    node_id: String,
    component: String,
    config: ComponentConfig,
    inputs: BTreeMap<String, serde_json::Value>,
    rationale: Option<String>,
    mapping: Option<serde_json::Value>,
}

impl NodeBuilder {
    pub fn new(node_id: impl Into<String>, component_ref: impl Into<String>) -> Self {
        Self {
            node_id: node_id.into(),
            component: component_ref.into(),
            config: ComponentConfig::default(),
            inputs: BTreeMap::new(),
            rationale: None,
            mapping: None,
        }
    }
    pub fn with_config(mut self, cfg: ComponentConfig) -> Self {
        self.config = cfg;
        self
    }
    pub fn with_input(
        mut self,
        key: impl Into<String>,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.inputs.insert(key.into(), value.into());
        self
    }
    pub fn with_rationale(mut self, r: impl Into<String>) -> Self {
        self.rationale = Some(r.into());
        self
    }
    pub fn with_mapping(mut self, m: serde_json::Value) -> Self {
        self.mapping = Some(m);
        self
    }
    pub fn build(self) -> YgtcNode {
        YgtcNode {
            node_id: self.node_id,
            component: self.component,
            config: self.config,
            inputs: self.inputs,
            rationale: self.rationale,
            mapping: self.mapping,
        }
    }
}
