//! inference-core — provider routing execution for LLMs.
//!
//! Authenticated gRPC inference on :9092, HTTP health/metrics on :8082.

use anyhow::Result;
use inference_core::{auth::JwtVerifier, config, grpc, http_health, provider};
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    let _otel_guard = mp_telemetry::init("inference-core")?;
    info!("inference-core starting");

    let cfg = config::InferenceConfig::from_env()?;
    info!(
        providers = ?cfg.provider_order,
        "provider chain configured"
    );

    let chain = provider::fallback::FallbackChain::from_config(&cfg);
    let speech = provider::speech::SpeechChain::from_env();
    info!(
        providers = speech.provider_count(),
        "speech provider chain configured"
    );
    let translation = provider::translation::TranslationChain::from_env();
    info!(
        providers = translation.provider_count(),
        "translation provider chain configured"
    );
    let vision = provider::vision::VisionChain::from_env();
    info!(
        providers = vision.provider_count(),
        "vision provider chain configured"
    );
    let doc_intel = provider::doc_intel::DocIntelChain::from_env();
    info!(
        providers = doc_intel.provider_count(),
        "document intelligence provider chain configured"
    );
    let language = provider::language::LanguageAnalyticsChain::from_env();
    info!(
        providers = language.provider_count(),
        "language analytics provider chain configured"
    );
    let realtime = provider::realtime::RealtimeChain::from_env();
    info!(
        providers = realtime.provider_count(),
        "realtime provider chain configured"
    );
    let video = provider::video::VideoChain::from_env();
    info!(
        providers = video.provider_count(),
        "video provider chain configured"
    );

    // Eager JWKS loading is a readiness gate: missing/unavailable verification
    // material prevents the listener and provider surface from starting.
    let auth = JwtVerifier::from_env().await?;
    let policy_state = http_health::PolicyState {
        auth: auth.clone(),
        policy: chain.policy_handle(),
        client: chain.policy_client(),
    };
    let grpc_handle = tokio::spawn(grpc::serve_with_providers(
        grpc::ProviderChains {
            chain: chain.clone(),
            speech,
            translation,
            vision,
            doc_intel,
            language,
            realtime,
            video,
        },
        auth,
    ));
    let http_handle = tokio::spawn(http_health::serve(policy_state));

    let shutdown = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler")
            .recv()
            .await;
        info!("SIGTERM received, shutting down");
    };

    tokio::select! {
        result = grpc_handle => result??,
        result = http_handle => result??,
        () = shutdown => {},
    }

    info!("inference-core stopped");
    Ok(())
}
