//! The graph extraction pipeline against the rules it really runs under (design task CR-T12).
//!
//! Each scenario is written once, against [`Control`], and runs on both rigs of
//! `harness`: in memory (always) and on the migrated schema with the real state SQL
//! (`#[ignore]`d behind `MIGRATED_TEST_DATABASE_URL`, and run, never skipped, by
//! `tests/scripts/run-postgres-contract-tests.sh`). What the scenarios prove, by the design's
//! own list:
//!
//! * a chunk that fails ten times in a row stops being retried after the cap and is reported
//!   `failed_permanent` and dead-lettered once (the live repro, 2026-09-29);
//! * the backoff follows the ladder;
//! * an empty result is durable across a restart: zero provider calls on the next pass;
//! * a zero-retention organization, a policy that is off and a restricted or private document
//!   make zero calls and leave a marker;
//! * a posture Auth Core could not resolve is a wait that never counts;
//! * the typed gRPC code is in the structured log;
//! * a spend is recorded for every paid call and for nothing else;
//! * the budget is alert-only unless an operator says `stop`.

use super::harness::{Control, Database, Memory};
use super::*;
use crate::testing::{
    answer, capture_logs, fail, golden_status, EMPTY_EXTRACTION, RICH_EXTRACTION,
};

const FAILED: &str = "graph extraction failed";
const GIVEN_UP: &str =
    "graph extraction GIVEN UP on a chunk: it will not be retried until an operator requeues it";

async fn pass<P: Control>(rig: &P, org: &str) -> OrgReport {
    run_org_pass(rig, org, 5).await.expect("a pass")
}

async fn statuses<P: Control>(rig: &P, org: &str, chunks: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for chunk in chunks {
        out.push(match rig.row(org, chunk).await {
            Some(row) => match row.error_class {
                Some(class) => format!("{}/{class}", row.status),
                None => row.status,
            },
            None => "<no state>".to_owned(),
        });
    }
    out
}

fn calls<P: Control>(rig: &P) -> usize {
    rig.parts().inference.call_count()
}

// ── the poison cap and the ladder ────────────────────────────────────────────

/// The live repro: a chunk whose provider keeps failing was retried at least ten times at
/// six-minute intervals, and re-paid on every restart. Now the sixth failed attempt is the last,
/// the chunk is `failed_permanent`, dead-lettered once, and the next four passes make no call.
async fn a_chunk_that_fails_ten_times_in_a_row_stops_after_the_cap<P: Control>(rig: &P) {
    let org = rig.org("cap");
    let chunks = rig
        .add_document(
            &org,
            "poisoned",
            "org",
            "internal",
            &["a chunk nobody can extract"],
        )
        .await;
    rig.parts().inference.always(fail("provider_unavailable"));
    let (logs, _guard) = capture_logs();

    let mut given_up = 0;
    for _ in 0..10 {
        given_up += pass(rig, &org).await.given_up;
        rig.make_due(&org, &chunks[0]).await;
    }

    assert_eq!(
        calls(rig),
        6,
        "six attempts, then nothing: a chunk that keeps failing is not paid for forever"
    );
    assert_eq!(given_up, 1, "reported failed_permanent exactly once");
    let row = rig
        .row(&org, &chunks[0])
        .await
        .expect("the chunk has a state");
    assert_eq!(row.status, "failed_permanent");
    assert_eq!(row.attempts, 6);
    assert_eq!(row.error_class.as_deref(), Some("provider_unavailable"));
    assert_eq!(row.retry_in_secs, None, "given up: nothing is scheduled");
    assert_eq!(
        rig.letters(),
        vec![DeadLetter {
            org_id: org.clone(),
            document_id: format!("{org}-poisoned"),
            knowledge_id: chunks[0].clone(),
            error_class: ErrorClass::ProviderUnavailable,
            attempts: 6,
        }],
        "one dead letter, when it was given up on"
    );
    assert_eq!(logs.events_saying(GIVEN_UP).len(), 1);
    assert_eq!(logs.events_saying(FAILED).len(), 5);

    // And it stays given up: an operator puts it back to pending, and only then is it tried.
    assert_eq!(pass(rig, &org).await.documents, 0, "no document has work");
}

/// Backoff: 5 minutes, 15, 45, 2 hours 15 and 6 hours.
async fn the_backoff_follows_the_ladder_on_the_rows<P: Control>(rig: &P) {
    let org = rig.org("ladder");
    let chunks = rig
        .add_document(&org, "d", "org", "internal", &["a chunk"])
        .await;
    rig.parts().inference.always(fail("provider_http"));
    for expected in [300i64, 900, 2_700, 8_100, 21_600] {
        pass(rig, &org).await;
        let row = rig.row(&org, &chunks[0]).await.expect("state");
        assert_eq!(row.status, "failed_transient");
        let wait = row.retry_in_secs.expect("a wait is scheduled");
        assert!(
            (wait - expected).abs() <= 2,
            "attempt {} waits {wait} s, want {expected}",
            row.attempts
        );
        // Not due before the wait is over: a pass makes no call.
        let before = calls(rig);
        pass(rig, &org).await;
        assert_eq!(calls(rig), before, "the chunk is waiting out its backoff");
        rig.make_due(&org, &chunks[0]).await;
    }
}

/// A provider that is down is asked once per pass, not once per chunk of the backlog; and the
/// chunks it was not asked about are not charged an attempt.
async fn a_provider_that_is_down_is_asked_once_and_the_rest_of_the_backlog_is_not_charged<
    P: Control,
>(
    rig: &P,
) {
    let org = rig.org("down");
    let chunks = rig
        .add_document(&org, "d", "org", "internal", &["a", "b", "c", "d", "e"])
        .await;
    rig.parts().inference.always(fail("provider_unavailable"));
    let report = pass(rig, &org).await;
    assert_eq!(calls(rig), 1, "one call, then the pass stops");
    assert_eq!(report.healed, 0);
    let rows = statuses(rig, &org, &chunks).await;
    assert_eq!(
        rows,
        [
            "failed_transient/provider_unavailable",
            "<no state>",
            "<no state>",
            "<no state>",
            "<no state>"
        ]
    );
}

