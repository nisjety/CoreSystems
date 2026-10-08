//! The reconciler's say over what a batch announces.
//!
//! The pipeline ends every batch by announcing, for each document with no unit left to embed,
//! `dataplane.documents.indexed`. That announcement is the keyword index's sync, and it is also
//! what graph extraction and the Meilisearch index run on: a re-announce of an unchanged document
//! makes graph-index extract it again (one provider call per chunk, until CR-T12 gives it a
//! hash to skip by) and makes Meilisearch clear and re-index it. So the reconciler announces a
//! document only when something the keyword index holds changed: a context was written for one
//! of its units. The re-embeds that change nothing the keyword index sees (verifying a `legacy`
//! unit, repairing a vector) and the attempts that failed are not announced.
//!
//! Whether a context was written is known only once the batch is over, and the pipeline decides to
//! publish inside the batch. So the batch is given this sink, which keeps the one subject back
//! until the reconciler has read the units and then either sends it or drops it. Every other
//! subject (the version 1 cost event, when the usage ledger is off) passes straight through: a
//! payment is never held back.
//!
//! The held payload is already signed by the pipeline's event signer, with the same issuer and
//! key as the pipeline's own announcement, and a signed envelope lives 120 seconds: a batch is
//! released within seconds of its end, never carried over.

use std::sync::Mutex;

use crate::batch::{EventSink, PortFuture};

/// The announcement of a document that is fully embedded (`infra/nats/SUBJECTS.md`).
const SUBJECT_DOC_INDEXED: &str = "dataplane.documents.indexed";

/// A sink that holds `documents.indexed` back and lets everything else through.
pub(crate) struct HeldSink<'a> {
    inner: &'a dyn EventSink,
    held: Mutex<Vec<Vec<u8>>>,
}

impl<'a> HeldSink<'a> {
    pub(crate) fn new(inner: &'a dyn EventSink) -> Self {
        Self {
            inner,
            held: Mutex::new(Vec::new()),
        }
    }

    /// How many announcements are being held.
    #[cfg(test)]
    pub(crate) fn held(&self) -> usize {
        self.held
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }

    /// Send what was held. A failure to publish is logged and does not fail the pass, exactly
    /// as it does not in the pipeline: the keyword index is re-synced by the next announcement of
    /// the document, and the parity report says if one never came.
    pub(crate) async fn release(&self) -> usize {
        let payloads = std::mem::take(
            &mut *self
                .held
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        let mut sent = 0;
        for payload in payloads {
            match self.inner.publish(SUBJECT_DOC_INDEXED, payload).await {
                Ok(()) => sent += 1,
                Err(error) => {
                    tracing::warn!(%error, "documents.indexed was not published by the reconciler")
                }
            }
        }
        sent
    }

    /// Throw away what was held.
    pub(crate) fn drop_held(&self) -> usize {
        std::mem::take(
            &mut *self
                .held
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
        .len()
    }
}

impl EventSink for HeldSink<'_> {
    fn publish<'a>(&'a self, subject: &'a str, payload: Vec<u8>) -> PortFuture<'a, ()> {
        if subject == SUBJECT_DOC_INDEXED {
            self.held
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(payload);
            return Box::pin(async { Ok(()) });
        }
        self.inner.publish(subject, payload)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    /// What was published: the subject and the payload of each.
    type Published = Arc<Mutex<Vec<(String, Vec<u8>)>>>;

    #[derive(Default)]
    struct Recorder(Published);

    impl EventSink for Recorder {
        fn publish<'a>(&'a self, subject: &'a str, payload: Vec<u8>) -> PortFuture<'a, ()> {
            self.0.lock().unwrap().push((subject.to_owned(), payload));
            Box::pin(async { Ok(()) })
        }
    }

    struct Failing;

    impl EventSink for Failing {
        fn publish<'a>(&'a self, _: &'a str, _: Vec<u8>) -> PortFuture<'a, ()> {
            Box::pin(async { anyhow::bail!("the broker is down") })
        }
    }

    #[tokio::test]
    async fn the_announcement_waits_to_be_released_and_everything_else_passes_through() {
        let recorder = Recorder::default();
        let seen = recorder.0.clone();
        let sink = HeldSink::new(&recorder);

        sink.publish("dataplane.cost.ledger", b"cost".to_vec())
            .await
            .expect("a cost event");
        sink.publish(SUBJECT_DOC_INDEXED, b"first".to_vec())
            .await
            .expect("held");
        sink.publish(SUBJECT_DOC_INDEXED, b"second".to_vec())
            .await
            .expect("held");
        assert_eq!(sink.held(), 2);
        let sent: Vec<_> = seen
            .lock()
            .unwrap()
            .iter()
            .map(|(s, _)| s.clone())
            .collect();
        assert_eq!(
            sent,
            ["dataplane.cost.ledger"],
            "a payment is never held back"
        );

        assert_eq!(sink.release().await, 2);
        assert_eq!(sink.held(), 0);
        let sent: Vec<_> = seen.lock().unwrap().clone();
        assert_eq!(
            sent[1..],
            [
                (SUBJECT_DOC_INDEXED.to_owned(), b"first".to_vec()),
                (SUBJECT_DOC_INDEXED.to_owned(), b"second".to_vec()),
            ],
            "released in the order they were held, on the same subject, bytes untouched"
        );
        assert_eq!(sink.release().await, 0, "released once");
    }

    #[tokio::test]
    async fn a_dropped_announcement_is_never_sent() {
        let recorder = Recorder::default();
        let seen = recorder.0.clone();
        let sink = HeldSink::new(&recorder);
        sink.publish(SUBJECT_DOC_INDEXED, b"x".to_vec())
            .await
            .expect("held");
        assert_eq!(sink.drop_held(), 1);
        assert_eq!(sink.release().await, 0);
        assert!(seen.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_broker_that_is_down_costs_the_announcement_and_nothing_else() {
        let sink = HeldSink::new(&Failing);
        sink.publish(SUBJECT_DOC_INDEXED, b"x".to_vec())
            .await
            .expect("held, so no failure yet");
        assert_eq!(sink.release().await, 0, "not published, and not an error");
        assert!(sink
            .publish("dataplane.cost.ledger", b"y".to_vec())
            .await
            .is_err());
    }
}
