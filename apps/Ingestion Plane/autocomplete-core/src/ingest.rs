use async_nats::jetstream::{self, consumer::PullConsumer};
use futures_util::StreamExt;
use serde_json::json;
use tokio::task::JoinHandle;

use crate::{
    config::NatsSettings,
    events::{IngestCommand, QuarryEvent},
    normalization::{bucket_for_org, normalize_host, normalize_query, stable_object},
    sonic::SonicClient,
    store::{IndexedObject, MetadataStore},
    AppError, AppResult,
};

const QUERIES_COLLECTION: &str = "queries";
const HOSTS_COLLECTION: &str = "hosts";

#[derive(Debug, Clone)]
pub struct IngestService {
    store: MetadataStore,
    sonic: SonicClient,
}

impl IngestService {
    pub fn new(store: MetadataStore, sonic: SonicClient) -> Self {
        Self { store, sonic }
    }

    pub async fn handle_event(&self, event: QuarryEvent) -> AppResult<IngestOutcome> {
        self.handle_command(event.into_command()).await
    }

    pub async fn handle_command(&self, command: IngestCommand) -> AppResult<IngestOutcome> {
        match command {
            IngestCommand::QueryIssued {
                org_id,
                user_id,
                query,
                provider,
                result_count,
            } => {
                let Some(text) = normalize_query(&query) else {
                    return Ok(IngestOutcome::Skipped);
                };
                let bucket = bucket_for_org(&org_id);
                let object = stable_object("query", &text.to_ascii_lowercase());
                let item = IndexedObject {
                    collection: QUERIES_COLLECTION.to_string(),
                    bucket,
                    object,
                    text,
                    source: "query".to_string(),
                    target_url: None,
                    metadata: json!({
                        "org_id": org_id,
                        "user_id": user_id,
                        "provider": provider,
                        "result_count": result_count,
                    }),
                };
                self.index(item).await?;
                Ok(IngestOutcome::Indexed)
            }
            IngestCommand::HostDiscovered {
                org_id,
                user_id,
                host,
                seed_url,
            } => {
                let Some(text) = normalize_host(&host) else {
                    return Ok(IngestOutcome::Skipped);
                };
                let bucket = bucket_for_org(&org_id);
                let object = format!("host:{text}");
                let item = IndexedObject {
                    collection: HOSTS_COLLECTION.to_string(),
                    bucket,
                    object,
                    text,
                    source: "host".to_string(),
                    target_url: seed_url.clone(),
                    metadata: json!({
                        "org_id": org_id,
                        "user_id": user_id,
                        "seed_url": seed_url,
                    }),
                };
                self.index(item).await?;
                Ok(IngestOutcome::Indexed)
            }
            IngestCommand::Unsupported => Ok(IngestOutcome::Skipped),
        }
    }

    pub async fn index_manual(&self, item: IndexedObject) -> AppResult<()> {
        self.index(item).await
    }

    async fn index(&self, item: IndexedObject) -> AppResult<()> {
        self.store.upsert(item.clone()).await?;
        if self.sonic.enabled() {
            if let Err(error) = self
                .sonic
                .push(
                    item.collection.clone(),
                    item.bucket.clone(),
                    item.object.clone(),
                    item.text.clone(),
                )
                .await
            {
                tracing::warn!(
                    error = %error,
                    collection = %item.collection,
                    bucket = %item.bucket,
                    object = %item.object,
                    "metadata indexed but sonic push failed"
                );
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IngestOutcome {
    Indexed,
    Skipped,
}

pub fn spawn_nats_consumer(config: NatsSettings, ingest: IngestService) -> JoinHandle<()> {
    tokio::spawn(async move {
        if let Err(error) = run_nats_consumer(config, ingest).await {
            tracing::error!(error = %error, "autocomplete-core nats consumer stopped");
        }
    })
}

async fn run_nats_consumer(config: NatsSettings, ingest: IngestService) -> AppResult<()> {
    let mut options = async_nats::ConnectOptions::new();
    if let Some(token) = &config.auth_token {
        options = options.token(token.clone());
    }

    let client = options
        .connect(&config.url)
        .await
        .map_err(|error| AppError::Nats(format!("connect failed: {error}")))?;
    let jetstream = jetstream::new(client);
    let stream = jetstream
        .get_stream(&config.stream_name)
        .await
        .map_err(|error| AppError::Nats(format!("get stream failed: {error}")))?;

    let consumer: PullConsumer = stream
        .get_or_create_consumer(
            &config.durable_name,
            jetstream::consumer::pull::Config {
                durable_name: Some(config.durable_name.clone()),
                filter_subject: config.subject_filter.clone(),
                ..Default::default()
            },
        )
        .await
        .map_err(|error| AppError::Nats(format!("create consumer failed: {error}")))?;

    tracing::info!(
        stream = %config.stream_name,
        durable = %config.durable_name,
        filter = %config.subject_filter,
        "autocomplete-core consuming quarry events"
    );

    let mut messages = consumer
        .messages()
        .await
        .map_err(|error| AppError::Nats(format!("consumer messages failed: {error}")))?;

    while let Some(message) = messages.next().await {
        let message = message.map_err(|error| AppError::Nats(format!("message error: {error}")))?;
        match serde_json::from_slice::<QuarryEvent>(&message.payload) {
            Ok(event) => {
                match ingest.handle_event(event).await {
                    Ok(outcome) => {
                        tracing::debug!(?outcome, "processed quarry event");
                    }
                    Err(error) => {
                        tracing::warn!(error = %error, "quarry event ingest failed");
                    }
                }
                let _ = message.ack().await;
            }
            Err(error) => {
                tracing::warn!(
                    error = %error,
                    payload_bytes = message.payload.len(),
                    "bad quarry event payload; acking to avoid poison-loop"
                );
                let _ = message.ack().await;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{config::SonicSettings, events::IngestCommand, store::MetadataStore};

    use super::*;

    #[tokio::test]
    async fn indexes_search_issued_into_metadata() {
        let store = MetadataStore::in_memory().unwrap();
        let sonic = SonicClient::new(SonicSettings {
            enabled: false,
            addr: "127.0.0.1:1491".to_string(),
            password: String::new(),
            timeout: std::time::Duration::from_secs(1),
        });
        let ingest = IngestService::new(store.clone(), sonic);

        ingest
            .handle_command(IngestCommand::QueryIssued {
                org_id: "org_a".to_string(),
                user_id: Some("user_a".to_string()),
                query: "Find me spa".to_string(),
                provider: Some("brave".to_string()),
                result_count: Some(3),
            })
            .await
            .unwrap();

        let found = store
            .search_prefix(
                QUERIES_COLLECTION.to_string(),
                bucket_for_org("org_a"),
                "Find".to_string(),
                10,
            )
            .await
            .unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].text, "Find me spa");
    }
}
