use anyhow::Result;
use conversation_ingest_rs::{build_router, AppState};
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
    let internal_api_key = env::var("INTERNAL_API_KEY").unwrap_or_default();

    let state = AppState {
        conversation_core_url: conversation_core_url.trim_end_matches('/').to_owned(),
        internal_api_key,
        client: reqwest::Client::builder()
            .timeout(Duration::from_secs(8))
            .build()?,
    };

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    info!(%addr, "conversation-ingest-rs listening");
    axum::serve(listener, build_router(state)).await?;
    Ok(())
}
