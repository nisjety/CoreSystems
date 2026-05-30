//! EventBus abstraction — decouples event emission from transport.
//!
//! In-process: wraps the existing EventSink (mpsc + broadcast).
//! Future: NATS JetStream implementation for distributed deployments.

use async_trait::async_trait;
use serde_json::Value;

use quarry_core::event::EventType;
use quarry_core::ids::kinds::RunKind;
use quarry_core::QuarryResult;

use crate::events::EventSink;

#[async_trait]
pub trait EventBus: Send + Sync {
    async fn publish(
        &self,
        run_id: RunKind,
        event_type: EventType,
        payload: Value,
        idempotency_key: String,
    ) -> QuarryResult<()>;

    async fn subscribe(&self, run_id: &RunKind) -> QuarryResult<Box<dyn EventReceiver>>;

    async fn unsubscribe(&self, run_id: &RunKind) -> QuarryResult<()>;
}

#[async_trait]
pub trait EventReceiver: Send {
    async fn recv(&mut self) -> Option<quarry_core::event::Event>;
}

pub struct InProcessEventBus {
    sink: EventSink,
}

impl InProcessEventBus {
    pub fn new(sink: EventSink) -> Self {
        Self { sink }
    }

    pub fn sink(&self) -> &EventSink {
        &self.sink
    }
}

#[async_trait]
impl EventBus for InProcessEventBus {
    async fn publish(
        &self,
        run_id: RunKind,
        event_type: EventType,
        payload: Value,
        idempotency_key: String,
    ) -> QuarryResult<()> {
        self.sink
            .emit(run_id, event_type, payload, idempotency_key)
            .await;
        Ok(())
    }

    async fn subscribe(&self, run_id: &RunKind) -> QuarryResult<Box<dyn EventReceiver>> {
        let rx = self.sink.subscribe(run_id);
        Ok(Box::new(BroadcastReceiver(rx)))
    }

    async fn unsubscribe(&self, run_id: &RunKind) -> QuarryResult<()> {
        self.sink.unsubscribe(run_id);
        Ok(())
    }
}

struct BroadcastReceiver(tokio::sync::broadcast::Receiver<quarry_core::event::Event>);

#[async_trait]
impl EventReceiver for BroadcastReceiver {
    async fn recv(&mut self) -> Option<quarry_core::event::Event> {
        self.0.recv().await.ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarry_core::ids::Id;
    use serde_json::json;
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn in_process_bus_publish_and_receive() {
        let (tx, mut rx) = mpsc::channel(64);
        let sink = EventSink::new(tx);
        let bus = InProcessEventBus::new(sink);

        let run_id: RunKind = Id::new();
        let mut subscriber = bus.subscribe(&run_id).await.unwrap();

        bus.publish(
            run_id.clone(),
            EventType::RunStarted,
            json!({"test": true}),
            "idem-1".into(),
        )
        .await
        .unwrap();

        let event = subscriber.recv().await.unwrap();
        assert_eq!(event.event_type, EventType::RunStarted);
        assert_eq!(event.idempotency_key, "idem-1");

        let mpsc_event = rx.recv().await.unwrap();
        assert_eq!(mpsc_event.event_type, EventType::RunStarted);

        bus.unsubscribe(&run_id).await.unwrap();
    }

    #[tokio::test]
    async fn in_process_bus_unsubscribe_stops_receiver() {
        let (tx, _rx) = mpsc::channel(64);
        let sink = EventSink::new(tx);
        let bus = InProcessEventBus::new(sink);

        let run_id: RunKind = Id::new();
        let mut subscriber = bus.subscribe(&run_id).await.unwrap();

        bus.unsubscribe(&run_id).await.unwrap();

        let result = subscriber.recv().await;
        assert!(result.is_none());
    }
}