/// A model that answers with something that is not an extraction is not systemic: the pass tries
/// the next chunk, and stops after three failures in a row. Every one of those calls was
/// answered and paid for, so every one is in the ledger.
async fn garbage_answers_stop_a_pass_after_three_and_are_charged_all_the_same<P: Control>(rig: &P) {
    let org = rig.org("garbage");
    let chunks = rig
        .add_document(&org, "d", "org", "internal", &["a", "b", "c", "d", "e"])
        .await;
    rig.parts()
        .inference
        .always(answer("I am sorry, I cannot do that."));

    pass(rig, &org).await;

    assert_eq!(
        calls(rig),
        3,
        "the breaker: three failures in a row, then stop"
    );
    let rows = statuses(rig, &org, &chunks).await;
    assert_eq!(
        rows,
        [
            "failed_transient/invalid_output",
            "failed_transient/invalid_output",
            "failed_transient/invalid_output",
            "<no state>",
            "<no state>"
        ]
    );
    let events = rig.parts().ledger.events();
    assert_eq!(
        events.len(),
        3,
        "the answers were paid for, whatever was made of them"
    );
    assert!(events
        .iter()
        .all(|event| event.feature == "graph-extraction"
            && (event.input_tokens, event.output_tokens) == (612, 48)));
    for chunk in &chunks[..3] {
        assert_eq!(rig.row(&org, chunk).await.expect("state").attempts, 1);
    }
}

// ── durable emptiness ────────────────────────────────────────────────────────

/// Nine documents that hold no entities cost 63 provider calls on every tick, about 18,000 a day,
/// and every restart paid for all of them again. An empty extraction is now a row.
async fn an_empty_result_is_durable_across_a_restart<P: Control>(rig: &P) {
    let org = rig.org("empty");
    let chunks = rig
        .add_document(&org, "d", "org", "internal", &["nothing here", "nor here"])
        .await;
    rig.parts().inference.always(answer(EMPTY_EXTRACTION));

    let first = pass(rig, &org).await;
    assert_eq!((first.documents, first.healed), (1, 0));
    assert_eq!(calls(rig), 2, "one call per chunk, once");
    for chunk in &chunks {
        let row = rig.row(&org, chunk).await.expect("state");
        assert_eq!(row.status, "empty");
        assert_eq!(row.prompt_version.as_deref(), Some(PROMPT_VERSION));
        assert_eq!(
            row.model.as_deref(),
            Some("gpt-4o-mini-2024-07-18"),
            "stamped with what served the call"
        );
        assert_eq!(row.provider.as_deref(), Some("azure-openai"));
        assert_eq!(
            (row.entities, row.relationships, row.claims),
            (Some(0), Some(0), Some(0))
        );
    }

    // The process restarts: a new extractor, a new bearer cache, the same database.
    let rig = rig.restart().await;
    for _ in 0..3 {
        assert_eq!(pass(&rig, &org).await.documents, 0, "no document has work");
    }
    let streamed = handle_document(&rig, Origin::Stream, &org, &format!("{org}-d"))
        .await
        .expect("a re-announced document");
    assert_eq!((streamed.empty, streamed.not_due), (0, 2));
    assert_eq!(
        calls(&rig),
        2,
        "zero provider calls after the restart, on the reconciler and on a re-announce"
    );

    // A prompt that changed is the one thing that makes an empty chunk worth another look.
    rig.set_prompt_version(&org, &chunks[0], "graph-v0").await;
    pass(&rig, &org).await;
    assert_eq!(
        calls(&rig),
        3,
        "only the chunk recorded under another prompt"
    );
    assert_eq!(
        rig.row(&org, &chunks[0])
            .await
            .expect("state")
            .prompt_version
            .as_deref(),
        Some(PROMPT_VERSION)
    );
}

/// Chunks that were extracted before the state existed (the migration's backfill) are `ready`,
/// and neither path pays for them again; nor for a chunk that stored something.
async fn what_was_already_extracted_is_never_extracted_again<P: Control>(rig: &P) {
    let org = rig.org("final");
    let legacy = rig
        .add_document(&org, "old", "org", "internal", &["from before"])
        .await;
    rig.set_legacy_ready(&org, &legacy[0]).await;
    let fresh = rig
        .add_document(&org, "new", "org", "internal", &["from today"])
        .await;
    rig.parts().inference.always(answer(RICH_EXTRACTION));

    pass(rig, &org).await;
    assert_eq!(
        calls(rig),
        1,
        "only the chunk that has no state is extracted"
    );
    pass(rig, &org).await;
    handle_document(rig, Origin::Stream, &org, &format!("{org}-old"))
        .await
        .expect("a re-announce");
    handle_document(rig, Origin::Stream, &org, &format!("{org}-new"))
        .await
        .expect("a re-announce");
    assert_eq!(calls(rig), 1, "ready stays ready");

    let old = rig.row(&org, &legacy[0]).await.expect("state");
    assert_eq!(
        (
            old.status.as_str(),
            old.prompt_version.as_deref(),
            old.model
        ),
        ("ready", Some("legacy"), None),
        "a legacy row keeps what it was: no model is invented for it"
    );
    let new = rig.row(&org, &fresh[0]).await.expect("state");
    assert_eq!(new.status, "ready");
    assert_eq!(
        (new.entities, new.relationships, new.claims),
        (Some(2), Some(1), Some(1))
    );
}

// ── what is never sent ───────────────────────────────────────────────────────

