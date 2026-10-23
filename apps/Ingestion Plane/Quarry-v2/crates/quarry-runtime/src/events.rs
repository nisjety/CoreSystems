//! Page-level event emitter. Feeds SSE at edge + durable log at control,
//! and (when configured) fans every emit out to NATS JetStream so
//! cross-plane consumers (autocomplete-core, model-plane, org-core) see
//! events on a durable subject without going through control-plane HTTP.
//!
//! WS10.S3: a run opened with [`EventSink::open_run`] has its own trace. Its
//! `seq` is dense from the value given to `open_run` (1 for an agent run). Every
//! event is minted under one lock that also pushes it into a bounded replay ring
//! and broadcasts it, so a subscriber that replays from a `seq` and then tails the
//! live channel sees no gap and no duplicate. A run that was never opened keeps
//! the global counter and has no entry in the trace map.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use chrono::Utc;
use dashmap::DashMap;
use tokio::sync::{broadcast, mpsc};

use quarry_core::event::{Event, EventType};
use quarry_core::ids::kinds::{EventKind, RunKind};
use quarry_core::zdr::ZdrMode;

use crate::event_bus::EventBus;

const BROADCAST_CAPACITY: usize = 256;

/// Estimated bytes an event costs in the ring beyond its payload and key.
const RETAINED_OVERHEAD_BYTES: usize = 192;

/// Default number of events an open run keeps for replay.
pub const DEFAULT_RING_EVENTS: usize = 512;
/// Default byte bound of one open run's replay ring (estimated).
pub const DEFAULT_RING_MAX_BYTES: usize = 1024 * 1024;

/// Bounds of one open run's replay ring. `max_events == 0` disables the ring:
/// nothing is retained, so a resume that missed events reports a gap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RingLimits {
    pub max_events: usize,
    pub max_bytes: usize,
}

impl Default for RingLimits {
    fn default() -> Self {
        Self {
            max_events: DEFAULT_RING_EVENTS,
            max_bytes: DEFAULT_RING_MAX_BYTES,
        }
    }
}

/// A resume asked for the events after `requested`, but the oldest event still in
/// the ring is `oldest_retained`: the events in between are gone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplayGap {
    pub requested: u64,
    pub oldest_retained: u64,
}

/// What a resuming subscriber gets: the retained events after its `from_seq` in
/// seq order, the gap when it missed events the ring no longer holds, and the
/// live tail.
pub struct ReplaySubscription {
    pub replay: Vec<Event>,
    pub gap: Option<ReplayGap>,
    pub rx: broadcast::Receiver<Event>,
}

struct RetainedEvent {
    event: Event,
    bytes: usize,
}

/// The trace of one open run: the next `seq` to mint and the replay ring.
struct RunTrace {
    next_seq: u64,
    ring: VecDeque<RetainedEvent>,
    ring_bytes: usize,
}

#[derive(Clone)]
pub struct EventSink {
    tx: mpsc::Sender<Event>,
    seq: Arc<AtomicU64>,
    subscribers: Arc<DashMap<RunKind, broadcast::Sender<Event>>>,
    /// Traces of open runs (WS10.S3). A run has an entry from `open_run` to `close_run`.
    runs: Arc<DashMap<RunKind, Arc<Mutex<RunTrace>>>>,
    ring: RingLimits,
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
            runs: Arc::new(DashMap::new()),
            ring: RingLimits::default(),
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

