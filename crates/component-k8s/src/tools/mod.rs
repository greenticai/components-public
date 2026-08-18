//! The tool modules `greentic.k8s` ships, copied from the extension.
//!
//! `remediate` carries only the EIGHT mutators that can be authorised without a
//! human present. `drain_node` and `delete_resource` are deliberately absent —
//! their own author judged them destructive enough to require `confirm: true`,
//! and a flow cannot supply one: a `confirm` typed into node config is a
//! constant the flow author wrote, and authorises nothing at the moment the
//! step runs.

pub mod diagnose;
pub mod observe;
pub mod remediate;
