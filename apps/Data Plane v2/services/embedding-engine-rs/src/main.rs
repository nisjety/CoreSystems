mod api;
mod batch;
mod cas_store;
mod config;
mod document_erasure_consumer;
mod gdpr;
mod gdpr_nats;
mod image_consumer;
mod media_consumer;
mod provider;
mod qdrant_writer;
mod stream;
mod wiki_consumer;

use crate::config::Config;
use crate::provider::EmbeddingProvider;
use event_envelope_rs::{EventSigner, EventVerifier};
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "embedding_engine_rs=info".into()),
        )
        .json()
        .init();

    let cfg = Config::from_env()?;
    tracing::info!(
        batch_size = cfg.batch_size,
        collection = %cfg.qdrant_collection,
        "embedding-engine-rs starting"
    );

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(10)
        .connect(&cfg.database_url)
        .await?;

    let qdrant = qdrant_client::Qdrant::from_url(&cfg.qdrant_url)
        .build()
        .map_err(|e| anyhow::anyhow!("qdrant connect: {e}"))?;

    qdrant_writer::ensure_collection(&qdrant, &cfg.qdrant_collection, cfg.embedding_dimension)
        .await?;
    // D4+D5 spec §2.3: provision wiki + entity-summary collections at boot so
    // wiki-store-go and the nightly summarizer can upsert without a separate
    // bootstrap step. Same vector dim as the primary collection — both use
    // the active Model Plane embedding route by default.
    qdrant_writer::ensure_collection(&qdrant, "wiki_block_embeddings", cfg.embedding_dimension)
        .await?;
    qdrant_writer::ensure_collection(
        &qdrant,
        "entity_summary_embeddings",
        cfg.embedding_dimension,
    )
    .await?;

    let provider = EmbeddingProvider::from_config(&cfg)?;
    tracing::info!(
        provider = provider.provider_name(),
        model = provider.model_name(),
        "embedding backend selected"
    );

    let signed_events_enabled = signed_event_consumers_enabled(
        std::env::var("ENABLE_SIGNED_EVENT_CONSUMERS")
            .as_deref()
            .unwrap_or(""),
    );
    let legacy_events_enabled = unverified_legacy_events_enabled(
        std::env::var("ALLOW_UNVERIFIED_LEGACY_EVENTS")
            .as_deref()
            .unwrap_or(""),
        std::env::var("ALLOW_INSECURE_DEV_DEFAULTS")
            .as_deref()
            .unwrap_or(""),
    );
    let legacy_page_images_enabled = unsigned_page_image_mutations_enabled(
        signed_events_enabled,
        std::env::var("ALLOW_UNVERIFIED_LEGACY_EVENTS")
            .as_deref()
            .unwrap_or(""),
        std::env::var("ALLOW_INSECURE_DEV_DEFAULTS")
            .as_deref()
            .unwrap_or(""),
    );
    // Built once, shared by the per-document erasure consumer below AND the
    // whole-org GDPR purge further down — one CasStore, two erasure triggers.
    // `None` when `cas_bucket` is unset: erasure of the OTHER stores this
    // crate owns must still work when CAS erasure hasn't been turned on.
    let cas_store = if cfg.cas_bucket.trim().is_empty() {
        tracing::info!("CAS erasure disabled (CAS_BUCKET unset)");
        None
    } else {
        let endpoint =
            (!cfg.cas_endpoint_url.trim().is_empty()).then(|| cfg.cas_endpoint_url.clone());
        Some(Arc::new(
            cas_store::CasStore::new(cfg.cas_bucket.clone(), endpoint).await,
        ))
    };

    let event_runtime = if signed_events_enabled {
        if cfg.index_event_public_key_path.is_empty()
            || cfg.wiki_event_public_key_path.is_empty()
            || cfg.embedding_event_private_key_path.is_empty()
            || cfg.documents_event_public_key_path.is_empty()
        {
            anyhow::bail!(
                "signed event consumers require producer public and local private key paths"
            );
        }
        let verifier = Arc::new(EventVerifier::from_rsa_pem(
            &std::fs::read(&cfg.index_event_public_key_path)?,
            "service:index-engine-rs",
            "index-events-v1",
            &cfg.event_auth_audience,
            "events:index:publish",
            100_000,
        )?);
        let signer = Arc::new(EventSigner::from_rsa_pem(
            &std::fs::read(&cfg.embedding_event_private_key_path)?,
            "service:embedding-engine-rs",
            "embedding-events-v1",
            &cfg.event_auth_audience,
            "events:embedding:publish",
        )?);
        let wiki_verifier = Arc::new(EventVerifier::from_rsa_pem(
            &std::fs::read(&cfg.wiki_event_public_key_path)?,
            "service:wiki-store-go",
            "wiki-events-v1",
            &cfg.event_auth_audience,
            "events:wiki:publish",
            100_000,
        )?);
        // Verifies `dataplane.documents.deleted` for the per-document erasure
        // consumer below. Same producer/kid/scope every other consumer of this
        // subject already uses (`retrieval-engine-rs/src/cache/invalidator.rs`,
        // `quickwit-adapter-rs`) — see `document_erasure_consumer` module docs.
        let documents_verifier = Arc::new(EventVerifier::from_rsa_pem(
            &std::fs::read(&cfg.documents_event_public_key_path)?,
            "service:documents-api-go",
            "documents-events-v1",
            &cfg.event_auth_audience,
            "events:documents:publish",
            100_000,
        )?);
        let nats_client = nats_connection::connect(&cfg.nats_url).await?;
        let js = async_nats::jetstream::new(nats_client.clone());
        // D17: idempotent, shared definition in `nats_connection::dlq`.
        nats_connection::ensure_or_warn(&js).await;
        stream::setup_stream(&js).await?;
        let consumer = stream::create_consumer(&js).await?;
        wiki_consumer::spawn(js.clone(), qdrant.clone(), provider.clone(), wiki_verifier).await?;
        document_erasure_consumer::spawn(
            js.clone(),
            qdrant.clone(),
            cfg.qdrant_visual_collection.clone(),
            cas_store.clone(),
            nats_client.clone(),
            documents_verifier,
        )
        .await?;
        Some((consumer, nats_client, Some(verifier), Some(signer)))
    } else if legacy_events_enabled {
        tracing::warn!("unsigned embedding mutation consumers enabled for insecure development");
        let nats_client = nats_connection::connect(&cfg.nats_url).await?;
        let js = async_nats::jetstream::new(nats_client.clone());
        // D17: idempotent, shared definition in `nats_connection::dlq`.
        nats_connection::ensure_or_warn(&js).await;

        stream::setup_stream(&js).await?;
        let consumer = stream::create_consumer(&js).await?;

        // These legacy consumers trust tenant and content fields in the event
        // payload. Keep every mutation arm under the same explicit dev gate
        // until producer-scoped signed envelopes are available.
        if !legacy_page_images_enabled {
            anyhow::bail!(
                "unsigned page-image mutations are disabled outside isolated legacy development"
            );
        }

        Some((consumer, nats_client, None, None))
    } else {
        tracing::warn!("embedding and wiki consumers disabled until signed producer-scoped envelopes are available");
        None
    };

    // Visual RAG arm — Cohere Embed v4 page-image embeddings, decoupled from
    // the text-event runtime selection above so a signed-consumer deployment
    // can still run it. Online when COHERE_EMBED_V4_ENDPOINT is configured
    // AND one of two explicit gates opens:
    //   - the legacy dev gate (ALLOW_UNVERIFIED_LEGACY_EVENTS +
    //     ALLOW_INSECURE_DEV_DEFAULTS, unsigned-everything path), or
    //   - ALLOW_UNSIGNED_PAGE_IMAGE_EVENTS=1, which admits ONLY the
    //     page-image arm alongside signed text consumers.
    // The trust delta the dedicated gate accepts is narrow and different in
    // kind from the text-mutation arms it stays separated from: a
    // dataplane.page_images.created event carries no document content at all
    // — only org_id/document_id and an image_url the consumer fetches from
    // quarry-edge's internal serve route over the token-authenticated
    // plane-local broker + trusted inter-plane bus. A forged event can at
    // worst point the embedder at a wrong picture; it cannot inject document
    // text. Producer-signed envelopes for the quarry-edge producer remain
    // the target state; default stays OFF.
    let page_image_events_allowed = legacy_page_images_enabled
        || std::env::var("ALLOW_UNSIGNED_PAGE_IMAGE_EVENTS")
            .as_deref()
            .unwrap_or("")
            == "1";
    if page_image_events_allowed {
        match crate::provider::visual::VisualEmbeddingProvider::from_config(&cfg) {
            Ok(Some(visual)) => {
                tracing::info!(
                    model = visual.model_name(),
                    "visual embedding (Embed v4) enabled"
                );
                let nats_client = nats_connection::connect(&cfg.nats_url).await?;
                let js = async_nats::jetstream::new(nats_client.clone());
                // D17: the page-image consumer publishes to
                // `dataplane.dlq.embedding-engine-page-images` and can run on
                // its own (the visual arm is gated separately from the text
                // arms), so it ensures the DLQ stream itself rather than
                // assuming a text-consumer boot already did.
                nats_connection::ensure_or_warn(&js).await;
                if let Err(e) = qdrant_writer::ensure_collection(
                    &qdrant,
                    &cfg.qdrant_visual_collection,
                    cfg.visual_embedding_dimension,
                )
                .await
                {
                    tracing::warn!(error = %e, "visual collection ensure failed; continuing");
                }
                if let Err(e) = image_consumer::spawn(
                    js,
                    nats_client,
                    qdrant.clone(),
                    visual,
                    cfg.qdrant_visual_collection.clone(),
                )
                .await
                {
                    tracing::warn!(error = %e, "page-image subscriber failed to start; continuing");
                }
            }
            Ok(None) => {
                tracing::info!("visual embedding disabled (COHERE_EMBED_V4_ENDPOINT unset)")
            }
            Err(e) => {
                tracing::warn!(error = %e, "visual embedding misconfigured; continuing without visual arm")
            }
        }
    } else {
        tracing::info!(
            "page-image consumer disabled (set ALLOW_UNSIGNED_PAGE_IMAGE_EVENTS=1 to enable alongside signed consumers)"
        );
    }

    // Audio + video arms. Gated only on the endpoint, not on the page-image
    // event-signing switch above: the media subjects are a separate stream with
    // their own DLQ, so they neither need nor imply the visual arm being on.
    match provider::media::MediaEmbeddingProvider::from_config(&cfg) {
        Ok(Some(media)) => {
            tracing::info!(
                endpoint = media.endpoint(),
                "media embedding (audio/video) enabled"
            );
            let nats_client = nats_connection::connect(&cfg.nats_url).await?;
            let js = async_nats::jetstream::new(nats_client.clone());
            // Own the DLQ-stream ensure like the page-image consumer does, since
            // this arm can be the only one running.
            nats_connection::ensure_or_warn(&js).await;
            // Two collections because the towers have different dimensions
            // (CLAP 512 vs the video tower, 512 for X-CLIP / 768 for
            // SigLIP 2) and Qdrant fixes size per
            // collection. Ensure both even when the video tower is off, so
            // enabling it later needs no bootstrap step.
            for (name, dim) in [
                (&cfg.qdrant_audio_collection, cfg.audio_embedding_dimension),
                (&cfg.qdrant_video_collection, cfg.video_embedding_dimension),
            ] {
                if let Err(e) = qdrant_writer::ensure_collection(&qdrant, name, dim).await {
                    tracing::warn!(error = %e, collection = %name, "media collection ensure failed; continuing");
                }
            }
            // Caption-to-text: the temporal fix for the video arm. Off unless
            // VIDEO_CAPTION_ENABLED — see
            // docs/core-research/video-temporal-retrieval-gap-2026-08-25.md for
            // why no embedding tower can do this, and why the answer is text.
            //
            // A MISCONFIGURED enablement warns and continues with captioning
            // off, rather than taking the whole media arm down: the segment
            // vectors are the primary function here and captions are additive.
            let captioning = match provider::video_caption::VideoDescriber::from_config(&cfg) {
                Ok(Some(describer)) => {
                    tracing::info!(
                        model = describer.model_name(),
                        text_collection = %cfg.qdrant_collection,
                        "video caption-to-text ENABLED: one vision call per video segment, \
                         indexed into both text arms (restricted documents are skipped)"
                    );
                    Some(media_consumer::VideoCaptioning {
                        describer,
                        text_provider: provider.clone(),
                        text_collection: cfg.qdrant_collection.clone(),
                        pool: pool.clone(),
                    })
                }
                Ok(None) => {
                    tracing::info!("video caption-to-text disabled (VIDEO_CAPTION_ENABLED)");
                    None
                }
                Err(e) => {
                    tracing::warn!(error = %e, "video caption-to-text misconfigured; continuing without captions");
                    None
                }
            };
            if let Err(e) = media_consumer::spawn(
                js,
                nats_client,
                qdrant.clone(),
                media,
                media_consumer::MediaCollections {
                    audio: cfg.qdrant_audio_collection.clone(),
                    video: cfg.qdrant_video_collection.clone(),
                },
                captioning,
            )
            .await
            {
                tracing::warn!(error = %e, "media-segment subscriber failed to start; continuing");
            }
        }
        Ok(None) => {
            tracing::info!("media embedding disabled (MEDIA_EMBEDDER_ENDPOINT unset)")
        }
        Err(e) => {
            tracing::warn!(error = %e, "media embedding misconfigured; continuing without audio/video arms")
        }
    }

    // Cross-plane GDPR organization-erasure consumer. Deliberately
    // independent of the embedding/wiki/page-image event_runtime above and
    // spawned as its own supervised task (not raced inside the
    // `tokio::select!` below) so a shared-broker outage never takes down the
    // admin HTTP server or the embedding consumers: it binds a
    // pre-provisioned pull consumer on the SHARED cross-plane broker
    // (control-shared-nats, stream AQENCIA_CONTROLPLANE) under its own
    // dedicated `embedding-engine-gdpr` identity
    // (EMBEDDING_ENGINE_GDPR_NATS_URL/_USER/_PASSWORD) — never this
    // service's own Data-Plane-local `cfg.nats_url` connection, which does
    // not host that stream. Left unset, the consumer is intentionally
    // disabled (fail open with a warning) rather than hot-looping doomed
    // connection attempts, matching this rollout's established posture for
    // this optional consumer. See `gdpr_nats` for the full provisioning
    // contract this depends on.
    let gdpr_nats_url = std::env::var("EMBEDDING_ENGINE_GDPR_NATS_URL").unwrap_or_default();
    if gdpr_nats_url.is_empty() {
        tracing::warn!(
            "EMBEDDING_ENGINE_GDPR_NATS_URL not set; embedding-engine GDPR erasure consumer disabled"
        );
    } else {
        let gdpr_nats_user = std::env::var("EMBEDDING_ENGINE_GDPR_NATS_USER").unwrap_or_default();
        let gdpr_nats_password =
            std::env::var("EMBEDDING_ENGINE_GDPR_NATS_PASSWORD").unwrap_or_default();
        let gdpr_collections = Arc::new(gdpr::PurgeCollections::from_config(
            &cfg,
            wiki_consumer::WIKI_COLLECTION,
        ));
        tracing::info!(
            collections = ?gdpr_collections.all(),
            "GDPR org-erasure will purge these collections"
        );
        tokio::spawn(gdpr_nats::run_supervised(
            qdrant.clone(),
            gdpr_collections,
            cas_store.clone(),
            gdpr_nats_url,
            gdpr_nats_user,
            gdpr_nats_password,
        ));
    }

    let admin_app = api::router();
    let admin_addr = format!("0.0.0.0:{}", cfg.admin_port);
    let admin_listener = tokio::net::TcpListener::bind(&admin_addr).await?;
    tracing::info!("admin on {admin_addr}");

    let consumer_task = async move {
        match event_runtime {
            Some((consumer, nats_client, verifier, signer)) => {
                stream::run_consumer(
                    consumer,
                    pool,
                    qdrant,
                    provider,
                    cfg,
                    nats_client,
                    stream::EventSecurity { verifier, signer },
                )
                .await
            }
            None => std::future::pending::<anyhow::Result<()>>().await,
        }
    };

    tokio::select! {
        res = axum::serve(admin_listener, admin_app) => {
            if let Err(e) = res { tracing::error!(err = %e, "admin error"); }
        }
        res = consumer_task => {
            if let Err(e) = res { tracing::error!(err = %e, "consumer error"); }
        }
    }

    Ok(())
}

