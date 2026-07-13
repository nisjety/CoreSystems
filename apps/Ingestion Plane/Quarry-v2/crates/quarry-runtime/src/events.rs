//! Page-level event emitter. Feeds SSE at edge + durable log at control,
//! and (when configured) fans every emit out to NATS JetStream so
//! cross-plane consumers (autocomplete-core, model-plane, org-core) see
//! events on a durable subject without going through control-plane HTTP.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use chrono::Utc;
use dashmap::DashMap;
use tokio::sync::{broadcast, mpsc};

use quarry_core::event::{Event, EventType};
use quarry_core::ids::kinds::{EventKind, RunKind};
use quarry_core::zdr::ZdrMode;

use crate::event_bus::EventBus;

const BROADCAST_CAPACITY: usize = 256;

#[derive(Clone)]
pub struct EventSink {
    tx: mpsc::Sender<Event>,
    seq: Arc<AtomicU64>,
    subscribers: Arc<DashMap<RunKind, broadcast::Sender<Event>>>,
    /// Optional NATS JetStream publisher. When present, `emit()` also
    /// fires the event onto NATS via a non-blocking spawn so the local
    /// mpsc → control-plane HTTP path is never gated on the broker.
    /// Failures are warn-logged and swallowed — events are best-effort
    /// on the cross-plane bus; the HTTP path remains authoritative.
    nats: Option<Arc<dyn EventBus>>,
}

impl EventSink {
    pub fn new(tx: mpsc::Sender<Event>) -> Self {
        Self {
            tx,
            seq: Arc::new(AtomicU64::new(0)),
            subscribers: Arc::new(DashMap::new()),
            nats: None,
        }
    }

    /// Plug in a NATS-backed [`EventBus`] for cross-plane fan-out. After
    /// this, every `emit()` writes to the local mpsc AND publishes onto
    /// NATS (best-effort). Production wiring sets this in `main.rs` when
    /// `QUARRY_EDGE__NATS_URL` is configured.
    pub fn with_nats(mut self, bus: Arc<dyn EventBus>) -> Self {
        self.nats = Some(bus);
        self
    }

    /// Subscribe to per-run event fanout. Receiver sees events emitted
    /// after subscription until `unsubscribe` is called or the sender drops.
    pub fn subscribe(&self, run_id: &RunKind) -> broadcast::Receiver<Event> {
        self.subscribers
            .entry(run_id.clone())
            .or_insert_with(|| broadcast::channel(BROADCAST_CAPACITY).0)
            .subscribe()
    }

    /// Drop the broadcast sender for a run. Existing receivers will see `Closed`.
    pub fn unsubscribe(&self, run_id: &RunKind) {
        self.subscribers.remove(run_id);
    }

    pub async fn emit(
        &self,
        run_id: RunKind,
        event_type: EventType,
        payload: serde_json::Value,
        idempotency_key: String,
    ) {
        let seq = self.seq.fetch_add(1, Ordering::SeqCst);
        let evt = Event {
            event_id: EventKind::new(),
            run_id: Some(run_id.clone()),
            job_id: None,
            event_type,
            ts: Utc::now(),
            seq,
            payload: payload.clone(),
            idempotency_key: idempotency_key.clone(),
        };
        if let Some(rid) = &evt.run_id {
            if let Some(tx) = self.subscribers.get(rid) {
                let _ = tx.send(evt.clone());
            }
        }
        // Cross-plane fan-out via NATS (fire-and-forget). Spawned so the
        // handler returns immediately — broker latency / outage never
        // bottlenecks the local request path. The local mpsc → HTTP
        // publisher below remains the authoritative durable channel.
        if let Some(bus) = &self.nats {
            let bus = bus.clone();
            tokio::spawn(async move {
                if let Err(e) = bus
                    .publish(run_id, event_type, payload, idempotency_key)
                    .await
                {
                    tracing::warn!(error = %e, "nats event publish failed");
                }
            });
        }
        let _ = self.tx.send(evt).await;
    }

