//! model-gateway library crate — exposes modules for integration tests.

pub mod approvals;
pub mod auth;
pub mod budget;
pub mod coordinator;
pub mod dataplane;
pub mod finetune_azure;
pub mod finetune_poller;
pub mod finetune_routes;
pub mod gateway_metrics;
pub mod grpc;
pub mod http_routes;
pub mod langcache;
pub mod lsp;
pub mod nats_publisher;
pub mod normalize;
pub mod profile;
pub mod quarry;
pub mod rate_limit;
pub mod runtime_registries;
pub mod session_flow;
pub mod skills;
pub mod sse;
pub mod state;
pub mod stream_buffer;
pub mod tools;
pub mod trajectory;