/// The organization is zero-retention: the bearer says so. No call, no spend, a marker on every
/// chunk, on the event path and on the reconciler, and the content never left.
async fn a_zero_retention_organization_gets_no_call_and_a_marker<P: Control>(rig: &P) {
    let org = rig.org("zdr");
    rig.parts().auth.is_zero_retention(&org);
    let chunks = rig
        .add_document(&org, "d", "org", "internal", &["one", "two", "three"])
        .await;
    rig.parts().inference.always(answer(RICH_EXTRACTION));

    let streamed = handle_document(rig, Origin::Stream, &org, &format!("{org}-d"))
        .await
        .expect("a document");
    assert_eq!(streamed.marked, 3);
    assert_eq!(calls(rig), 0, "zero provider calls");
    assert!(rig.parts().ledger.events().is_empty(), "nothing was spent");
    assert_eq!(
        statuses(rig, &org, &chunks).await,
        ["skipped_zdr/zdr_org"; 3],
        "a marker on every chunk, and it says WHY: the organization, not the document"
    );
    for chunk in &chunks {
        assert_eq!(rig.row(&org, chunk).await.expect("state").attempts, 0);
    }

    // The reconciler: the organization is withheld as a whole, new chunks are marked too.
    let later = rig
        .add_document(&org, "later", "org", "internal", &["four"])
        .await;
    let report = pass(rig, &org).await;
    assert_eq!(report.blocked, Some(Outcome::SkippedZdr(ZdrReason::Org)));
    assert_eq!(report.marked, 1, "only the chunk that was not marked yet");
    assert_eq!(statuses(rig, &org, &later).await, ["skipped_zdr/zdr_org"]);
    assert_eq!(calls(rig), 0);
}

/// The extractor itself refuses, so a caller that forgot to ask the gate cannot send either.
async fn the_extractor_refuses_a_zero_retention_organization_on_its_own<P: Control>(rig: &P) {
    let org = rig.org("zdr-direct");
    rig.parts().auth.is_zero_retention(&org);
    let error = rig
        .parts()
        .extractor
        .extract("a chunk of a zero-retention organization", &org)
        .await
        .expect_err("no call for a zero-retention organization");
    assert!(
        error
            .downcast_ref::<crate::extractor::ZdrOrganization>()
            .is_some(),
        "{error}"
    );
    assert_eq!(calls(rig), 0);
    assert!(rig.parts().ledger.events().is_empty());
}

/// Graph extraction turned off for an organization: no call, no bearer even, a marker, and the
/// work resumes the moment it is turned back on.
async fn a_policy_that_is_off_gets_no_call_and_a_marker_and_resumes_when_turned_on<P: Control>(
    rig: &P,
) {
    let org = rig.org("policy");
    let chunks = rig
        .add_document(&org, "d", "org", "internal", &["one", "two"])
        .await;
    rig.set_policy(&org, "off", "inherit", None).await;
    rig.parts().inference.always(answer(RICH_EXTRACTION));

    let report = pass(rig, &org).await;
    assert_eq!(report.blocked, Some(Outcome::SkippedPolicy));
    assert_eq!(calls(rig), 0);
    assert!(
        rig.parts()
            .auth
            .requested()
            .iter()
            .all(|(asked, _)| asked != &org),
        "an organization that turned graph extraction off needs no bearer"
    );
    assert_eq!(statuses(rig, &org, &chunks).await, ["skipped_policy"; 2]);

    rig.set_policy(&org, "on", "inherit", None).await;
    let report = pass(rig, &org).await;
    assert_eq!(report.healed, 1);
    assert_eq!(
        calls(rig),
        2,
        "the chunks that were skipped are extracted at once"
    );
    assert_eq!(statuses(rig, &org, &chunks).await, ["ready"; 2]);
}

/// A document that is private is not extracted (graph entities are organization-shared); one
/// classified restricted is never sent. Both are marked, and neither is selected by the scan.
async fn a_private_or_restricted_document_is_marked_and_never_sent<P: Control>(rig: &P) {
    let org = rig.org("standing");
    let private = rig
        .add_document(&org, "private", "private", "internal", &["a private chunk"])
        .await;
    let restricted = rig
        .add_document(
            &org,
            "restricted",
            "org",
            "restricted",
            &["a restricted chunk"],
        )
        .await;
    rig.parts().inference.always(answer(RICH_EXTRACTION));

    // The event path says why, document by document.
    handle_document(rig, Origin::Stream, &org, &format!("{org}-private"))
        .await
        .expect("the private document");
    handle_document(rig, Origin::Stream, &org, &format!("{org}-restricted"))
        .await
        .expect("the restricted document");
    assert_eq!(statuses(rig, &org, &private).await, ["skipped_visibility"]);
    assert_eq!(
        statuses(rig, &org, &restricted).await,
        ["skipped_zdr/zdr_document"],
        "a restricted document and a zero-retention organization stay distinguishable"
    );
    assert_eq!(calls(rig), 0);

    // The reconciler never selects either, and marks a restricted chunk it has not seen.
    let more = rig
        .add_document(&org, "restricted-too", "org", "restricted", &["another"])
        .await;
    let report = pass(rig, &org).await;
    assert_eq!((report.documents, report.marked), (0, 1));
    assert_eq!(
        statuses(rig, &org, &more).await,
        ["skipped_zdr/zdr_document"]
    );
    assert_eq!(calls(rig), 0);

    // The private document becomes visible to the organization: it is due at once.
    rig.set_visibility(&org, &format!("{org}-private"), "org")
        .await;
    let report = pass(rig, &org).await;
    assert_eq!(report.healed, 1);
    assert_eq!(calls(rig), 1);
    assert_eq!(statuses(rig, &org, &private).await, ["ready"]);
    // The restricted one is still not sent.
    assert_eq!(
        statuses(rig, &org, &restricted).await,
        ["skipped_zdr/zdr_document"]
    );
}

