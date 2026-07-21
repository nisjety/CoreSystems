pub mod api;
pub mod auth;
pub mod config;
pub mod gdpr;
pub mod gdpr_nats;
pub mod jobs;
pub mod model;
// Bulk rebuild primitives remain compiled for the future durable job runner,
// but no unauthenticated production route or event may invoke them.
pub mod quickwit;
pub mod rebuild;
pub mod stream;