fn unverified_legacy_events_enabled(legacy: &str, insecure_dev: &str) -> bool {
    legacy == "1" && insecure_dev == "1"
}

fn signed_event_consumers_enabled(value: &str) -> bool {
    value == "1"
}

fn unsigned_page_image_mutations_enabled(
    signed_events_enabled: bool,
    legacy: &str,
    insecure_dev: &str,
) -> bool {
    !signed_events_enabled && legacy == "1" && insecure_dev == "1"
}

#[cfg(test)]
mod event_containment_tests {
    use super::{
        signed_event_consumers_enabled, unsigned_page_image_mutations_enabled,
        unverified_legacy_events_enabled,
    };

    #[test]
    fn unsigned_embedding_mutations_require_two_explicit_dev_gates() {
        assert!(!unverified_legacy_events_enabled("", ""));
        assert!(!unverified_legacy_events_enabled("1", ""));
        assert!(!unverified_legacy_events_enabled("", "1"));
        assert!(unverified_legacy_events_enabled("1", "1"));
    }

    #[test]
    fn signed_consumers_require_explicit_enablement() {
        assert!(!signed_event_consumers_enabled(""));
        assert!(signed_event_consumers_enabled("1"));
    }

    #[test]
    fn unsigned_page_image_deletion_is_impossible_in_signed_or_production_posture() {
        assert!(!unsigned_page_image_mutations_enabled(false, "", ""));
        assert!(!unsigned_page_image_mutations_enabled(false, "1", ""));
        assert!(!unsigned_page_image_mutations_enabled(false, "", "1"));
        assert!(!unsigned_page_image_mutations_enabled(true, "1", "1"));
        assert!(unsigned_page_image_mutations_enabled(false, "1", "1"));
    }
}
