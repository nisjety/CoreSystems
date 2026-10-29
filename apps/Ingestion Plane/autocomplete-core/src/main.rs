use autocomplete_core::{build_app, ingest, AppState, Settings};
use tokio::net::TcpListener;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

#[tokio::main]
async fn main() -> autocomplete_core::AppResult<()> {
    init_tracing();

    let settings = Settings::from_env()?;
    let state = AppState::from_settings(settings.clone()).await?;

    let _nats_task = if settings.nats.enabled {
        Some(ingest::spawn_nats_consumer(
            settings.nats.clone(),
            state.ingest.clone(),
        ))
    } else {
        tracing::info!("nats consumer disabled; serving API only");
        None
    };

    let app = build_app(state);
    let listener = TcpListener::bind(settings.http_addr).await?;
    let local_addr = listener.local_addr().unwrap_or(settings.http_addr);
    tracing::info!(addr = %local_addr, "autocomplete-core listening");

    axum::serve(listener, app).await?;
    Ok(())
}

fn init_tracing() {
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("autocomplete_core=info,tower_http=info"));

    tracing_subscriber::registry()
        .with(env_filter)
        .with(tracing_subscriber::fmt::layer().json())
        .init();
}
