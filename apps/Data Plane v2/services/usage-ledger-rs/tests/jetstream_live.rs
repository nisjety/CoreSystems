//! The real broker, end to end: the stream is created, events are published with the broker's
//! acknowledgement, a retry is dropped by the broker's duplicate window, and the events are
//! what the Go consumer reads (`services/data-orchestrator-go/internal/cost/live_test.go`).
//!
//! Needs a disposable NATS server with JetStream, so it is ignored by default:
//!
//! ```text
//! USAGE_LEDGER_TEST_NATS_URL=nats://127.0.0.1:4222 \
//! USAGE_LEDGER_TEST_KEYS_DIR=<an empty directory> \
//!   cargo test -p usage-ledger-rs --test jetstream_live -- --ignored
//! ```
//!
//! With the directory it also writes the three producers' public keys (PKIX PEM, as
//! data-orchestrator reads them) and `expected-events.txt`, which the Go test reads: run it
//! afterwards with the same directory and `COST_LIVE_NATS_URL`.

use std::sync::Arc;

use async_nats::jetstream;
use event_envelope_rs::EventSigner;
use rsa::pkcs1::EncodeRsaPrivateKey;
use rsa::pkcs8::{EncodePublicKey, LineEnding};
use rsa::{RsaPrivateKey, RsaPublicKey};
use usage_ledger_rs::{
    ensure_stream, CallUsage, Feature, JetStreamLedger, LedgerSink, UsageBuilder, UsageEvent,
    STREAM_NAME,
};

struct Producer {
    name: &'static str,
    issuer: &'static str,
    key_id: &'static str,
    scope: &'static str,
}

const PRODUCERS: [Producer; 3] = [
    Producer {
        name: "embedding-engine-rs",
        issuer: "service:embedding-engine-rs",
        key_id: "embedding-events-v1",
        scope: "events:embedding:publish",
    },
    Producer {
        name: "graph-index-rs",
        issuer: "service:graph-index-rs",
        key_id: "graph-events-v1",
        scope: "events:graph:publish",
    },
    Producer {
        name: "retrieval-engine-rs",
        issuer: "service:retrieval-engine-rs",
        key_id: "retrieval-events-v1",
        scope: "events:retrieval:publish",
    },
];

fn event(feature: Feature, org: &str, zdr: bool, id: &str, input: u64, output: u64) -> UsageEvent {
    let mut builder = UsageBuilder::new(feature, org, zdr).expect("builder");
    builder.push(CallUsage::from_counts(
        id,
        "gpt-4o-mini-2024-07-18",
        "azure-openai",
        input,
        output,
        0,
        true,
        false,
    ));
    builder.build().remove(0)
}

#[tokio::test]
#[ignore = "requires USAGE_LEDGER_TEST_NATS_URL: a disposable NATS server with JetStream"]
async fn events_are_published_with_an_acknowledgement_and_a_retry_is_dropped_by_the_broker() {
    let url = std::env::var("USAGE_LEDGER_TEST_NATS_URL")
        .expect("USAGE_LEDGER_TEST_NATS_URL names a disposable NATS server with JetStream");
    let keys_dir = std::env::var("USAGE_LEDGER_TEST_KEYS_DIR").ok();
    let client = async_nats::connect(url).await.expect("connect");
    let js = jetstream::new(client.clone());

    // Idempotent: a producer and the consumer each create the stream on boot.
    ensure_stream(&js).await.expect("create the stream");
    ensure_stream(&js).await.expect("ensure it again");
    let info = js
        .get_stream(STREAM_NAME)
        .await
        .expect("the stream exists")
        .cached_info()
        .config
        .clone();
    // What the ledger decides is what the broker holds (the server fills in the defaults the
    // definition leaves out, so only the decisions are compared).
    let wanted = usage_ledger_rs::stream_config();
    assert_eq!(info.name, wanted.name);
    assert_eq!(info.subjects, wanted.subjects);
    assert_eq!(info.retention, wanted.retention);
    assert_eq!(info.storage, wanted.storage);
    assert_eq!(info.max_age, wanted.max_age);
    assert_eq!(info.max_bytes, wanted.max_bytes);
    assert_eq!(info.discard, wanted.discard);
    assert_eq!(info.duplicate_window, wanted.duplicate_window);

    let mut ledgers = Vec::new();
    for producer in &PRODUCERS {
        let private = RsaPrivateKey::new(&mut rand::thread_rng(), 2048).expect("test RSA key");
        if let Some(dir) = &keys_dir {
            let public = RsaPublicKey::from(&private)
                .to_public_key_pem(LineEnding::LF)
                .expect("public pem");
            std::fs::write(format!("{dir}/{}.pub", producer.name), public).expect("write key");
        }
        let signer = EventSigner::from_rsa_pem(
            private
                .to_pkcs1_pem(LineEnding::LF)
                .expect("pem")
                .as_bytes(),
            producer.issuer,
            producer.key_id,
            "dataplane-events",
            producer.scope,
        )
        .expect("signer");
        ledgers.push(JetStreamLedger::new(client.clone(), Arc::new(signer)));
    }

    let before = js
        .get_stream(STREAM_NAME)
        .await
        .unwrap()
        .get_info()
        .await
        .unwrap()
        .state
        .messages;
    let events = [
        (
            0,
            event(
                Feature::ContextualRetrieval,
                "org-live-a",
                false,
                "live-1",
                1200,
                60,
            ),
        ),
        (
            0,
            event(Feature::Embedding, "org-live-a", true, "live-2", 900, 0),
        ),
        (
            1,
            event(
                Feature::GraphExtraction,
                "org-live-b",
                false,
                "live-3",
                2100,
                480,
            ),
        ),
        (
            2,
            event(Feature::WebRerank, "org-live-a", false, "live-4", 0, 0),
        ),
    ];
    for (producer, event) in &events {
        ledgers[*producer]
            .publish(event)
            .await
            .expect("the broker acknowledged the event");
    }
    // A retry of the first event: the broker's duplicate window drops it (same Nats-Msg-Id),
    // and the producer cannot tell, which is the point.
    ledgers[0]
        .publish(&events[0].1)
        .await
        .expect("a retry is acknowledged too");

    let after = js
        .get_stream(STREAM_NAME)
        .await
        .unwrap()
        .get_info()
        .await
        .unwrap()
        .state
        .messages;
    assert_eq!(
        after - before,
        events.len() as u64,
        "four events and one retry must be four messages in the stream"
    );

    if let Some(dir) = keys_dir {
        std::fs::write(
            format!("{dir}/expected-events.txt"),
            format!("{}\n", events.len()),
        )
        .expect("write the expectation");
    }
}
