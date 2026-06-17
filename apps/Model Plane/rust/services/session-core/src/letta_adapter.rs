//! Optional Letta/agent-memory adapter.
//!
//! Session Core remains the durable source of truth. This adapter enriches
//! memory with semantic recall and agent-memory backends when configured.

use mp_contracts::model_plane::v1::{
    memory_service_client::MemoryServiceClient, IndexMemoryRequest, MemoryEntry,
    SearchMemoryRequest,
};
use std::time::Duration;
use tonic::transport::{Channel, Endpoint};
use tracing::{debug, warn};

const LETTA_TIMEOUT: Duration = Duration::from_millis(900);

#[derive(Clone)]
pub(crate) struct LettaMemoryAdapter {
    client: MemoryServiceClient<Channel>,
}

impl LettaMemoryAdapter {
    pub(crate) fn from_env() -> Option<Self> {
        let endpoint = std::env::var("LETTA_MEMORY_ADDR")
            .or_else(|_| std::env::var("LETTA_MEMORY_URL"))
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())?;

        let normalized = if endpoint.contains("://") {
            endpoint
        } else {
            format!("http://{endpoint}")
        };

        match Endpoint::from_shared(normalized.clone()) {
            Ok(endpoint) => {
                debug!(addr = %normalized, "Letta memory adapter configured");
                Some(Self {
                    client: MemoryServiceClient::new(endpoint.connect_lazy()),
                })
            }
            Err(error) => {
                warn!(%error, addr = %normalized, "invalid Letta memory adapter endpoint");
                None
            }
        }
    }

    pub(crate) async fn index(
        &self,
        org_id: &str,
        thread_id: &str,
        topic: &str,
        content: &str,
    ) -> Option<String> {
        let mut client = self.client.clone();
        let request = IndexMemoryRequest {
            thread_id: thread_id.to_owned(),
            topic: topic.to_owned(),
            content: content.to_owned(),
            org_id: org_id.to_owned(),
        };

        match tokio::time::timeout(LETTA_TIMEOUT, client.index_memory(request)).await {
            Ok(Ok(response)) => Some(response.into_inner().memory_id),
            Ok(Err(error)) => {
                warn!(%error, "Letta memory index failed");
                None
            }
            Err(_) => {
                warn!("Letta memory index timed out");
                None
            }
        }
    }

    pub(crate) async fn search(
        &self,
        org_id: &str,
        thread_id: &str,
        query: &str,
        topic_filter: &[String],
        limit: u32,
    ) -> Vec<MemoryEntry> {
        let mut client = self.client.clone();
        let request = SearchMemoryRequest {
            thread_id: thread_id.to_owned(),
            query: query.to_owned(),
            topic_filter: topic_filter.to_vec(),
            limit,
            org_id: org_id.to_owned(),
            updated_after: None,
        };

        match tokio::time::timeout(LETTA_TIMEOUT, client.search_memory(request)).await {
            Ok(Ok(response)) => response.into_inner().entries,
            Ok(Err(error)) => {
                warn!(%error, "Letta memory search failed");
                Vec::new()
            }
            Err(_) => {
                warn!("Letta memory search timed out");
                Vec::new()
            }
        }
    }
}
