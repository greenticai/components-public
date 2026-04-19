//! Stub — real implementation in Task 5.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[allow(dead_code)]
pub struct YgtcNode {}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct ComponentRef {
    pub oci: String,
}

#[derive(Default)]
#[allow(dead_code)]
pub struct NodeBuilder {}

impl NodeBuilder {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self::default()
    }

    #[allow(dead_code)]
    pub fn build(self) -> YgtcNode {
        YgtcNode::default()
    }
}
