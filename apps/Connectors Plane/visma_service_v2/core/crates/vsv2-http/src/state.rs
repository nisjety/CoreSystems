use std::sync::Arc;
use std::time::Duration;

use sqlx::postgres::PgPool;
use vsv2_orchestrator::EndpointRegistry;

use crate::{metadata_store::MetadataStore, sync_runner::SyncRunner};

#[derive(Clone)]
pub struct AppState {
    pg: Option<PgPool>,
    require_pg_for_health: bool,
    registry: Arc<EndpointRegistry>,
    sync_runner: Option<Arc<dyn SyncRunner>>,
    metadata_store: Option<Arc<dyn MetadataStore>>,
    bearer_token: Option<Arc<str>>,
    backfill_timeout: Duration,
}

impl AppState {
    pub fn new(pg: PgPool, registry: EndpointRegistry, sync_runner: Arc<dyn SyncRunner>) -> Self {
        Self {
            pg: Some(pg),
            require_pg_for_health: true,
            registry: Arc::new(registry),
            sync_runner: Some(sync_runner),
            metadata_store: None,
            bearer_token: None,
            backfill_timeout: Duration::from_secs(300),
        }
    }

    pub fn without_pg(registry: EndpointRegistry) -> Self {
        Self {
            pg: None,
            require_pg_for_health: false,
            registry: Arc::new(registry),
            sync_runner: None,
            metadata_store: None,
            bearer_token: None,
            backfill_timeout: Duration::from_secs(300),
        }
    }

    pub fn with_sync_runner(mut self, sync_runner: Arc<dyn SyncRunner>) -> Self {
        self.sync_runner = Some(sync_runner);
        self
    }

    pub fn with_metadata_store(mut self, metadata_store: Arc<dyn MetadataStore>) -> Self {
        self.metadata_store = Some(metadata_store);
        self
    }

    pub fn with_bearer_token(mut self, bearer_token: impl Into<String>) -> Self {
        self.bearer_token = Some(Arc::<str>::from(bearer_token.into()));
        self
    }

    pub fn with_backfill_timeout(mut self, backfill_timeout: Duration) -> Self {
        self.backfill_timeout = backfill_timeout;
        self
    }

    pub fn pg(&self) -> Option<&PgPool> {
        self.pg.as_ref()
    }

    pub fn require_pg_for_health(&self) -> bool {
        self.require_pg_for_health
    }

    pub fn registry(&self) -> &EndpointRegistry {
        self.registry.as_ref()
    }

    pub fn sync_runner(&self) -> Option<&Arc<dyn SyncRunner>> {
        self.sync_runner.as_ref()
    }

    pub fn metadata_store(&self) -> Option<&Arc<dyn MetadataStore>> {
        self.metadata_store.as_ref()
    }

    pub fn bearer_token(&self) -> Option<&str> {
        self.bearer_token.as_deref()
    }

    pub fn backfill_timeout(&self) -> Duration {
        self.backfill_timeout
    }
}
