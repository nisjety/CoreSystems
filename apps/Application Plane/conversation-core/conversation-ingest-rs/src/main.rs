use anyhow::Result;
use conversation_ingest_rs::{build_router, new_replay_cache, AppState};
use std::{env, net::SocketAddr, time::Duration};
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .json()
        .init();

    let port = env::var("PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(3161);
    let conversation_core_url = env::var("CONVERSATION_CORE_URL")
        .unwrap_or_else(|_| "http://conversation-core-go:3160".into());
    let ingest_service_token =
        env::var("CONVERSATION_EMAIL_INGEST_SERVICE_TOKEN").unwrap_or_default();
    let conversation_core_service_token =
        env::var("CONVERSATION_CORE_SERVICE_TOKEN").unwrap_or_default();
    for (name, token) in [
        (
            "CONVERSATION_EMAIL_INGEST_SERVICE_TOKEN",
            &ingest_service_token,
        ),
        (
            "CONVERSATION_CORE_SERVICE_TOKEN",
            &conversation_core_service_token,
        ),
    ] {
        let normalized = token.trim().to_ascii_lowercase();
        if token.trim().len() < 32
            || normalized.starts_with("change-me")
            || normalized.starts_with("replace-with")
        {
            anyhow::bail!("{name} must be a non-placeholder secret of at least 32 bytes");
        }
    }
    if ingest_service_token.trim() == conversation_core_service_token.trim() {
        anyhow::bail!("email-ingest and conversation-core delegation tokens must be distinct");
    }

    let state = AppState {
        conversation_core_url: conversation_core_url.trim_end_matches('/').to_owned(),
        ingest_service_token: ingest_service_token.trim().to_owned(),
        conversation_core_service_token: conversation_core_service_token.trim().to_owned(),
        delegation_replays: new_replay_cache(),
        client: reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(8))
            .build()?,
    };

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    info!(%addr, "conversation-ingest-rs listening");
    axum::serve(listener, build_router(state)).await?;
    Ok(())
}