/// Auth Core cannot say what the organization's retention posture is (503
/// `retention_posture_unavailable`): the chunks wait. Never a send, never a skip for good, and
/// never an attempt.
async fn a_posture_auth_core_cannot_resolve_is_a_wait_that_never_counts<P: Control>(rig: &P) {
    let org = rig.org("posture");
    rig.parts().auth.cannot_resolve_posture_of(&org);
    let chunks = rig
        .add_document(&org, "d", "org", "internal", &["one", "two"])
        .await;
    rig.parts().inference.always(answer(RICH_EXTRACTION));

    let streamed = handle_document(rig, Origin::Stream, &org, &format!("{org}-d"))
        .await
        .expect("a document");
    assert_eq!(streamed.marked, 2);
    assert_eq!(calls(rig), 0, "not sent while the posture is unknown");
    assert_eq!(
        statuses(rig, &org, &chunks).await,
        ["pending/posture_unavailable"; 2]
    );
    let row = rig.row(&org, &chunks[0]).await.expect("state");
    assert_eq!(row.attempts, 0, "a wait is never an attempt");
    assert!(
        (row.retry_in_secs.expect("a wait") - 300).abs() <= 2,
        "five minutes"
    );

    // The reconciler waits too, however many times it asks.
    for _ in 0..8 {
        let report = pass(rig, &org).await;
        assert_eq!(
            report.blocked,
            Some(Outcome::PostureUnavailable {
                retry_after: std::time::Duration::from_secs(300)
            })
        );
    }
    for chunk in &chunks {
        let row = rig.row(&org, chunk).await.expect("state");
        assert_eq!(
            (row.status.as_str(), row.attempts),
            ("pending", 0),
            "never failed"
        );
    }

    // Auth Core comes back: the chunks are extracted and the first attempt is the first.
    rig.parts().auth.recovers(&org);
    for chunk in &chunks {
        rig.make_due(&org, chunk).await;
    }
    pass(rig, &org).await;
    assert_eq!(calls(rig), 2);
    for chunk in &chunks {
        let row = rig.row(&org, chunk).await.expect("state");
        assert_eq!((row.status.as_str(), row.attempts), ("ready", 1));
    }
}

// ── one chunk, one payment ───────────────────────────────────────────────────

/// The event consumer and the reconciler both reach a document; the second finds nothing to do.
async fn the_stream_and_the_reconciler_do_not_extract_the_same_chunk_twice<P: Control>(rig: &P) {
    let org = rig.org("twice");
    let chunks = rig
        .add_document(&org, "d", "org", "internal", &["one", "two", "three"])
        .await;
    rig.parts().inference.always(answer(RICH_EXTRACTION));

    let streamed = handle_document(rig, Origin::Stream, &org, &format!("{org}-d"))
        .await
        .expect("a document");
    assert_eq!((streamed.ready, streamed.not_due), (3, 0));
    assert_eq!(
        (streamed.entities, streamed.relationships, streamed.claims),
        (6, 3, 3)
    );
    assert_eq!(calls(rig), 3);

    let report = pass(rig, &org).await;
    assert_eq!(report.documents, 0, "nothing is due");
    let again = handle_document(rig, Origin::Stream, &org, &format!("{org}-d"))
        .await
        .expect("a redelivery");
    assert_eq!((again.ready, again.not_due), (0, 3));
    assert_eq!(calls(rig), 3, "one payment per chunk");
    assert_eq!(statuses(rig, &org, &chunks).await, ["ready"; 3]);
}

/// Another worker holds the chunk: it is not extracted a second time, and when that worker dies
/// the lease runs out and the chunk is due again with its first attempt already counted.
async fn a_chunk_held_by_another_worker_is_left_alone_until_its_lease_ends<P: Control>(rig: &P) {
    let org = rig.org("lease");
    let chunks = rig
        .add_document(&org, "d", "org", "internal", &["held"])
        .await;
    rig.parts().inference.always(answer(RICH_EXTRACTION));

    let held = rig.claim_chunk(&org, &chunks[0]).await.expect("a claim");
    assert_eq!(held, Some(Claim { attempts: 1 }));
    assert_eq!(
        rig.claim_chunk(&org, &chunks[0])
            .await
            .expect("a second claim"),
        None,
        "one chunk, one holder"
    );
    let row = rig.row(&org, &chunks[0]).await.expect("state");
    assert!(row.leased);

    let streamed = handle_document(rig, Origin::Stream, &org, &format!("{org}-d"))
        .await
        .expect("a document");
    assert_eq!((streamed.not_due, streamed.ready), (1, 0));
    assert_eq!(
        pass(rig, &org).await.documents,
        0,
        "the scan does not select it either"
    );
    assert_eq!(calls(rig), 0);

    // The holder died: its lease runs out.
    rig.make_due(&org, &chunks[0]).await;
    pass(rig, &org).await;
    assert_eq!(calls(rig), 1);
    let row = rig.row(&org, &chunks[0]).await.expect("state");
    assert_eq!(
        (row.status.as_str(), row.attempts, row.leased),
        ("ready", 2, false),
        "the dead worker's attempt was counted: a chunk that kills its worker is still bounded"
    );
}

// ── typed failures in the log ────────────────────────────────────────────────

/// "graph-index logged `%e` and hid it: the 8 live `model-plane inference failed` lines": the
/// gRPC code, the provider's kind and the decoded failure are fields of the event.
async fn the_typed_grpc_code_is_in_the_structured_log<P: Control>(rig: &P) {
    let org = rig.org("log");
    let chunks = rig
        .add_document(&org, "d", "org", "internal", &["one"])
        .await;
    rig.parts().inference.then(fail("provider_rate_limited"));
    let (logs, _guard) = capture_logs();

    pass(rig, &org).await;

    let failed = logs.events_saying(FAILED);
    assert_eq!(failed.len(), 1, "{:?}", logs.events());
    let event = &failed[0];
    let status = golden_status("provider_rate_limited");
    assert_eq!(event["level"], "WARN");
    assert_eq!(event["grpc_code"], format!("{:?}", status.code()));
    assert_eq!(event["provider_error"], "rate_limited");
    assert_eq!(event["failure"], "rate_limited");
    assert_eq!(event["stage"], "call");
    assert_eq!(event["error_class"], "rate_limited");
    assert_eq!(event["status"], "failed_transient");
    assert_eq!(event["attempts"], 1);
    assert_eq!(event["org_id"], org.as_str());
    assert_eq!(event["knowledge_id"], chunks[0].as_str());
    assert!(
        event["retry_in_secs"]
            .as_u64()
            .is_some_and(|secs| secs >= 300),
        "{event}"
    );
    assert!(
        logs.events().iter().all(|event| !event["fields"]["message"]
            .as_str()
            .is_some_and(|message| message == "model-plane inference failed")),
        "the old line that said nothing"
    );
    // Content-free: no document text, no prompt, no provider body reaches a log line.
    assert!(!logs.text().contains("one"), "the chunk's text is logged");
}

