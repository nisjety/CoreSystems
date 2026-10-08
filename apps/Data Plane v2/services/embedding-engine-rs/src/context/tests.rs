use super::*;

/// The ceiling is half of the consumer's ack wait, and the ack wait is what
/// `stream::create_consumer` really configures: read from its source, so changing one
/// without the other fails here instead of redelivering batches in production.
#[test]
fn the_inline_ceiling_is_half_of_the_consumers_ack_wait() {
    let stream = include_str!("../stream/mod.rs");
    assert!(
        stream.contains("ack_wait: Duration::from_secs(60)"),
        "stream::create_consumer no longer sets a 60 s ack_wait; INLINE_BATCH_BUDGET_CEILING is derived from it"
    );
    assert_eq!(INLINE_BATCH_BUDGET_CEILING * 2, Duration::from_secs(60));
}

#[test]
fn the_default_budgets_are_accepted_and_the_ceiling_is_the_most_allowed() {
    let budget = InlineBudget::new(8_000, 20_000).expect("the defaults");
    assert_eq!(budget.document, Duration::from_secs(8));
    assert_eq!(budget.batch, Duration::from_secs(20));
    assert!(InlineBudget::new(8_000, 30_000).is_ok());
    assert!(InlineBudget::new(30_000, 30_000).is_ok());
}

#[test]
fn a_budget_that_would_redeliver_the_batch_is_refused_at_startup() {
    let error = InlineBudget::new(8_000, 30_001).expect_err("past the ceiling");
    assert!(error.contains("CONTEXT_INLINE_BUDGET_MS"), "{error}");
    assert!(error.contains("ack_wait"), "{error}");
    let error = InlineBudget::new(25_000, 20_000).expect_err("a document longer than its batch");
    assert!(
        error.contains("CONTEXT_INLINE_DOCUMENT_BUDGET_MS"),
        "{error}"
    );
    let error = InlineBudget::new(0, 20_000).expect_err("no time for any document");
    assert!(
        error.contains("CONTEXT_INLINE_DOCUMENT_BUDGET_MS"),
        "{error}"
    );
}

/// Zero means "generate nothing inline": the deadline is the moment the batch
/// starts, so every unit is embedded raw and left pending.
#[tokio::test(start_paused = true)]
async fn a_zero_batch_budget_expires_at_once() {
    let budget = InlineBudget::new(8_000, 0).expect("async-only");
    let now = Instant::now();
    let clock = budget.start(now);
    assert_eq!(clock.batch_deadline(), now);
    assert!(clock.document_deadline(now) <= now);
}

/// A slow document cannot take the time a later one needs: its deadline is its own
/// budget, but never past the batch's.
#[tokio::test(start_paused = true)]
async fn a_documents_deadline_never_outlives_the_batchs() {
    let budget = InlineBudget::new(8_000, 20_000).expect("defaults");
    let start = Instant::now();
    let clock = budget.start(start);
    assert_eq!(
        clock.document_deadline(start),
        start + Duration::from_secs(8)
    );
    tokio::time::advance(Duration::from_secs(15)).await;
    // Seven seconds are left in the batch; a document that starts now gets seven,
    // not eight.
    assert_eq!(
        clock.document_deadline(Instant::now()),
        start + Duration::from_secs(20)
    );
    tokio::time::advance(Duration::from_secs(10)).await;
    assert!(clock.document_deadline(Instant::now()) <= Instant::now());
}

#[test]
fn a_prepared_document_uses_the_organizations_pin_over_the_services() {
    let input = DocumentInput {
        org_id: "org",
        document_id: "doc",
        meta: DocumentMeta::default(),
        text: "tekst",
        restricted: false,
        prompt_version: None,
        total_chunks: None,
    };
    let service = PreparedDocument::new(input, PromptVersion::V1, WindowConfig::default());
    assert_eq!(service.version(), PromptVersion::V1);
    let pinned = PreparedDocument::new(
        DocumentInput {
            prompt_version: Some(PromptVersion::V2),
            ..input
        },
        PromptVersion::V1,
        WindowConfig::default(),
    );
    assert_eq!(pinned.version(), PromptVersion::V2);
    assert!(pinned.windows().is_whole());
    let chunk = ChunkInput {
        index: 0,
        text: "tekst",
    };
    assert_eq!(pinned.window_for(&chunk).text, "tekst");
}
