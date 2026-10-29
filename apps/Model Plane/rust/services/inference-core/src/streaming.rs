//! Token-by-token stream combiner using tokio channels.
//!
//! Bridges internal `mpsc::Receiver<InferChunk>` to tonic streaming response types.

use tokio::sync::mpsc;
use tonic::Status;

use crate::provider::InferChunk;

/// Convert an internal `InferChunk` receiver into a tonic-compatible stream.
///
/// Maps each `InferChunk` to the proto `InferChunk` message and wraps it in `Result<_, Status>`.
pub fn bridge_to_grpc(
    mut rx: mpsc::Receiver<InferChunk>,
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
            };
            if tx.send(Ok(proto_chunk)).await.is_err() {
                break;
            }
        }
    });

    grpc_rx
}