/// The same for a bearer Auth Core would not issue: the failure at the mint is a field too.
async fn a_posture_wait_is_logged_with_its_kind<P: Control>(rig: &P) {
    let org = rig.org("log-posture");
    rig.parts().auth.cannot_resolve_posture_of(&org);
    rig.add_document(&org, "d", "org", "internal", &["one"])
        .await;
    let (logs, _guard) = capture_logs();

    pass(rig, &org).await;

    let waits = logs.events_saying("graph extraction waits: no bearer for this organization");
    assert_eq!(waits.len(), 1, "{:?}", logs.events());
    assert_eq!(waits[0]["failure"], "posture_unavailable");
    assert_eq!(waits[0]["stage"], "mint");
    assert_eq!(waits[0]["provider_error"], "retention_posture_unavailable");
}

// ── metering ─────────────────────────────────────────────────────────────────

/// One event per paid call, with the exact tokens, under the feature of its kind; replaying a
/// pass publishes nothing new because nothing is paid for.
async fn every_paid_call_is_recorded_and_nothing_else<P: Control>(rig: &P) {
    let org = rig.org("meter");
    rig.add_document(&org, "d", "org", "internal", &["one", "two"])
        .await;
    rig.parts().inference.always(answer(RICH_EXTRACTION));

    pass(rig, &org).await;
    let events = rig.parts().ledger.events();
    assert_eq!(events.len(), 2, "one event per paid call");
    for event in &events {
        assert_eq!(
            (event.feature, event.event_type, event.org_id.as_str()),
            ("graph-extraction", "llm", org.as_str())
        );
        assert_eq!(
            (
                event.input_tokens,
                event.output_tokens,
                event.cache_read_tokens
            ),
            (612, 48, 512)
        );
        assert!(!event.zdr && !event.estimated);
    }
    assert_ne!(
        events[0].idempotency_key, events[1].idempotency_key,
        "two payments are two events"
    );

    pass(rig, &org).await;
    assert_eq!(
        rig.parts().ledger.events().len(),
        2,
        "nothing was paid for, nothing is recorded"
    );
}

/// A ledger that does not answer never fails the work, and says so loudly.
async fn a_ledger_that_cannot_be_reached_does_not_stop_extraction<P: Control>(rig: &P) {
    let org = rig.org("meter-down");
    let chunks = rig
        .add_document(&org, "d", "org", "internal", &["one"])
        .await;
    rig.parts().inference.always(answer(RICH_EXTRACTION));
    rig.parts().ledger.refuse_next(1);
    let (logs, _guard) = capture_logs();

    pass(rig, &org).await;

    assert_eq!(
        statuses(rig, &org, &chunks).await,
        ["ready"],
        "the chunk was extracted"
    );
    assert!(rig.parts().ledger.events().is_empty());
    assert_eq!(
        logs.events_saying(
            "SPEND NOT RECORDED: a usage event was not acknowledged by the ledger stream"
        )
        .len(),
        1
    );
}

// ── the budget ───────────────────────────────────────────────────────────────

/// The owner's ruling: configurable per organization, alert-only by default, `stop` explicit
/// (and then a marker, never a silent drop), `throttle` one chunk at a time.
async fn the_budget_alerts_by_default_and_stops_only_when_an_operator_says_stop<P: Control>(
    rig: &P,
) {
    rig.parts().inference.always(answer(RICH_EXTRACTION));

    // Over budget, mode inherited (alert-only): nothing is stopped, and it is said.
    let alert = rig.org("budget-alert");
    let alert_chunks = rig
        .add_document(&alert, "d", "org", "internal", &["one", "two"])
        .await;
    rig.set_policy(&alert, "inherit", "inherit", Some(10.0))
        .await;
    rig.set_spend_usd(&alert, 12.0).await;
    let (logs, _guard) = capture_logs();
    pass(rig, &alert).await;
    assert_eq!(statuses(rig, &alert, &alert_chunks).await, ["ready"; 2]);
    assert_eq!(
        logs.events_saying(
            "graph extraction: the background budget is exceeded; alert only, nothing is stopped"
        )
        .len(),
        1
    );

    // Over budget, stop: no call, a marker, and it resumes when the budget is raised.
    let stop = rig.org("budget-stop");
    let stop_chunks = rig
        .add_document(&stop, "d", "org", "internal", &["one", "two"])
        .await;
    rig.set_policy(&stop, "inherit", "stop", Some(10.0)).await;
    rig.set_spend_usd(&stop, 12.0).await;
    let before = calls(rig);
    let report = pass(rig, &stop).await;
    assert_eq!(report.blocked, Some(Outcome::SkippedBudget));
    assert_eq!(
        calls(rig),
        before,
        "a used-up budget in stop mode makes no call"
    );
    assert_eq!(
        statuses(rig, &stop, &stop_chunks).await,
        ["skipped_budget"; 2]
    );
    rig.set_policy(&stop, "inherit", "stop", Some(20.0)).await;
    pass(rig, &stop).await;
    assert_eq!(statuses(rig, &stop, &stop_chunks).await, ["ready"; 2]);

    // Over budget, throttle: one chunk per pass, never silently skipped.
    let throttle = rig.org("budget-throttle");
    let throttled = rig
        .add_document(&throttle, "d", "org", "internal", &["one", "two", "three"])
        .await;
    rig.set_policy(&throttle, "inherit", "throttle", Some(10.0))
        .await;
    rig.set_spend_usd(&throttle, 12.0).await;
    let before = calls(rig);
    pass(rig, &throttle).await;
    assert_eq!(calls(rig) - before, 1);
    pass(rig, &throttle).await;
    assert_eq!(calls(rig) - before, 2);
    assert_eq!(
        statuses(rig, &throttle, &throttled).await,
        ["ready", "ready", "<no state>"]
    );

    // Under budget, stop mode: it never fires.
    let under = rig.org("budget-under");
    let under_chunks = rig
        .add_document(&under, "d", "org", "internal", &["one"])
        .await;
    rig.set_policy(&under, "inherit", "stop", Some(10.0)).await;
    rig.set_spend_usd(&under, 3.0).await;
    pass(rig, &under).await;
    assert_eq!(statuses(rig, &under, &under_chunks).await, ["ready"]);
}