    /// Emit only to in-process live subscribers. ZDR callers use this path so
    /// page content and provenance never enter the durable HTTP publisher or
    /// NATS JetStream while connected SSE/WebSocket clients still see progress.
    pub async fn emit_ephemeral(
        &self,
        run_id: RunKind,
        event_type: EventType,
        payload: serde_json::Value,
        idempotency_key: String,
    ) {
        let seq = self.seq.fetch_add(1, Ordering::SeqCst);
        let evt = Event {
            event_id: EventKind::new(),
            run_id: Some(run_id.clone()),
            job_id: None,
            event_type,
            ts: Utc::now(),
            seq,
            payload,
            idempotency_key,
        };
        if let Some(tx) = self.subscribers.get(&run_id) {
            let _ = tx.send(evt);
        }
    }

    /// Route an event according to the request's effective ZDR posture.
    /// Restrictive events remain available to already-connected live clients,
    /// but never enter the durable HTTP publisher or NATS JetStream.
    pub async fn emit_for_zdr(
        &self,
        zdr: ZdrMode,
        run_id: RunKind,
        event_type: EventType,
        payload: serde_json::Value,
        idempotency_key: String,
    ) {
        if zdr.is_active() {
            self.emit_ephemeral(run_id, event_type, payload, idempotency_key)
                .await;
        } else {
            self.emit(run_id, event_type, payload, idempotency_key)
                .await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_bus::EventReceiver;
    use async_trait::async_trait;
    use quarry_core::ids::Id;
    use quarry_core::QuarryResult;
    use serde_json::json;
    use std::sync::Mutex;

    /// Records every publish call so the test can assert the sink fanned
    /// out to NATS in addition to the local mpsc.
    #[derive(Default)]
    struct RecordingBus {
        published: Mutex<Vec<(RunKind, EventType, String)>>,
    }

    #[async_trait]
    impl EventBus for RecordingBus {
        async fn publish(
            &self,
            run_id: RunKind,
            event_type: EventType,
            _payload: serde_json::Value,
            idempotency_key: String,
        ) -> QuarryResult<()> {
            self.published
                .lock()
                .unwrap()
                .push((run_id, event_type, idempotency_key));
            Ok(())
        }
        async fn subscribe(&self, _run_id: &RunKind) -> QuarryResult<Box<dyn EventReceiver>> {
            unimplemented!("RecordingBus does not support subscribe in this test")
        }
        async fn unsubscribe(&self, _run_id: &RunKind) -> QuarryResult<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn emit_without_nats_only_hits_mpsc() {
        let (tx, mut rx) = mpsc::channel(8);
        let sink = EventSink::new(tx);
        let run_id: RunKind = Id::new();
        sink.emit(
            run_id.clone(),
            EventType::RunStarted,
            json!({}),
            "idem-1".into(),
        )
        .await;

        let evt = rx.recv().await.expect("mpsc must receive event");
        assert_eq!(evt.event_type, EventType::RunStarted);
        assert_eq!(evt.idempotency_key, "idem-1");
    }

    #[tokio::test]
    async fn emit_with_nats_fans_out_to_both() {
        let (tx, mut rx) = mpsc::channel(8);
        let bus = Arc::new(RecordingBus::default());
        let bus_handle: Arc<dyn EventBus> = bus.clone();
        let sink = EventSink::new(tx).with_nats(bus_handle);
        let run_id: RunKind = Id::new();

        sink.emit(
            run_id.clone(),
            EventType::HostDiscovered,
            json!({"host": "example.com"}),
            "idem-host".into(),
        )
        .await;

        // mpsc path — authoritative durable channel — must always see
        // the event regardless of NATS state.
        let evt = rx.recv().await.expect("mpsc must receive event");
        assert_eq!(evt.event_type, EventType::HostDiscovered);

        // NATS path — fire-and-forget tokio::spawn, so give it a tick
        // to settle before asserting. The yield_now is enough because
        // the spawned future has no awaits before pushing to the Vec.
        for _ in 0..16 {
            if !bus.published.lock().unwrap().is_empty() {
                break;
            }
            tokio::task::yield_now().await;
        }
        let recorded = bus.published.lock().unwrap();
        assert_eq!(
            recorded.len(),
            1,
            "NATS publisher should see exactly one event"
        );
        assert_eq!(recorded[0].1, EventType::HostDiscovered);
        assert_eq!(recorded[0].2, "idem-host");
    }

    #[tokio::test]
    async fn emit_ephemeral_reaches_subscriber_without_durable_channels() {
        let (tx, mut rx) = mpsc::channel(8);
        let bus = Arc::new(RecordingBus::default());
        let bus_handle: Arc<dyn EventBus> = bus.clone();
        let sink = EventSink::new(tx).with_nats(bus_handle);
        let run_id: RunKind = Id::new();
        let mut subscriber = sink.subscribe(&run_id);

        sink.emit_ephemeral(
            run_id,
            EventType::PageFetched,
            json!({"url": "https://example.invalid"}),
            "zdr-ephemeral".into(),
        )
        .await;

        let evt = subscriber
            .recv()
            .await
            .expect("ephemeral subscriber must receive the live event");
        assert_eq!(evt.event_type, EventType::PageFetched);
        assert!(
            rx.try_recv().is_err(),
            "ZDR event must not enter HTTP publisher"
        );
        tokio::task::yield_now().await;
        assert!(
            bus.published.lock().unwrap().is_empty(),
            "ZDR event must not enter NATS JetStream"
        );
    }

    #[tokio::test]
    async fn emit_for_zdr_routes_restrictive_events_only_to_live_subscribers() {
        let (tx, mut rx) = mpsc::channel(8);
        let bus = Arc::new(RecordingBus::default());
        let bus_handle: Arc<dyn EventBus> = bus.clone();
        let sink = EventSink::new(tx).with_nats(bus_handle);
        let run_id: RunKind = Id::new();
        let mut subscriber = sink.subscribe(&run_id);

        sink.emit_for_zdr(
            quarry_core::zdr::ZdrMode::On,
            run_id,
            EventType::AgentStarted,
            json!({"sensitive": "must-stay-ephemeral"}),
            "zdr-agent-start".into(),
        )
        .await;

        assert!(subscriber.recv().await.is_ok());
        assert!(
            rx.try_recv().is_err(),
            "ZDR event entered durable HTTP path"
        );
        tokio::task::yield_now().await;
        assert!(
            bus.published.lock().unwrap().is_empty(),
            "ZDR event entered NATS JetStream"
        );
    }

    /// If the NATS bus errors, the mpsc path must still succeed — NATS
    /// is best-effort and should never bottleneck the local handler.
    #[tokio::test]
    async fn emit_continues_when_nats_publish_fails() {
        struct FailingBus;
        #[async_trait]
        impl EventBus for FailingBus {
            async fn publish(
                &self,
                _run_id: RunKind,
                _event_type: EventType,
                _payload: serde_json::Value,
                _idempotency_key: String,
            ) -> QuarryResult<()> {
                Err(quarry_core::QuarryError::new(
                    quarry_core::ErrorCode::DriverFailed,
                    "broker down",
                ))
            }
            async fn subscribe(&self, _run_id: &RunKind) -> QuarryResult<Box<dyn EventReceiver>> {
                unimplemented!()
            }
            async fn unsubscribe(&self, _run_id: &RunKind) -> QuarryResult<()> {
                Ok(())
            }
        }
        let (tx, mut rx) = mpsc::channel(8);
        let bus: Arc<dyn EventBus> = Arc::new(FailingBus);
        let sink = EventSink::new(tx).with_nats(bus);
        let run_id: RunKind = Id::new();

        sink.emit(run_id, EventType::RunStarted, json!({}), "idem-x".into())
            .await;

        let evt = rx
            .recv()
            .await
            .expect("mpsc must receive event even when NATS fails");
        assert_eq!(evt.event_type, EventType::RunStarted);
    }
}
