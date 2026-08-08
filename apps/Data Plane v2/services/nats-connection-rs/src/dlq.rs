//! D17 — the durable dead-letter stream for `dataplane.dlq.>`.
//!
//! Five Data Plane services publish dead-lettered work to `dataplane.dlq.*`
//! (`index-engine`, `embedding-engine`, `embedding-engine-page-images`,
//! `graph-index`, `quickwit-adapter`). Until this module existed **no
//! JetStream stream bound those subjects**, so every one of those publishes
//! was a plain core-NATS publish with no subscriber: the broker fanned it out
//! to nobody and destroyed it. The DLQ looked like a safety net in code and
//! was a black hole at runtime.
//!
//! This module owns the stream definition in exactly one place. Every service
//! that can publish a dead letter calls [`ensure_dlq_stream`] during startup;
//! the call is idempotent, so five callers converge on one stream rather than
//! five copy-pasted definitions drifting apart.
//!
//! Note that the publishers keep using core-NATS `publish`. That is
//! deliberate and sufficient: a JetStream stream whose subject filter matches
//! a core publish captures and persists it. What was missing was never the
//! publish call — it was the stream.

use std::time::Duration;

use async_nats::jetstream::{self, Context as JsContext};

/// The single dead-letter stream for the whole plane.
pub const DLQ_STREAM_NAME: &str = "DATAPLANE_DLQ";

/// Wildcard binding every per-service DLQ subject. `>` (not `*`) so a future
/// `dataplane.dlq.<service>.<detail>` subject is captured without a code
/// change — a DLQ that silently stops matching is the bug this closes.
pub const DLQ_SUBJECT_FILTER: &str = "dataplane.dlq.>";

/// 30 days. A dead letter is evidence of a production incident; it has to
/// outlive the weekend plus the investigation, not the 7 days the live
/// fan-out streams use.
pub const DLQ_MAX_AGE: Duration = Duration::from_secs(30 * 24 * 3600);

/// 1 GiB. Bounded on purpose: `Limits` retention never discards on
/// acknowledgement, so without a byte ceiling a stuck poison-message loop
/// could fill the JetStream store and take down every other stream with it.
/// With `DiscardPolicy::Old` the stream sheds its oldest dead letters instead
/// of refusing new ones — losing the oldest evidence beats losing the newest
/// plus the broker.
pub const DLQ_MAX_BYTES: i64 = 1024 * 1024 * 1024;

/// The stream configuration. Split out from [`ensure_dlq_stream`] so it can be
/// asserted in a unit test without a live broker — the retention policy in
/// particular is a correctness invariant, not a tuning knob.
pub fn dlq_stream_config() -> jetstream::stream::Config {
    jetstream::stream::Config {
        name: DLQ_STREAM_NAME.to_string(),
        subjects: vec![DLQ_SUBJECT_FILTER.to_string()],

        // ⚠️ RETENTION MUST STAY `Limits`. THIS IS THE WHOLE FIX. ⚠️
        //
        // `Interest` retention discards a message that has no *bound
        // consumer* at publish time. A DLQ has no consumer in steady state —
        // that is its entire purpose; `dlq-replay` is run by a human after an
        // incident, hours or days later. Under `Interest` this stream would
        // therefore throw away exactly the messages it exists to keep, and
        // would reproduce the original black hole while *looking* durable in
        // `nats stream ls`. That failure mode is worse than the bug it
        // replaced, because it is invisible.
        //
        // `Limits` keeps every message until `max_age` / `max_bytes`,
        // regardless of whether anything is listening. It is also the
        // async-nats default, so this line is written out explicitly rather
        // than left to `..Default::default()` — to make it a deliberate,
        // reviewable decision that nobody "tidies up" into `Interest` for
        // consistency with the fan-out streams (which are a different shape:
        // they always have consumers).
        //
        // Retention is immutable after stream creation, so getting this wrong
        // costs a delete-and-recreate of the incident evidence.
        retention: jetstream::stream::RetentionPolicy::Limits,

        max_age: DLQ_MAX_AGE,
        max_bytes: DLQ_MAX_BYTES,
        discard: jetstream::stream::DiscardPolicy::Old,

        // Explicit: a dead letter that only lived in RAM would not survive the
        // broker restart that often accompanies the incident that produced it.
        storage: jetstream::stream::StorageType::File,

        ..Default::default()
    }
}