// ── the two rigs ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn memory_a_chunk_that_fails_ten_times_in_a_row_stops_after_the_cap() {
    a_chunk_that_fails_ten_times_in_a_row_stops_after_the_cap(&Memory::new().await).await;
}
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn database_a_chunk_that_fails_ten_times_in_a_row_stops_after_the_cap() {
    a_chunk_that_fails_ten_times_in_a_row_stops_after_the_cap(&Database::new().await).await;
}

#[tokio::test]
async fn memory_the_backoff_follows_the_ladder_on_the_rows() {
    the_backoff_follows_the_ladder_on_the_rows(&Memory::new().await).await;
}
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn database_the_backoff_follows_the_ladder_on_the_rows() {
    the_backoff_follows_the_ladder_on_the_rows(&Database::new().await).await;
}

#[tokio::test]
async fn memory_a_provider_that_is_down_is_asked_once() {
    a_provider_that_is_down_is_asked_once_and_the_rest_of_the_backlog_is_not_charged(
        &Memory::new().await,
    )
    .await;
}
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn database_a_provider_that_is_down_is_asked_once() {
    a_provider_that_is_down_is_asked_once_and_the_rest_of_the_backlog_is_not_charged(
        &Database::new().await,
    )
    .await;
}

#[tokio::test]
async fn memory_garbage_answers_stop_a_pass_after_three() {
    garbage_answers_stop_a_pass_after_three_and_are_charged_all_the_same(&Memory::new().await)
        .await;
}
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn database_garbage_answers_stop_a_pass_after_three() {
    garbage_answers_stop_a_pass_after_three_and_are_charged_all_the_same(&Database::new().await)
        .await;
}

#[tokio::test]
async fn memory_an_empty_result_is_durable_across_a_restart() {
    an_empty_result_is_durable_across_a_restart(&Memory::new().await).await;
}
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn database_an_empty_result_is_durable_across_a_restart() {
    an_empty_result_is_durable_across_a_restart(&Database::new().await).await;
}

#[tokio::test]
async fn memory_what_was_already_extracted_is_never_extracted_again() {
    what_was_already_extracted_is_never_extracted_again(&Memory::new().await).await;
}
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn database_what_was_already_extracted_is_never_extracted_again() {
    what_was_already_extracted_is_never_extracted_again(&Database::new().await).await;
}

#[tokio::test]
async fn memory_a_zero_retention_organization_gets_no_call_and_a_marker() {
    a_zero_retention_organization_gets_no_call_and_a_marker(&Memory::new().await).await;
}
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn database_a_zero_retention_organization_gets_no_call_and_a_marker() {
    a_zero_retention_organization_gets_no_call_and_a_marker(&Database::new().await).await;
}

#[tokio::test]
async fn memory_the_extractor_refuses_a_zero_retention_organization_on_its_own() {
    the_extractor_refuses_a_zero_retention_organization_on_its_own(&Memory::new().await).await;
}

#[tokio::test]
async fn memory_a_policy_that_is_off_gets_no_call_and_a_marker() {
    a_policy_that_is_off_gets_no_call_and_a_marker_and_resumes_when_turned_on(&Memory::new().await)
        .await;
}
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn database_a_policy_that_is_off_gets_no_call_and_a_marker() {
    a_policy_that_is_off_gets_no_call_and_a_marker_and_resumes_when_turned_on(
        &Database::new().await,
    )
    .await;
}

#[tokio::test]
async fn memory_a_private_or_restricted_document_is_marked_and_never_sent() {
    a_private_or_restricted_document_is_marked_and_never_sent(&Memory::new().await).await;
}
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn database_a_private_or_restricted_document_is_marked_and_never_sent() {
    a_private_or_restricted_document_is_marked_and_never_sent(&Database::new().await).await;
}

#[tokio::test]
async fn memory_a_posture_auth_core_cannot_resolve_is_a_wait_that_never_counts() {
    a_posture_auth_core_cannot_resolve_is_a_wait_that_never_counts(&Memory::new().await).await;
}
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn database_a_posture_auth_core_cannot_resolve_is_a_wait_that_never_counts() {
    a_posture_auth_core_cannot_resolve_is_a_wait_that_never_counts(&Database::new().await).await;
}

#[tokio::test]
async fn memory_the_stream_and_the_reconciler_do_not_extract_the_same_chunk_twice() {
    the_stream_and_the_reconciler_do_not_extract_the_same_chunk_twice(&Memory::new().await).await;
}
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn database_the_stream_and_the_reconciler_do_not_extract_the_same_chunk_twice() {
    the_stream_and_the_reconciler_do_not_extract_the_same_chunk_twice(&Database::new().await).await;
}

#[tokio::test]
async fn memory_a_chunk_held_by_another_worker_is_left_alone_until_its_lease_ends() {
    a_chunk_held_by_another_worker_is_left_alone_until_its_lease_ends(&Memory::new().await).await;
}
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn database_a_chunk_held_by_another_worker_is_left_alone_until_its_lease_ends() {
    a_chunk_held_by_another_worker_is_left_alone_until_its_lease_ends(&Database::new().await).await;
}

