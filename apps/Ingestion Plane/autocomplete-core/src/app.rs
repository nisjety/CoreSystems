use std::{collections::HashSet, sync::Arc};

use axum::Router;
use serde::Serialize;

use crate::{
    config::Settings,
    ingest::IngestService,
    normalization::{bounded_limit, bucket_for_org, normalize_query},
    routes,
    sonic::SonicClient,
    store::{IndexedObject, MetadataStore},
    AppResult,
};

const QUERIES_COLLECTION: &str = "queries";
const HOSTS_COLLECTION: &str = "hosts";
const TITLES_COLLECTION: &str = "titles";

#[derive(Clone)]
pub struct AppState {
    pub store: MetadataStore,
    pub sonic: SonicClient,
    pub ingest: IngestService,
    pub settings: Arc<Settings>,
}

impl AppState {
    pub async fn from_settings(settings: Settings) -> AppResult<Self> {
        let store = MetadataStore::open(&settings.metadata_db_path)?;
        let sonic = SonicClient::new(settings.sonic.clone());
        let ingest = IngestService::new(store.clone(), sonic.clone());

        Ok(Self {
            store,
            sonic,
            ingest,
            settings: Arc::new(settings),
        })
    }

    pub fn for_tests(store: MetadataStore, sonic: SonicClient, settings: Settings) -> Self {
        let ingest = IngestService::new(store.clone(), sonic.clone());
        Self {
            store,
            sonic,
            ingest,
            settings: Arc::new(settings),
        }
    }

    pub async fn suggestions(
        &self,
        org_id: &str,
        query: &str,
        scope: SuggestionScope,
        limit: Option<usize>,
    ) -> AppResult<SuggestionSet> {
        let Some(query) = normalize_query(query) else {
            return Ok(SuggestionSet {
                query: String::new(),
                scope,
                suggestions: Vec::new(),
                sonic_enabled: self.sonic.enabled(),
            });
        };

        let limit = bounded_limit(limit, 8, 20);
        let bucket = bucket_for_org(org_id);
        let mut suggestions = Vec::new();
        let mut seen = HashSet::new();

        for collection in scope.collections() {
            if suggestions.len() >= limit {
                break;
            }

            let remaining = limit - suggestions.len();
            let items = self
                .suggest_collection(collection, &bucket, &query, remaining)
                .await?;

            for item in items {
                let key = item.text.to_ascii_lowercase();
                if seen.insert(key) {
                    suggestions.push(Suggestion::from(item));
                }
                if suggestions.len() >= limit {
                    break;
                }
            }
        }

        Ok(SuggestionSet {
            query,
            scope,
            suggestions,
            sonic_enabled: self.sonic.enabled(),
        })
    }

    async fn suggest_collection(
        &self,
        collection: &'static str,
        bucket: &str,
        query: &str,
        limit: usize,
    ) -> AppResult<Vec<IndexedObject>> {
        let mut output = Vec::new();

        if self.sonic.enabled() {
            match self
                .sonic
                .query(
                    collection.to_string(),
                    bucket.to_string(),
                    query.to_string(),
                    limit.saturating_mul(2).max(limit),
                )
                .await
            {
                Ok(objects) if !objects.is_empty() => {
                    output = self
                        .store
                        .find_by_objects(collection.to_string(), bucket.to_string(), objects)
                        .await?;
                }
                Ok(_) => {}
                Err(error) => {
                    tracing::warn!(
                        error = %error,
                        collection,
                        "sonic query failed; falling back to metadata prefix scan"
                    );
                }
            }
        }

        if output.len() < limit {
            let fallback = self
                .store
                .search_prefix(
                    collection.to_string(),
                    bucket.to_string(),
                    query.to_string(),
                    limit,
                )
                .await?;
            merge_items(&mut output, fallback, limit);
        }

        Ok(output)
    }
}

pub fn build_app(state: AppState) -> Router {
    routes::router(state)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SuggestionScope {
    All,
    Queries,
    Hosts,
    Titles,
}

impl SuggestionScope {
    pub fn parse(value: Option<&str>) -> Self {
        match value.unwrap_or("all").to_ascii_lowercase().as_str() {
            "query" | "queries" => Self::Queries,
            "host" | "hosts" => Self::Hosts,
            "title" | "titles" => Self::Titles,
            _ => Self::All,
        }
    }

    fn collections(self) -> &'static [&'static str] {
        match self {
            Self::All => &[QUERIES_COLLECTION, HOSTS_COLLECTION, TITLES_COLLECTION],
            Self::Queries => &[QUERIES_COLLECTION],
            Self::Hosts => &[HOSTS_COLLECTION],
            Self::Titles => &[TITLES_COLLECTION],
        }
    }
}

#[derive(Debug, Serialize)]
pub struct SuggestionSet {
    pub query: String,
    pub scope: SuggestionScope,
    pub suggestions: Vec<Suggestion>,
    pub sonic_enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Suggestion {
    pub text: String,
    pub source: String,
    pub collection: String,
    pub object: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_url: Option<String>,
    pub metadata: serde_json::Value,
}

impl From<IndexedObject> for Suggestion {
    fn from(item: IndexedObject) -> Self {
        Self {
            text: item.text,
            source: item.source,
            collection: item.collection,
            object: item.object,
            target_url: item.target_url,
            metadata: item.metadata,
        }
    }
}

fn merge_items(output: &mut Vec<IndexedObject>, fallback: Vec<IndexedObject>, limit: usize) {
    let mut seen = output
        .iter()
        .map(|item| item.object.clone())
        .collect::<HashSet<_>>();

    for item in fallback {
        if seen.insert(item.object.clone()) {
            output.push(item);
        }
        if output.len() >= limit {
            break;
        }
    }
}