/// Create or converge the DLQ stream. Idempotent and safe to call from every
/// service on every boot.
///
/// Returns an error only when the stream can neither be created nor read. A
/// stream that already exists with a *wrong but immutable* retention policy is
/// reported loudly (`tracing::error!`) rather than failing startup: refusing
/// to boot would take down a healthy indexing path over a dead-letter
/// misconfiguration, and the operator fix (delete + recreate the stream) is
/// manual either way.
pub async fn ensure_dlq_stream(js: &JsContext) -> Result<(), async_nats::Error> {
    let desired = dlq_stream_config();
    let mut stream = js.get_or_create_stream(desired).await?;
    let current = stream.info().await?.config.clone();

    if current.retention != jetstream::stream::RetentionPolicy::Limits {
        tracing_error_wrong_retention(&current.retention);
    }

    if let Some(upgraded) = upgraded_dlq_config(&current) {
        js.update_stream(upgraded).await?;
        tracing::info!(
            stream = DLQ_STREAM_NAME,
            "dead-letter stream limits converged"
        );
    }

    tracing::info!(
        stream = DLQ_STREAM_NAME,
        subjects = DLQ_SUBJECT_FILTER,
        retention = ?current.retention,
        "dead-letter stream ready"
    );
    Ok(())
}

/// [`ensure_dlq_stream`] with the "never block startup" policy applied, so the
/// five call sites are one identical line each instead of five hand-rolled
/// error arms that could drift into `?` and take a healthy service down over a
/// dead-letter stream.
pub async fn ensure_or_warn(js: &JsContext) {
    if let Err(error) = ensure_dlq_stream(js).await {
        tracing::error!(
            stream = DLQ_STREAM_NAME,
            %error,
            "dead-letter stream unavailable; dead letters published now would be DISCARDED"
        );
    }
}

fn tracing_error_wrong_retention(actual: &jetstream::stream::RetentionPolicy) {
    tracing::error!(
        stream = DLQ_STREAM_NAME,
        retention = ?actual,
        "DATAPLANE_DLQ exists with non-Limits retention; dead letters WILL be \
         discarded when no consumer is bound. Retention is immutable — delete \
         and recreate the stream to repair."
    );
}