#[tokio::test]
async fn memory_the_typed_grpc_code_is_in_the_structured_log() {
    the_typed_grpc_code_is_in_the_structured_log(&Memory::new().await).await;
}
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn database_the_typed_grpc_code_is_in_the_structured_log() {
    the_typed_grpc_code_is_in_the_structured_log(&Database::new().await).await;
}

#[tokio::test]
async fn memory_a_posture_wait_is_logged_with_its_kind() {
    a_posture_wait_is_logged_with_its_kind(&Memory::new().await).await;
}

#[tokio::test]
async fn memory_every_paid_call_is_recorded_and_nothing_else() {
    every_paid_call_is_recorded_and_nothing_else(&Memory::new().await).await;
}
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn database_every_paid_call_is_recorded_and_nothing_else() {
    every_paid_call_is_recorded_and_nothing_else(&Database::new().await).await;
}

#[tokio::test]
async fn memory_a_ledger_that_cannot_be_reached_does_not_stop_extraction() {
    a_ledger_that_cannot_be_reached_does_not_stop_extraction(&Memory::new().await).await;
}

#[tokio::test]
async fn memory_the_budget_alerts_by_default_and_stops_only_when_an_operator_says_stop() {
    the_budget_alerts_by_default_and_stops_only_when_an_operator_says_stop(&Memory::new().await)
        .await;
}
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn database_the_budget_alerts_by_default_and_stops_only_when_an_operator_says_stop() {
    the_budget_alerts_by_default_and_stops_only_when_an_operator_says_stop(&Database::new().await)
        .await;
}

// ── in memory only: what needs the clock, or a race ──────────────────────────

/// A worker that dies mid-call leaves a lease that runs out by itself.
#[tokio::test]
async fn a_dead_workers_lease_expires_with_the_clock() {
    let rig = Memory::new().await;
    let org = rig.org("clock");
    let chunks = rig
        .add_document(&org, "d", "org", "internal", &["held"])
        .await;
    rig.parts().inference.always(answer(RICH_EXTRACTION));
    assert!(rig.claim_chunk(&org, &chunks[0]).await.unwrap().is_some());

    rig.advance(179);
    assert_eq!(
        pass(&rig, &org).await.documents,
        0,
        "still held at 179 s of 180"
    );
    rig.advance(2);
    pass(&rig, &org).await;
    assert_eq!(calls(&rig), 1);
    assert_eq!(rig.row(&org, &chunks[0]).await.unwrap().attempts, 2);
}

/// The document stops being visible to the organization between the claim and the write: nothing
/// is written, the call was paid for, and the chunk says why it has no graph.
#[tokio::test]
async fn a_document_that_becomes_invisible_mid_extraction_is_skipped_not_empty() {
    let rig = Memory::new().await;
    let org = rig.org("race");
    let chunks = rig
        .add_document(&org, "d", "org", "internal", &["one"])
        .await;
    rig.parts().inference.always(answer(RICH_EXTRACTION));
    rig.hide_documents_while_extracting();

    pass(&rig, &org).await;

    assert_eq!(
        statuses(&rig, &org, &chunks).await,
        ["skipped_visibility"],
        "not `empty`: an extraction that found nothing is a different statement"
    );
    assert_eq!(
        rig.row(&org, &chunks[0]).await.unwrap().attempts,
        0,
        "not the chunk's attempt"
    );
    assert_eq!(
        rig.parts().ledger.events().len(),
        1,
        "the call was paid for and is recorded"
    );
}

/// A chunk whose extraction is too long for the model cannot be fixed by trying again.
#[tokio::test]
async fn a_chunk_that_is_too_long_is_given_up_on_at_once_and_dead_lettered() {
    let rig = Memory::new().await;
    let org = rig.org("toolong");
    let chunks = rig
        .add_document(&org, "d", "org", "internal", &["a very long chunk"])
        .await;
    rig.parts().inference.always(fail("provider_too_long"));

    let report = pass(&rig, &org).await;

    assert_eq!(report.given_up, 1);
    assert_eq!(calls(&rig), 1);
    assert_eq!(
        statuses(&rig, &org, &chunks).await,
        ["failed_permanent/too_long"]
    );
    assert_eq!(rig.letters().len(), 1);
}

/// A usage limit that resets in fourteen hours parks the chunk for six, and a refusal to send
/// content (no attested zero-retention route) is a skip that never reaches `failed_permanent`.
#[tokio::test]
async fn what_the_provider_asked_for_is_obeyed_up_to_six_hours_and_a_refusal_is_never_a_failure() {
    let rig = Memory::new().await;
    let org = rig.org("provider-asks");
    let chunks = rig
        .add_document(&org, "d", "org", "internal", &["one", "two"])
        .await;
    rig.parts()
        .inference
        .then(fail("subscription_usage_limit_with_timing"))
        .then(fail("provider_zdr_unavailable"));

    pass(&rig, &org).await;
    let limited = rig.row(&org, &chunks[0]).await.unwrap();
    assert_eq!(limited.status, "failed_transient");
    assert_eq!(
        limited.retry_in_secs,
        Some(21_600),
        "capped at six hours, not fourteen"
    );

    // The refusal is a systemic outcome: the pass stopped before the second chunk.
    assert_eq!(statuses(&rig, &org, &chunks).await[1], "<no state>");
    rig.make_due(&org, &chunks[0]).await;
    pass(&rig, &org).await;
    let refused = rig.row(&org, &chunks[0]).await.unwrap();
    assert_eq!(
        (refused.status.as_str(), refused.error_class.as_deref(), refused.attempts),
        ("skipped_zdr", Some("zdr_unavailable"), 1),
        "inference-core's refusal to send is a skip, and the attempt is handed back (1 left of the 2 claims)"
    );
}

// ── community summaries ──────────────────────────────────────────────────────

