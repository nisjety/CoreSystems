//! Test-only capture of `tracing` events, so a test can assert that a value never reaches a log
//! line (EP-10-S1, decision D4). Each event is kept as `LEVEL field=value ...`.

use std::sync::{Arc, Mutex};

#[derive(Default, Clone)]
pub(crate) struct CapturedLines(Arc<Mutex<Vec<String>>>);

impl CapturedLines {
    /// Every line captured so far, in emission order.
    pub(crate) fn lines(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
}

struct LineVisitor<'a>(&'a mut String);

impl tracing::field::Visit for LineVisitor<'_> {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        use std::fmt::Write;
        let _ = write!(self.0, "{}={value:?} ", field.name());
    }
}

impl tracing::Subscriber for CapturedLines {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        let mut line = format!("{} ", event.metadata().level());
        event.record(&mut LineVisitor(&mut line));
        self.0.lock().unwrap().push(line);
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}
