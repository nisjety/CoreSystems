//! Token-by-token stream combiner using tokio channels.
//!
//! Bridges internal `mpsc::Receiver<InferChunk>` to tonic streaming response types.

use tokio::sync::mpsc;
use tonic::Status;

use crate::provider::timing::HandlerClock;
use crate::provider::InferChunk;

/// Convert an internal `InferChunk` receiver into a tonic-compatible stream.
///
/// Maps each `InferChunk` to the proto `InferChunk` message and wraps it in `Result<_, Status>`.
/// `clock` is the handler's own: the final chunk's timing is re-anchored on
/// the handler's entry and its `handler` interval closed as it is forwarded.
pub fn bridge_to_grpc(
    mut rx: mpsc::Receiver<InferChunk>,
    clock: HandlerClock,
) -> mpsc::Receiver<Result<mp_contracts::model_plane::v1::InferChunk, Status>> {
    let (tx, grpc_rx) = mpsc::channel(64);

    tokio::spawn(async move {
        while let Some(chunk) = rx.recv().await {
            let proto_chunk = mp_contracts::model_plane::v1::InferChunk {
                request_id: chunk.request_id,
                delta: chunk.delta,
                done: chunk.done,
                model_used: chunk.model_used,
                input_tokens: chunk.input_tokens,
                output_tokens: chunk.output_tokens,
                stop_reason: chunk.stop_reason,
                reasoning_delta: chunk.reasoning_delta,
                // Stream provenance stamped by the chain (final chunk, and any
                // chunk the provider left unlabeled) so SSE consumers observe
                // exactly where the tokens were served from.
                provider_used: chunk.provider_used,
                residency: chunk.residency,
                // Present on the final chunk only, and only for providers that
                // report logprobs; `None` reaches consumers as "unknown".
                token_confidence: chunk.token_confidence.map(Into::into),
                // Prompt-cache telemetry (final chunk only, 0 otherwise) --
                // the streaming path's cache-hit observability.
                cache_read_input_tokens: chunk.cache_read_input_tokens,
                cache_creation_input_tokens: chunk.cache_creation_input_tokens,
                compaction_summary: chunk.compaction_summary,
                // Final chunk only (the chain stamps nothing else).
                provider_timing: chunk.provider_timing.map(|mut timing| {
                    clock.stamp(&mut timing);
                    timing.into()
                }),
            };
            if tx.send(Ok(proto_chunk)).await.is_err() {
                break;
            }
        }
    });

    grpc_rx
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::provider::ProviderTiming;

    fn chunk(done: bool, provider_timing: Option<ProviderTiming>) -> InferChunk {
        InferChunk {
            compaction_summary: String::new(),
            request_id: "req-bridge".to_owned(),
            delta: if done {
                String::new()
            } else {
                "hei".to_owned()
            },
            done,
            model_used: String::new(),
            input_tokens: 0,
            output_tokens: 0,
            stop_reason: String::new(),
            reasoning_delta: String::new(),
            provider_used: String::new(),
            residency: String::new(),
            token_confidence: None,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
            provider_timing,
        }
    }

    #[tokio::test(start_paused = true)]
    async fn the_final_chunks_timing_reaches_the_wire_on_the_handler_origin() {
        let mut clock = HandlerClock::start();
        // Authorization before the chain was entered.
        tokio::time::advance(Duration::from_millis(3)).await;
        clock.enter_chain();
        let (tx, rx) = mpsc::channel(4);
        let mut wire = bridge_to_grpc(rx, clock);

        tx.send(chunk(false, None)).await.expect("bridge is open");
        tokio::time::advance(Duration::from_millis(40)).await;
        let timing = ProviderTiming {
            routing: Duration::from_millis(2),
            provider: Duration::from_millis(38),
            first_token: Some(Duration::from_millis(20)),
            attempts: 1,
            ..ProviderTiming::default()
        };
        tx.send(chunk(true, Some(timing)))
            .await
            .expect("bridge is open");

        let delta = wire.recv().await.expect("delta").expect("ok chunk");
        assert!(
            delta.provider_timing.is_none(),
            "only the final chunk carries timing"
        );
        let last = wire.recv().await.expect("final").expect("ok chunk");
        let timing = last
            .provider_timing
            .expect("the final chunk's timing is copied");
        assert_eq!(timing.routing_ms, 5, "authorization counts as routing");
        assert_eq!(timing.handler_ms, 43);
        assert_eq!(timing.provider_ms, 38);
        assert_eq!(timing.first_token_ms, 20);
        assert_eq!(timing.attempts, 1);
        assert!(!timing.usage_reported);
    }
}