    /// Set the replay ring bounds for open runs (WS10.S3). Boot sets them from the
    /// trace configuration; a sink built without it uses [`RingLimits::default`].
    pub fn with_ring_limits(mut self, limits: RingLimits) -> Self {
        self.ring = limits;
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

    /// Open a run's trace (WS10.S3). Its events are numbered densely from
    /// `start_seq`, and the newest events within the ring limits are kept for
    /// replay until [`EventSink::close_run`].
    pub fn open_run(&self, run_id: &RunKind, start_seq: u64) {
        self.subscribers
            .entry(run_id.clone())
            .or_insert_with(|| broadcast::channel(BROADCAST_CAPACITY).0);
        self.runs.insert(
            run_id.clone(),
            Arc::new(Mutex::new(RunTrace {
                next_seq: start_seq,
                ring: VecDeque::new(),
                ring_bytes: 0,
            })),
        );
    }

    /// Close a run's trace (WS10.S3): the replay ring and the broadcast sender are
    /// dropped, so live subscribers see `Closed` after the events already sent.
    pub fn close_run(&self, run_id: &RunKind) {
        self.runs.remove(run_id);
        self.subscribers.remove(run_id);
        tracing::debug!(open_runs = self.runs.len(), "event trace closed");
    }

    /// Subscribe to a run's events after `from_seq` (exclusive): the retained events
    /// after it, then the live tail, with no gap and no duplicate between them.
    /// `None` when the run has no open trace.
    pub fn subscribe_with_replay(
        &self,
        run_id: &RunKind,
        from_seq: u64,
    ) -> Option<ReplaySubscription> {
        let trace = self.runs.get(run_id).map(|entry| entry.value().clone())?;
        // Taken under the trace lock: a mint cannot land between the snapshot and the
        // subscription, so the replay and the tail join without a gap or a repeat.
        let run = trace.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let rx = self
            .subscribers
            .get(run_id)
            .map(|entry| entry.value().subscribe())?;
        let oldest_retained = run
            .ring
            .front()
            .map_or(run.next_seq, |retained| retained.event.seq);
        let gap = (from_seq.saturating_add(1) < oldest_retained).then_some(ReplayGap {
            requested: from_seq,
            oldest_retained,
        });
        let replay = run
            .ring
            .iter()
            .filter(|retained| retained.event.seq > from_seq)
            .map(|retained| retained.event.clone())
            .collect();
        Some(ReplaySubscription { replay, gap, rx })
    }

    /// Number the event and record it. For an open run this is one critical section:
    /// the seq, the ring and the broadcast all happen under the run's lock.
    fn mint(
        &self,
        run_id: RunKind,
        event_type: EventType,
        payload: serde_json::Value,
        idempotency_key: String,
    ) -> Event {
        let trace = self.runs.get(&run_id).map(|entry| entry.value().clone());
        let Some(trace) = trace else {
            let seq = self.seq.fetch_add(1, Ordering::SeqCst);
            let evt = new_event(run_id.clone(), event_type, payload, idempotency_key, seq);
            self.broadcast(&run_id, &evt);
            return evt;
        };
        let mut run = trace.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let seq = run.next_seq;
        run.next_seq = seq.saturating_add(1);
        let evt = new_event(run_id.clone(), event_type, payload, idempotency_key, seq);
        if self.ring.max_events > 0 {
            retain(&mut run, evt.clone(), self.ring);
        }
        self.broadcast(&run_id, &evt);
        evt
    }

    fn broadcast(&self, run_id: &RunKind, evt: &Event) {
        if let Some(tx) = self
            .subscribers
            .get(run_id)
            .map(|entry| entry.value().clone())
        {
            let _ = tx.send(evt.clone());
        }
    }

    pub async fn emit(
        &self,
        run_id: RunKind,
        event_type: EventType,
        payload: serde_json::Value,
        idempotency_key: String,
    ) {
        let evt = self.mint(run_id, event_type, payload, idempotency_key);
        // Cross-plane fan-out via NATS (fire-and-forget). Spawned so the
        // handler returns immediately — broker latency / outage never
        // bottlenecks the local request path. The local mpsc → HTTP
        // publisher below remains the authoritative durable channel. The
        // transport gets the minted event itself, so its id and seq are the
        // ones the SSE path carries.
        if let Some(bus) = &self.nats {
            let bus = bus.clone();
            let for_bus = evt.clone();
            tokio::spawn(async move {
                if let Err(e) = bus.publish_event(for_bus).await {
                // zdr-log-ok: a NATS client error (subject, connection state) from a spawned fire-and-forget task; it never includes the event payload, which is passed to publish() and not echoed in the error
                    tracing::warn!(error = %e, "nats event publish failed");
                }
            });
        }
        let _ = self.tx.send(evt).await;
    }

    /// Emit only to in-process live subscribers. ZDR callers use this path so
    /// page content and provenance never enter the durable HTTP publisher or
    /// NATS JetStream while connected SSE/WebSocket clients still see progress.
    /// An open run keeps the event in its in-memory replay ring for the run's
    /// lifetime; the ring is never persisted.
    pub async fn emit_ephemeral(
        &self,
        run_id: RunKind,
        event_type: EventType,
        payload: serde_json::Value,
        idempotency_key: String,
    ) {
        let _ = self.mint(run_id, event_type, payload, idempotency_key);
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

fn new_event(
    run_id: RunKind,
    event_type: EventType,
    payload: serde_json::Value,
    idempotency_key: String,
    seq: u64,
) -> Event {
    Event {
        event_id: EventKind::new(),
        run_id: Some(run_id),
        job_id: None,
        event_type,
        ts: Utc::now(),
        seq,
        payload,
        idempotency_key,
    }
}

/// Keep the event in the ring, then evict the oldest events until both bounds hold.
fn retain(run: &mut RunTrace, event: Event, limits: RingLimits) {
    let bytes = event.payload.to_string().len() + event.idempotency_key.len() + RETAINED_OVERHEAD_BYTES;
    run.ring_bytes += bytes;
    run.ring.push_back(RetainedEvent { event, bytes });
    while !run.ring.is_empty()
        && (run.ring.len() > limits.max_events || run.ring_bytes > limits.max_bytes)
    {
        if let Some(evicted) = run.ring.pop_front() {
            run.ring_bytes -= evicted.bytes;
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
    use std::time::Duration;
    use tokio::sync::broadcast::error::RecvError;

    /// Records every event the sink hands to NATS, with the id and seq it carried.
    #[derive(Default)]
    struct RecordingBus {
        events: Mutex<Vec<Event>>,
    }

    #[async_trait]
    impl EventBus for RecordingBus {
        async fn publish(
            &self,
            _run_id: RunKind,
            _event_type: EventType,
            _payload: serde_json::Value,
            _idempotency_key: String,
        ) -> QuarryResult<()> {
            Ok(())
        }
        async fn publish_event(&self, event: Event) -> QuarryResult<()> {
            self.events.lock().unwrap().push(event);
            Ok(())
        }
        async fn subscribe(&self, _run_id: &RunKind) -> QuarryResult<Box<dyn EventReceiver>> {
            unimplemented!("RecordingBus does not support subscribe in this test")
        }
        async fn unsubscribe(&self, _run_id: &RunKind) -> QuarryResult<()> {
            Ok(())
        }
    }

    async fn wait_for_bus(bus: &RecordingBus, count: usize) -> Vec<Event> {
        for _ in 0..64 {
            if bus.events.lock().unwrap().len() >= count {
                break;
            }
            tokio::task::yield_now().await;
        }
        bus.events.lock().unwrap().clone()
    }

    fn sink_with(limits: RingLimits) -> (EventSink, mpsc::Receiver<Event>) {
        let (tx, rx) = mpsc::channel(4096);
        (EventSink::new(tx).with_ring_limits(limits), rx)
    }

    fn seqs(events: &[Event]) -> Vec<u64> {
        events.iter().map(|event| event.seq).collect()
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

        let recorded = wait_for_bus(&bus, 1).await;
        assert_eq!(recorded.len(), 1, "NATS publisher should see exactly one event");
        assert_eq!(recorded[0].event_type, EventType::HostDiscovered);
        assert_eq!(recorded[0].idempotency_key, "idem-host");
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
        assert!(
            wait_for_bus(&bus, 1).await.is_empty(),
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
        assert!(
            wait_for_bus(&bus, 1).await.is_empty(),
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

    #[tokio::test]
    async fn agent_run_seq_starts_at_one_and_is_dense() {
        let (sink, _rx) = sink_with(RingLimits::default());
        let run: RunKind = Id::new();
        sink.open_run(&run, 1);
        let mut live = sink.subscribe(&run);
        for _ in 0..3 {
            sink.emit_ephemeral(run.clone(), EventType::ActionStarted, json!({}), "k".into())
                .await;
        }
        let mut got = Vec::new();
        for _ in 0..3 {
            got.push(live.recv().await.expect("live event").seq);
        }
        assert_eq!(got, vec![1, 2, 3]);
    }

    #[tokio::test]
    async fn two_runs_have_independent_seq() {
        let (sink, _rx) = sink_with(RingLimits::default());
        let a: RunKind = Id::new();
        let b: RunKind = Id::new();
        sink.open_run(&a, 1);
        sink.open_run(&b, 1);
        sink.emit_ephemeral(a.clone(), EventType::ActionStarted, json!({}), "a1".into())
            .await;
        sink.emit_ephemeral(a.clone(), EventType::ActionStarted, json!({}), "a2".into())
            .await;
        sink.emit_ephemeral(b.clone(), EventType::ActionStarted, json!({}), "b1".into())
            .await;
        assert_eq!(seqs(&sink.subscribe_with_replay(&a, 0).unwrap().replay), vec![1, 2]);
        assert_eq!(seqs(&sink.subscribe_with_replay(&b, 0).unwrap().replay), vec![1]);
    }

    #[tokio::test]
    async fn a_run_that_was_never_opened_keeps_the_global_counter_and_adds_no_map_entry() {
        let (sink, _rx) = sink_with(RingLimits::default());
        let run: RunKind = Id::new();
        let mut live = sink.subscribe(&run);
        sink.emit_ephemeral(run.clone(), EventType::RunStarted, json!({}), "u1".into())
            .await;
        sink.emit_ephemeral(run.clone(), EventType::RunStarted, json!({}), "u2".into())
            .await;
        assert_eq!(live.recv().await.unwrap().seq, 0);
        assert_eq!(live.recv().await.unwrap().seq, 1);
        assert!(!sink.runs.contains_key(&run), "an unopened run has no trace entry");
        assert!(sink.subscribe_with_replay(&run, 0).is_none());
    }

    #[tokio::test]
    async fn ring_is_bounded_by_events_and_by_bytes() {
        let run: RunKind = Id::new();

        let (by_count, _r1) = sink_with(RingLimits { max_events: 3, max_bytes: usize::MAX });
        by_count.open_run(&run, 1);
        for i in 0..5 {
            by_count
                .emit_ephemeral(run.clone(), EventType::ActionStarted, json!({ "i": i }), "k".into())
                .await;
        }
        assert_eq!(seqs(&by_count.subscribe_with_replay(&run, 0).unwrap().replay), vec![3, 4, 5]);

        // Each event costs about 303 estimated bytes (a 110-byte payload, a 1-byte key,
        // 192 overhead), so 700 holds two of them.
        let (by_bytes, _r2) = sink_with(RingLimits { max_events: 1000, max_bytes: 700 });
        by_bytes.open_run(&run, 1);
        for _ in 0..5 {
            by_bytes
                .emit_ephemeral(
                    run.clone(),
                    EventType::ActionStarted,
                    json!({ "pad": "x".repeat(100) }),
                    "k".into(),
                )
                .await;
        }
        assert_eq!(seqs(&by_bytes.subscribe_with_replay(&run, 0).unwrap().replay), vec![4, 5]);
    }

    #[tokio::test]
    async fn replay_returns_only_events_after_from_seq() {
        let (sink, _rx) = sink_with(RingLimits::default());
        let run: RunKind = Id::new();
        sink.open_run(&run, 1);
        for _ in 0..5 {
            sink.emit_ephemeral(run.clone(), EventType::ActionStarted, json!({}), "k".into())
                .await;
        }
        let resumed = sink.subscribe_with_replay(&run, 2).unwrap();
        assert_eq!(seqs(&resumed.replay), vec![3, 4, 5]);
        assert!(resumed.gap.is_none());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn replay_then_live_has_no_gap_or_duplicate_under_concurrent_emit() {
        const EMITTERS: u64 = 8;
        const PER_EMITTER: u64 = 50;
        const TOTAL: u64 = EMITTERS * PER_EMITTER;
        let (sink, _rx) = sink_with(RingLimits { max_events: 4096, max_bytes: usize::MAX });
        let run: RunKind = Id::new();
        sink.open_run(&run, 1);

        let mut emitters = Vec::new();
        for _ in 0..EMITTERS {
            let sink = sink.clone();
            let run = run.clone();
            emitters.push(tokio::spawn(async move {
                for _ in 0..PER_EMITTER {
                    sink.emit_ephemeral(run.clone(), EventType::ActionStarted, json!({}), "k".into())
                        .await;
                }
            }));
        }

        let subscription = sink.subscribe_with_replay(&run, 0).unwrap();
        let mut seen = seqs(&subscription.replay);
        let mut rx = subscription.rx;
        while seen.last().copied().unwrap_or(0) < TOTAL {
            let next = tokio::time::timeout(Duration::from_secs(10), rx.recv())
                .await
                .expect("the tail must not stall");
            match next {
                Ok(evt) => {
                    if evt.seq > seen.last().copied().unwrap_or(0) {
                        seen.push(evt.seq);
                    }
                }
                Err(RecvError::Lagged(_)) => {
                    let from = seen.last().copied().unwrap_or(0);
                    let resumed = sink.subscribe_with_replay(&run, from).unwrap();
                    seen.extend(seqs(&resumed.replay));
                    rx = resumed.rx;
                }
                Err(RecvError::Closed) => panic!("the run closed during the test"),
            }
        }
        for emitter in emitters {
            emitter.await.unwrap();
        }
        assert_eq!(seen, (1..=TOTAL).collect::<Vec<u64>>());
    }

    #[tokio::test]
    async fn replay_reports_a_gap_when_from_seq_is_older_than_the_ring() {
        let (sink, _rx) = sink_with(RingLimits { max_events: 2, max_bytes: usize::MAX });
        let run: RunKind = Id::new();
        sink.open_run(&run, 1);
        for _ in 0..5 {
            sink.emit_ephemeral(run.clone(), EventType::ActionStarted, json!({}), "k".into())
                .await;
        }
        let stale = sink.subscribe_with_replay(&run, 0).unwrap();
        assert_eq!(
            stale.gap,
            Some(ReplayGap { requested: 0, oldest_retained: 4 })
        );
        assert_eq!(seqs(&stale.replay), vec![4, 5]);
        // Resuming right before the oldest retained event misses nothing.
        let adjacent = sink.subscribe_with_replay(&run, 3).unwrap();
        assert!(adjacent.gap.is_none());
        assert_eq!(seqs(&adjacent.replay), vec![4, 5]);
    }

    #[tokio::test]
    async fn a_lagged_receiver_recovers_from_the_ring() {
        let (sink, _rx) = sink_with(RingLimits::default());
        let run: RunKind = Id::new();
        sink.open_run(&run, 1);
        let mut rx = sink.subscribe_with_replay(&run, 0).unwrap().rx;
        for _ in 0..300 {
            sink.emit_ephemeral(run.clone(), EventType::ActionStarted, json!({}), "k".into())
                .await;
        }
        assert!(matches!(rx.recv().await, Err(RecvError::Lagged(_))));
        let recovered = sink.subscribe_with_replay(&run, 0).unwrap();
        assert_eq!(recovered.replay.len(), 300);
        assert_eq!(recovered.replay.first().map(|e| e.seq), Some(1));
    }

    #[tokio::test]
    async fn close_run_closes_subscribers_and_frees_state() {
        let (sink, _rx) = sink_with(RingLimits::default());
        let run: RunKind = Id::new();
        sink.open_run(&run, 1);
        let mut rx = sink.subscribe_with_replay(&run, 0).unwrap().rx;
        sink.emit_ephemeral(run.clone(), EventType::AgentCompleted, json!({}), "done".into())
            .await;
        sink.close_run(&run);
        assert!(!sink.runs.contains_key(&run));
        assert!(!sink.subscribers.contains_key(&run));
        // The event sent before the close is still delivered, then the channel closes.
        assert_eq!(rx.recv().await.unwrap().seq, 1);
        assert!(matches!(rx.recv().await, Err(RecvError::Closed)));
    }

    #[tokio::test]
    async fn a_zdr_event_never_reaches_nats_or_mpsc_but_is_replayable_from_the_ring() {
        let (tx, mut rx) = mpsc::channel(8);
        let bus = Arc::new(RecordingBus::default());
        let bus_handle: Arc<dyn EventBus> = bus.clone();
        let sink = EventSink::new(tx).with_nats(bus_handle);
        let run: RunKind = Id::new();
        sink.open_run(&run, 1);
        let mut live = sink.subscribe_with_replay(&run, 0).unwrap().rx;

        sink.emit_for_zdr(
            ZdrMode::On,
            run.clone(),
            EventType::ActionStarted,
            json!({"sensitive": "stays-in-memory"}),
            "zdr-1".into(),
        )
        .await;

        assert_eq!(live.recv().await.unwrap().seq, 1);
        assert!(rx.try_recv().is_err(), "ZDR event entered the durable path");
        assert!(wait_for_bus(&bus, 1).await.is_empty(), "ZDR event entered NATS");
        let replayed = sink.subscribe_with_replay(&run, 0).unwrap().replay;
        assert_eq!(seqs(&replayed), vec![1]);
    }

    #[tokio::test]
    async fn nats_receives_the_same_event_id_and_seq_as_the_sse_path() {
        let (tx, mut rx) = mpsc::channel(8);
        let bus = Arc::new(RecordingBus::default());
        let bus_handle: Arc<dyn EventBus> = bus.clone();
        let sink = EventSink::new(tx).with_nats(bus_handle);
        let run: RunKind = Id::new();
        sink.open_run(&run, 1);

        sink.emit(run.clone(), EventType::ActionCompleted, json!({"n": 1}), "same".into())
            .await;

        let durable = rx.recv().await.expect("mpsc must receive the event");
        let recorded = wait_for_bus(&bus, 1).await;
        assert_eq!(recorded.len(), 1);
        let replayed = sink.subscribe_with_replay(&run, 0).unwrap().replay;
        assert_eq!(replayed[0].event_id, recorded[0].event_id);
        assert_eq!(replayed[0].seq, recorded[0].seq);
        assert_eq!(durable.event_id, recorded[0].event_id);
    }
}