/// A summary is a paid call like an extraction, and passes the same gate.
#[tokio::test]
async fn community_summaries_obey_the_policy_the_posture_and_the_budget() {
    let rig = Memory::new().await;
    rig.parts()
        .inference
        .always(answer("Hygiene solutions for the food industry."));
    let labels = ["Aquatiq", "Hygieneanlegg", "Næringsmiddelindustri"];

    // Policy off: nothing, and no bearer.
    let off = rig.org("sum-off");
    rig.add_community(&off, "c1", &labels);
    rig.set_policy(&off, "off", "inherit", None).await;
    assert_eq!(summarize_communities(&rig, &off, 5).await.unwrap(), 0);
    assert!(rig
        .parts()
        .auth
        .requested()
        .iter()
        .all(|(org, _)| org != &off));

    // A zero-retention organization: nothing, under its own bearer reason.
    let zdr = rig.org("sum-zdr");
    rig.parts().auth.is_zero_retention(&zdr);
    rig.add_community(&zdr, "c1", &labels);
    assert_eq!(summarize_communities(&rig, &zdr, 5).await.unwrap(), 0);
    assert!(rig
        .parts()
        .auth
        .requested()
        .contains(&(zdr.clone(), "graph-community-summary".to_owned())));

    // A posture Auth Core cannot resolve: a wait.
    let waiting = rig.org("sum-wait");
    rig.parts().auth.cannot_resolve_posture_of(&waiting);
    rig.add_community(&waiting, "c1", &labels);
    assert_eq!(summarize_communities(&rig, &waiting, 5).await.unwrap(), 0);

    // Budget stop: nothing. Alert-only (the default): proceeds, and says so. Throttle: one.
    let stop = rig.org("sum-stop");
    rig.add_community(&stop, "c1", &labels);
    rig.set_policy(&stop, "inherit", "stop", Some(10.0)).await;
    rig.set_spend_usd(&stop, 12.0).await;
    assert_eq!(summarize_communities(&rig, &stop, 5).await.unwrap(), 0);
    assert_eq!(calls(&rig), 0, "not one call so far");

    let (logs, _guard) = capture_logs();
    let alert = rig.org("sum-alert");
    for id in ["c1", "c2", "c3"] {
        rig.add_community(&alert, id, &labels);
    }
    rig.set_policy(&alert, "inherit", "alert_only", Some(10.0))
        .await;
    rig.set_spend_usd(&alert, 12.0).await;
    assert_eq!(summarize_communities(&rig, &alert, 5).await.unwrap(), 3);
    assert_eq!(rig.summaries().len(), 3, "the summaries were stored");
    assert_eq!(
        logs.events_saying(
            "graph extraction: the background budget is exceeded; alert only, nothing is stopped"
        )
        .len(),
        1
    );

    let throttle = rig.org("sum-throttle");
    for id in ["c1", "c2", "c3"] {
        rig.add_community(&throttle, id, &labels);
    }
    rig.set_policy(&throttle, "inherit", "throttle", Some(10.0))
        .await;
    rig.set_spend_usd(&throttle, 12.0).await;
    assert_eq!(summarize_communities(&rig, &throttle, 5).await.unwrap(), 1);

    // Each paid summary is metered under its own feature.
    let events = rig.parts().ledger.events();
    assert_eq!(events.len(), 4);
    assert!(events
        .iter()
        .all(|event| event.feature == "graph-community-summary"));
}

/// A provider that is down ends the run: one call, not one per community.
#[tokio::test]
async fn a_provider_failure_ends_a_summary_run() {
    let rig = Memory::new().await;
    let org = rig.org("sum-fail");
    for id in ["c1", "c2", "c3", "c4"] {
        rig.add_community(&org, id, &["Aquatiq", "Hygieneanlegg"]);
    }
    rig.parts().inference.always(fail("provider_unavailable"));
    let (logs, _guard) = capture_logs();

    assert_eq!(summarize_communities(&rig, &org, 5).await.unwrap(), 0);

    assert_eq!(calls(&rig), 1);
    let failed = logs.events_saying("community summarisation failed");
    assert_eq!(failed.len(), 1);
    assert_eq!(
        failed[0]["failure"], "transient",
        "the decoded kind; the provider's own kind and the code are fields of their own"
    );
    assert_eq!(
        failed[0]["grpc_code"],
        format!("{:?}", golden_status("provider_unavailable").code())
    );
}

// ── the gate's own decisions ─────────────────────────────────────────────────

#[test]
fn a_throttle_allowance_runs_out_and_an_unlimited_one_does_not() {
    let mut limited = Allowance::limited(2);
    assert!(!limited.exhausted());
    limited.take();
    limited.take();
    assert!(limited.exhausted());
    limited.take();
    assert!(limited.exhausted(), "it does not go below zero");
    let mut unlimited = Allowance::unlimited();
    for _ in 0..1_000 {
        unlimited.take();
    }
    assert!(!unlimited.exhausted());
}

#[test]
fn the_dead_letter_is_content_free_and_names_the_class() {
    let payload = DeadLetter {
        org_id: "org-a".to_owned(),
        document_id: "doc-1".to_owned(),
        knowledge_id: "k-1".to_owned(),
        error_class: ErrorClass::InvalidOutput,
        attempts: 6,
    }
    .payload();
    assert_eq!(payload["original_subject"], "dataplane.documents.indexed");
    assert_eq!(payload["error"], "invalid_output");
    assert_eq!(payload["error_class"], "invalid_output");
    assert_eq!(payload["attempts"], 6);
    assert_eq!(payload["prompt_version"], PROMPT_VERSION);
    assert_eq!(payload["reason"], "poison_cap");
    let keys: std::collections::BTreeSet<&str> = payload
        .as_object()
        .expect("an object")
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        [
            "attempts",
            "document_id",
            "error",
            "error_class",
            "knowledge_id",
            "org_id",
            "original_subject",
            "prompt_version",
            "reason"
        ]
        .into_iter()
        .collect(),
        "ids and a closed class: free text is where document content leaks"
    );
}
