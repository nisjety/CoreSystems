use async_nats::Client as NatsClient;
use futures::StreamExt;
use serde::Deserialize;

use super::CacheLayer;

const SUBJECT_DOC_CREATED: &str = "dataplane.documents.created";
const SUBJECT_DOC_DELETED: &str = "dataplane.documents.deleted";
const SUBJECT_DOC_UPDATED: &str = "dataplane.documents.updated";

#[derive(Deserialize)]
struct DocEvent {
    org_id: String,
}

/// Spawns a background task that listens for document mutation events on NATS
/// and invalidates the retrieval-result cache for the affected org_id.
///
/// Embedding cache (keyed by query text) is intentionally NOT invalidated — it
/// is independent of document state.
pub fn spawn_invalidator(nats: NatsClient, cache: CacheLayer) {
    tokio::spawn(async move {
        let subjects = [
            SUBJECT_DOC_CREATED,
            SUBJECT_DOC_DELETED,
            SUBJECT_DOC_UPDATED,
        ];
        for subject in subjects {
            let cache = cache.clone();
            let nats = nats.clone();
            tokio::spawn(async move {
                let mut sub = match nats.subscribe(subject).await {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::warn!(?e, subject, "cache invalidator subscribe failed");
                        return;
                    }
                };
                tracing::info!(subject, "cache invalidator subscribed");

                while let Some(msg) = sub.next().await {
                    let evt: DocEvent = match serde_json::from_slice(&msg.payload) {
                        Ok(e) => e,
                        Err(e) => {
                            tracing::warn!(?e, "cache invalidator decode failed");
                            continue;
                        }
                    };
                    cache.invalidate_org_retrieval(&evt.org_id).await;
                }
            });
        }
    });
}
