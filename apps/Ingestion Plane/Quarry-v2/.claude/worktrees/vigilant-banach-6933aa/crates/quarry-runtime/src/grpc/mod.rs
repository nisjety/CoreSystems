//! gRPC clients for cross-plane RPC.
//!
//! Quarry calls:
//! - `model_plane.v1.BrowserBrokerService` (grant validation/issuance/revocation)
//! - `dataplane.documents.v2.DocumentService` (ingest path)
//!
//! Generated code lives in `OUT_DIR` and is included via `tonic::include_proto!`.
//! All wrappers expose strongly-typed `QuarryError` instead of leaking
//! `tonic::Status` to callers.

pub mod model_plane {
    pub mod v1 {
        tonic::include_proto!("model_plane.v1");
    }
}

pub mod dataplane {
    pub mod documents {
        pub mod v2 {
            tonic::include_proto!("dataplane.documents.v2");
        }
    }
}

pub mod browser_broker;
pub mod data_plane_client;
pub mod ingest_adapter;

pub use browser_broker::GrpcGrantValidator;
pub use data_plane_client::{DataPlaneIngestPolicy, GrpcDataPlaneClient};
pub use ingest_adapter::{into_dyn as ingest_adapter_into_dyn, GrpcIngestAdapter};
