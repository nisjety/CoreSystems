//! The three things a batch talks to that are not Postgres: the embedding provider,
//! the vector store and the message bus.
//!
//! They are traits for one reason: a test of the pipeline has to see exactly what
//! left the process, in what order, relative to the database writes, and a Qdrant
//! client, a NATS connection or a hosted embedding model cannot be made to say.
//! Nothing here decides anything. The production implementations are one line each
//! over the code that always did the work (`EmbeddingProvider::embed_batch`,
//! `qdrant_writer::upsert_vectors`, the NATS client's `publish`), and the recording
//! ones in the tests apply the same conversion to the same input
//! (`qdrant_writer::point_structs`), so what they record is the payload Qdrant would
//! have received, not a paraphrase of it. The rules that matter are not behind these
//! seams: the zero-retention egress guard of the embedding clients is tested where
//! it lives (`provider`), and the token client, the status decoder and the whole
//! context state machine run for real in the pipeline's tests.

use std::future::Future;
use std::pin::Pin;

use crate::provider::EmbeddingProvider;
use crate::qdrant_writer::{self, EmbeddingPoint};

pub(crate) type PortFuture<'a, T> = Pin<Box<dyn Future<Output = anyhow::Result<T>> + Send + 'a>>;

/// What turns text into vectors.
pub(crate) trait Embedder: Send + Sync {
    /// The model that produced the vectors. Recorded in `knowledge_units.embedding_model`
    /// and announced with `documents.indexed`.
    fn model_name(&self) -> &str;
    fn provider_name(&self) -> &str;
    fn embed_batch<'a>(
        &'a self,
        org_id: &'a str,
        texts: &'a [String],
        zdr: bool,
    ) -> PortFuture<'a, Vec<Vec<f32>>>;
}

impl Embedder for EmbeddingProvider {
    fn model_name(&self) -> &str {
        EmbeddingProvider::model_name(self)
    }

    fn provider_name(&self) -> &str {
        EmbeddingProvider::provider_name(self)
    }

    fn embed_batch<'a>(
        &'a self,
        org_id: &'a str,
        texts: &'a [String],
        zdr: bool,
    ) -> PortFuture<'a, Vec<Vec<f32>>> {
        Box::pin(EmbeddingProvider::embed_batch(self, org_id, texts, zdr))
    }
}

/// Where the vectors of a batch are stored.
pub(crate) trait VectorSink: Send + Sync {
    fn upsert<'a>(&'a self, collection: &'a str, points: Vec<EmbeddingPoint>)
        -> PortFuture<'a, ()>;
}

impl VectorSink for qdrant_client::Qdrant {
    fn upsert<'a>(
        &'a self,
        collection: &'a str,
        points: Vec<EmbeddingPoint>,
    ) -> PortFuture<'a, ()> {
        Box::pin(qdrant_writer::upsert_vectors(self, collection, points))
    }
}

/// Where a batch announces what it did (`documents.indexed`, the cost ledger).
pub(crate) trait EventSink: Send + Sync {
    fn publish<'a>(&'a self, subject: &'a str, payload: Vec<u8>) -> PortFuture<'a, ()>;
}

impl EventSink for async_nats::Client {
    fn publish<'a>(&'a self, subject: &'a str, payload: Vec<u8>) -> PortFuture<'a, ()> {
        Box::pin(async move {
            // The inherent method, not this one: the path says which.
            async_nats::Client::publish(self, subject.to_owned(), payload.into())
                .await
                .map_err(anyhow::Error::from)
        })
    }
}
