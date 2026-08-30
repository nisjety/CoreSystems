pub mod autonomy;
pub mod dataplane_posture;
pub mod skill_recovery;
pub mod tool_arguments;

/// Generated protobuf and gRPC types for the Model Plane.
///
/// Re-exports the `model_plane.v1` package as a Rust module tree.
pub mod model_plane {
    #[allow(
        clippy::default_trait_access,
        clippy::doc_markdown,
        clippy::missing_errors_doc,
        clippy::too_many_lines
    )]
    pub mod v1 {
        tonic::include_proto!("model_plane.v1");
    }
}

/// Generated protobuf and gRPC types for Data Plane v2.
pub mod dataplane {
    #[allow(
        clippy::default_trait_access,
        clippy::doc_markdown,
        clippy::missing_errors_doc,
        clippy::too_many_lines
    )]
    pub mod retrieval_v2 {
        tonic::include_proto!("dataplane.retrieval.v2");
    }

    #[allow(
        clippy::default_trait_access,
        clippy::doc_markdown,
        clippy::missing_errors_doc,
        clippy::too_many_lines
    )]
    pub mod documents_v2 {
        tonic::include_proto!("dataplane.documents.v2");
    }

    #[allow(
        clippy::default_trait_access,
        clippy::doc_markdown,
        clippy::missing_errors_doc,
        clippy::too_many_lines
    )]
    pub mod knowledge_v2 {
        tonic::include_proto!("dataplane.knowledge.v2");
    }

    #[allow(
        clippy::default_trait_access,
        clippy::doc_markdown,
        clippy::missing_errors_doc,
        clippy::too_many_lines
    )]
    pub mod graph_v1 {
        tonic::include_proto!("dataplane.graph.v1");
    }

    #[allow(
        clippy::default_trait_access,
        clippy::doc_markdown,
        clippy::missing_errors_doc,
        clippy::too_many_lines
    )]
    pub mod wiki_v1 {
        tonic::include_proto!("dataplane.wiki.v1");
    }
}
