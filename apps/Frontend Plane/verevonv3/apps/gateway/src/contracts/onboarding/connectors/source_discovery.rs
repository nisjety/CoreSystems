use serde::Deserialize;

use crate::contracts::ActionActor;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SourceDiscoveryRequest {
    pub(crate) actor: Option<ActionActor>,
    #[serde(alias = "org_id")]
    pub(crate) org_id: String,
    // connector_id/label/sources remain part of the SPA wire contract but are
    // no longer forwarded: integration-core v2 resolves discovery and sync per
    // CONNECTION (looked up from org + provider), not per onboarding connector.
    #[serde(alias = "connector_id")]
    #[allow(dead_code)]
    pub(crate) connector_id: String,
    #[allow(dead_code)]
    pub(crate) label: Option<String>,
    pub(crate) provider: String,
    #[allow(dead_code)]
    pub(crate) sources: Vec<String>,
}
