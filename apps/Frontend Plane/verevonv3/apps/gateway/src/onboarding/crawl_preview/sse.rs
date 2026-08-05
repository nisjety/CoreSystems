use axum::response::sse::Event;
use serde_json::Value;

pub(super) fn sse_json(event: &str, data: Value) -> Event {
    Event::default().event(event).data(data.to_string())
}
