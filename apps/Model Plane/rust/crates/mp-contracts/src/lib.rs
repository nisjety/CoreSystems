pub mod agent_mode;
pub mod agentic_subscription;
pub mod approval_cause;
pub mod approval_descriptor;
pub mod approval_edit;
pub mod attachments;
pub mod autonomy;
pub mod dataplane_posture;
pub mod durable_gate;
pub mod effect_ledger;
pub mod lifecycle_notice;
pub mod loop_fold;
pub mod loop_guard;
pub mod message_revision;
pub mod model_entitlement;
pub mod office_verification;
pub mod org_agent_policy;
pub mod park_reason;
pub mod prompt_tier;
pub mod provider_reads;
pub mod provider_writes;
pub mod queued_limits;
pub mod request_snapshot;
pub mod result_query;
pub mod retention;
pub mod run_context;
pub mod run_origin;
pub mod scheduled_contract;
pub mod skill_bundle;
pub mod skill_recovery;
pub mod space_attention;
pub mod space_approvals;
pub mod space_history;
pub mod space_human_posts;
pub mod space_memory;
pub mod space_message_lifecycle;
pub mod space_mentions;
pub mod space_timeline;
pub mod telemetry_labels;
pub mod tool_arguments;
pub mod tool_class;
pub mod user_question;
pub mod zdr_refusal;

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