/// Which of the *mutable* settings need converging on an already-existing
/// stream. Retention is deliberately absent: it cannot be updated in place,
/// and attempting it makes `update_stream` fail the whole call.
fn upgraded_dlq_config(current: &jetstream::stream::Config) -> Option<jetstream::stream::Config> {
    let mut upgraded = current.clone();
    let mut changed = false;

    if !upgraded
        .subjects
        .iter()
        .any(|subject| subject == DLQ_SUBJECT_FILTER)
    {
        upgraded.subjects.push(DLQ_SUBJECT_FILTER.to_string());
        changed = true;
    }
    // Only ever *lengthen* retention / *raise* the ceiling here. An operator
    // who deliberately widened them on a live broker should not have that
    // undone by the next deploy.
    if upgraded.max_age < DLQ_MAX_AGE {
        upgraded.max_age = DLQ_MAX_AGE;
        changed = true;
    }
    if upgraded.max_bytes < DLQ_MAX_BYTES {
        upgraded.max_bytes = DLQ_MAX_BYTES;
        changed = true;
    }

    changed.then_some(upgraded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dlq_retention_is_limits_not_interest() {
        // The single most important assertion in this crate. `Interest`
        // retention drops messages that have no bound consumer, which is the
        // steady state of a dead-letter queue, and would silently recreate the
        // black hole this stream exists to close.
        assert_eq!(
            dlq_stream_config().retention,
            jetstream::stream::RetentionPolicy::Limits
        );
        assert_ne!(
            dlq_stream_config().retention,
            jetstream::stream::RetentionPolicy::Interest
        );
        assert_ne!(
            dlq_stream_config().retention,
            jetstream::stream::RetentionPolicy::WorkQueue
        );
    }

    #[test]
    fn dlq_captures_every_current_publisher_subject() {
        let config = dlq_stream_config();
        assert_eq!(config.subjects, vec![DLQ_SUBJECT_FILTER.to_string()]);
        for subject in [
            "dataplane.dlq.index-engine",
            "dataplane.dlq.embedding-engine",
            "dataplane.dlq.embedding-engine-page-images",
            "dataplane.dlq.graph-index",
            "dataplane.dlq.quickwit-adapter",
        ] {
            assert!(
                subject_matches_filter(DLQ_SUBJECT_FILTER, subject),
                "{subject} is published today but would not be captured"
            );
        }
        // Guards the blast radius: the DLQ stream must not swallow live
        // traffic subjects, which would make them unroutable to their own
        // streams. The near-miss token is assembled at runtime so
        // `scripts/check-subjects.sh` does not mistake a negative test
        // fixture for a real subject that belongs in the contract file.
        let near_miss = format!("{}.{}.index-engine", "dataplane", "dlqx");
        for subject in [
            "dataplane.documents.created",
            "dataplane.knowledge.units.created",
            near_miss.as_str(),
        ] {
            assert!(!subject_matches_filter(DLQ_SUBJECT_FILTER, subject));
        }
    }

    #[test]
    fn dlq_is_bounded_and_outlives_an_incident() {
        let config = dlq_stream_config();
        assert_eq!(config.max_age, Duration::from_secs(30 * 24 * 3600));
        assert!(config.max_bytes > 0, "unbounded DLQ can fill the store");
        assert_eq!(config.discard, jetstream::stream::DiscardPolicy::Old);
        assert_eq!(config.storage, jetstream::stream::StorageType::File);
    }

    #[test]
    fn converged_stream_needs_no_update() {
        assert!(upgraded_dlq_config(&dlq_stream_config()).is_none());
    }

    #[test]
    fn narrower_existing_stream_is_widened() {
        let mut existing = dlq_stream_config();
        existing.subjects = vec!["dataplane.dlq.index-engine".to_string()];
        existing.max_age = Duration::from_secs(3600);
        existing.max_bytes = 1024;

        let upgraded = upgraded_dlq_config(&existing).expect("needs converging");
        assert!(upgraded
            .subjects
            .iter()
            .any(|subject| subject == DLQ_SUBJECT_FILTER));
        assert_eq!(upgraded.max_age, DLQ_MAX_AGE);
        assert_eq!(upgraded.max_bytes, DLQ_MAX_BYTES);
    }

    #[test]
    fn operator_widened_limits_are_not_narrowed_back() {
        let mut existing = dlq_stream_config();
        existing.max_age = DLQ_MAX_AGE * 2;
        existing.max_bytes = DLQ_MAX_BYTES * 4;
        assert!(upgraded_dlq_config(&existing).is_none());
    }

    #[test]
    fn retention_is_never_proposed_as_an_update() {
        // Retention is immutable on a live stream; including it in an
        // update_stream payload fails the entire call, which would leave the
        // subject/limit convergence unapplied too.
        let mut existing = dlq_stream_config();
        existing.retention = jetstream::stream::RetentionPolicy::Interest;
        existing.max_age = Duration::from_secs(60);

        let upgraded = upgraded_dlq_config(&existing).expect("limits still converge");
        assert_eq!(
            upgraded.retention,
            jetstream::stream::RetentionPolicy::Interest,
            "update payload must carry the existing retention untouched"
        );
    }

    /// Minimal NATS subject-filter matcher, used only to assert the wildcard
    /// covers what we think it covers.
    fn subject_matches_filter(filter: &str, subject: &str) -> bool {
        let mut filter_tokens = filter.split('.');
        let mut subject_tokens = subject.split('.');
        loop {
            match (filter_tokens.next(), subject_tokens.next()) {
                (Some(">"), Some(_)) => return true,
                (Some("*"), Some(_)) => continue,
                (Some(f), Some(s)) if f == s => continue,
                (None, None) => return true,
                _ => return false,
            }
        }
    }
}
