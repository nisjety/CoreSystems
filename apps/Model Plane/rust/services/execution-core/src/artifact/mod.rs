//! Artifact metadata helpers.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactDescriptor {
    pub key: String,
    pub content_type: String,
    pub size_bytes: u64,
}

#[must_use]
pub fn build_artifact_key(run_id: &str, step_id: &str) -> String {
    format!("runs/{run_id}/steps/{step_id}.json")
}
