pub mod document_svc;
pub mod interceptor;
pub mod knowledge_svc;
pub mod retrieval_svc;

pub mod pb_retrieval {
    tonic::include_proto!("dataplane.retrieval.v2");
}

pub mod pb_documents {
    tonic::include_proto!("dataplane.documents.v2");
}

pub mod pb_knowledge {
    tonic::include_proto!("dataplane.knowledge.v2");
}
