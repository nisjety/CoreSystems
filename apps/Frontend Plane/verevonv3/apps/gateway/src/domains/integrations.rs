mod browse;
mod connect_sessions;
pub(crate) mod connections;
mod credential_bindings;
pub(crate) mod model_subscriptions;
mod model_connections;
mod profile;
pub(crate) mod providers;
mod shared;
mod sync_jobs;
mod algolia;
#[cfg(test)]
mod algolia_tests;

use axum::{
    routing::{delete, get, post},
    Router,
};

use crate::{config::AppState, middleware::require_session};

pub(crate) fn router(state: AppState) -> Router<AppState> {
    let zdr_waiver_ui_enabled = state.zdr_waiver_ui_enabled;
    let router = Router::new()
        // Model subscriptions. These are separate from source integrations:
        // they establish a credential-free browser device flow for Model Plane.
        .route(
            "/api/v1/model-subscriptions/openai-codex/connect",
            post(model_subscriptions::start_openai_codex_subscription),
        )
        .route(
            "/api/v1/model-subscriptions/openai-codex/connect/{connection_id}/{login_id}",
            get(model_subscriptions::openai_codex_subscription_status),
        )
        .route(
            "/api/v1/model-subscriptions/openai-codex/connections/{connection_id}",
            delete(model_subscriptions::disconnect_openai_codex_subscription),
        )
        .route(
            "/api/v1/model-subscriptions/openai-codex/connections/{connection_id}/usage",
            get(model_subscriptions::openai_codex_subscription_usage),
        )
        // Customer-gateway model connections (W4-BYO-6): an org admin registers, lists and removes the org's own
        // OpenAI-compatible gateway. Integration Core holds the key; the browser never reads it back.
        .route(
            "/api/v1/model-connections/customer-gateway",
            get(model_connections::list_customer_gateways),
        )
        .route(
            "/api/v1/model-connections/customer-gateway",
            post(model_connections::create_customer_gateway),
        )
        .route(
            "/api/v1/model-connections/customer-gateway",
            delete(model_connections::delete_customer_gateway),
        )
        // Credential bindings and the custom REST connector (W4-CC-8): an organisation admin stores one key for one
        // host and creates a connector that calls it. Integration Core seals the key; the browser never reads it back.
        // Off, every one of these paths answers 404.
        .route(
            "/api/v1/credential-bindings",
            get(credential_bindings::list_credential_bindings),
        )
        // A stored key is at most 4096 bytes, and JSON escaping can double it, so 16 KiB bounds both POST bodies.
        .route(
            "/api/v1/credential-bindings",
            post(credential_bindings::create_credential_binding)
                .layer(axum::extract::DefaultBodyLimit::max(16 * 1024)),
        )
        .route(
            "/api/v1/credential-bindings/{id}",
            delete(credential_bindings::revoke_credential_binding),
        )
        .route(
            "/api/v1/connectors/custom-rest",
            post(credential_bindings::create_custom_rest_connector)
                .layer(axum::extract::DefaultBodyLimit::max(16 * 1024)),
        )
        .route("/api/v1/integrations/algolia/connections",get(algolia::connections))
        .route("/api/v1/integrations/algolia/connect-session",post(algolia::connect).layer(axum::extract::DefaultBodyLimit::max(2048)))
        .route("/api/v1/integrations/algolia/connect-sessions/{id}/status",get(algolia::status))
        .route("/api/v1/integrations/algolia/connections/{id}/applications",get(algolia::applications))
        .route("/api/v1/integrations/algolia/connections/{id}/indices",get(algolia::indices))
        // Providers
        .route(
            "/api/v1/integrations/providers",
            get(providers::list_providers),
        )
        .route(
            "/api/v1/integrations/providers/{provider}/connect-session",
            post(providers::start_connect_session),
        )
        // Connect sessions
        .route(
            "/api/v1/integrations/connect-sessions/{id}/status",
            get(connect_sessions::connect_session_status),
        )
        // Connections — static "connections" route before param ":id" routes
        .route(
            "/api/v1/integrations/connections",
            get(connections::list_connections),
        )
        .route(
            "/api/v1/integrations/connections/{id}",
            get(connections::get_connection),
        )
        .route(
            "/api/v1/integrations/connections/{id}",
            delete(connections::disconnect),
        )
        .route(
            "/api/v1/integrations/connections/{id}/sync",
            post(connections::trigger_sync),
        )
        .route(
            "/api/v1/integrations/connections/{id}/inbox-sync",
            post(connections::trigger_inbox_sync),
        )
        .route(
            "/api/v1/integrations/connections/{id}/inbox-history",
            post(connections::extend_inbox_history),
        )
        // Connector-attach picker (composer): browser-reachable list/search +
        // single-item content read, gated by a fixed read-only operation
        // allowlist in browse.rs on top of integration-corev2's own
        // capability gate. First browser-reachable route into the action
        // executor — execution-core's agent-tool path is separate and unaffected.
        .route(
            "/api/v1/integrations/connections/{id}/browse",
            post(browse::browse_connection),
        )
        .route(
            "/api/v1/integrations/connections/{id}/content",
            post(browse::content_connection),
        )
        // Sync jobs — static before param
        .route(
            "/api/v1/integrations/sync-jobs",
            get(sync_jobs::list_sync_jobs),
        )
        .route(
            "/api/v1/integrations/sync-jobs/{id}/events",
            get(sync_jobs::sync_job_events),
        )
        .route(
            "/api/v1/integrations/sync-jobs/{id}",
            get(sync_jobs::get_sync_job),
        )
        // Profile projection
        .route(
            "/api/v1/integrations/profile",
            get(profile::integration_profile),
        );
    // ZDR assessment and acknowledgement (Z-19c): Integration Core owns the record, so these forward the caller's
    // own bearer and answer with its envelope. Mounted only while `ZDR_WAIVER_UI_ENABLED` is the literal `true`
    // (read once into `AppState`); off, they are not mounted and answer 404. The static `zdr-acknowledgements`
    // path comes before the `{connection_id}` routes.
    let router = if zdr_waiver_ui_enabled {
        router
            .route(
                "/api/v1/model-connections/zdr-acknowledgements",
                get(model_connections::list_zdr_acknowledgements),
            )
            .route(
                "/api/v1/model-connections/{connection_id}/zdr",
                get(model_connections::get_zdr_assessment),
            )
            .route(
                "/api/v1/model-connections/{connection_id}/zdr-acknowledgement",
                post(model_connections::acknowledge_zdr)
                    .layer(axum::extract::DefaultBodyLimit::max(8 * 1024)),
            )
            .route(
                "/api/v1/model-connections/{connection_id}/zdr-acknowledgement",
                delete(model_connections::withdraw_zdr_acknowledgement),
            )
    } else {
        router
    };
    router.route_layer(axum::middleware::from_fn_with_state(state, require_session))
}
