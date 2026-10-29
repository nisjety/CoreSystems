use vsv2_auth::{OAuthClient, Scope};
use vsv2_common::{config::AppConfig, Source};
use vsv2_datamart::DatamartClient;
use vsv2_erp_rest::ErpRestClient;
use vsv2_expense::ExpenseClient;
use vsv2_orchestrator::{
    BqWriter, DatamartAdapter, DispatchTable, EndpointRegistry, ErpRestAdapter, ExpenseAdapter,
    NoopAdapter, SyncExecutor,
};
use vsv2_ratelimit::Limiter;
use vsv2_state::StateStore;

fn build_oauth(cfg: &AppConfig) -> OAuthClient {
    let client = OAuthClient::new(
        &cfg.oauth_token_url,
        &cfg.oauth_client_id,
        &cfg.oauth_client_secret,
    )
    .with_scope(Scope::Erp, cfg.oauth_scope_erp.clone())
    .with_scope(Scope::Datamart, cfg.oauth_scope_datamart.clone())
    .with_scope(Scope::SalesOrder, cfg.oauth_scope_salesorder.clone());

    if let Some(scope) = cfg.oauth_scope_expense.clone() {
        client.with_scope(Scope::Expense, scope)
    } else {
        client
    }
}

fn build_datamart(cfg: &AppConfig, redis: Option<deadpool_redis::Pool>) -> DatamartClient {
    let oauth = build_oauth(cfg);
    let mut limiter = Limiter::new(
        Source::DatamartGraphql,
        cfg.rate_dm_per_min,
        cfg.rate_dm_per_day,
    );
    if let Some(pool) = redis {
        limiter = limiter.with_redis(pool);
    }
    DatamartClient::new(&cfg.datamart_graphql_url, oauth, limiter)
}

fn build_erp_rest(cfg: &AppConfig, redis: Option<deadpool_redis::Pool>) -> ErpRestClient {
    let oauth = build_oauth(cfg);
    let mut limiter = Limiter::new(
        Source::ErpRest,
        cfg.rate_rest_per_min,
        cfg.rate_rest_per_day,
    );
    if let Some(pool) = redis {
        limiter = limiter.with_redis(pool);
    }
    ErpRestClient::new(&cfg.erp_base_url, oauth, limiter)
}

fn build_expense(cfg: &AppConfig, redis: Option<deadpool_redis::Pool>) -> Option<ExpenseClient> {
    let base_url = cfg.expense_base_url.clone()?;
    cfg.oauth_scope_expense.as_ref()?;

    let oauth = build_oauth(cfg);
    let mut limiter = Limiter::new(
        Source::ExpenseApi,
        cfg.rate_expense_per_min,
        cfg.rate_expense_per_day,
    );
    if let Some(pool) = redis {
        limiter = limiter.with_redis(pool);
    }

    Some(ExpenseClient::new(base_url, oauth, limiter))
}

pub fn build_sync_executor(
    registry: EndpointRegistry,
    state: StateStore,
    cfg: &AppConfig,
) -> SyncExecutor {
    let redis = state.redis.clone();
    let erp_adapter = ErpRestAdapter {
        client: build_erp_rest(cfg, Some(redis.clone())),
        page_size: 100,
    };
    let dm_adapter = DatamartAdapter {
        client: build_datamart(cfg, Some(redis.clone())),
        state: state.clone(),
        page_size: 500,
    };
    let expense_box: Box<dyn vsv2_orchestrator::FetchAdapter> =
        match build_expense(cfg, Some(redis)) {
            Some(client) => Box::new(ExpenseAdapter {
                client,
                page_size: 200,
            }),
            None => Box::new(NoopAdapter {
                source_name: "expense_api",
            }),
        };

    let dispatch = DispatchTable {
        erp: Box::new(erp_adapter),
        datamart: Box::new(dm_adapter),
        expense: expense_box,
    };

    let bq = BqWriter::from_config(cfg);

    SyncExecutor {
        registry,
        dispatch,
        state,
        bq,
        local_bronze_enabled: cfg.local_bronze_pg_enabled,
    }
}
