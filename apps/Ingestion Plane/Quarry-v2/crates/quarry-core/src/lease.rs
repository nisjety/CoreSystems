//! Browser lease schema. Mirrors CONTRACTS §8.

use serde::{Deserialize, Serialize};

use crate::ids::kinds;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserLease {
    pub lease_id: kinds::LeaseKind,
    pub profile_id: kinds::ProfileKind,
    pub session_affinity_key: String,
    pub proxy_affinity: ProxyAffinity,
    pub ttl_s: u32,
    pub capabilities: Vec<Capability>,
    pub artifact_bucket: String,
    /// Persist browser state back into the profile store when the session
    /// releases. False keeps the profile id run-scoped and isolated.
    #[serde(default)]
    pub persist_profile: bool,
    /// Requested browser viewport for this lease. Drivers that can only apply
    /// viewport at launch should relaunch when this differs from the active
    /// browser viewport.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewport: Option<BrowserViewport>,
    /// Tenant scope. P0 / cluster #auth+tenancy. The lease's `org_id`
    /// MUST match the requesting tenant — orchestrator activities
    /// populate this from the verified JWT claim that arrived with the
    /// originating request. Empty string means a legacy lease created
    /// before the auth refactor; downstream stores treat that as
    /// "default tenant" only in dev / test.
    #[serde(default)]
    pub org_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyAffinity {
    pub pool: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sticky_key: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    Js,
    Screenshots,
    Pdf,
    Actions,
    Downloads,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BrowserViewport {
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub device_scale_factor: f64,
    #[serde(default)]
    pub is_mobile: bool,
}
