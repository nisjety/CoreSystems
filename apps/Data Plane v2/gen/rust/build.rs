fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::configure()
        .build_server(false)
        .build_client(true)
        .compile_protos(
            &[
                "proto/retrieval_v2.proto",
                "proto/documents_v2.proto",
                "proto/knowledge_v2.proto",
                "proto/graph/v1/graph.proto",
                "proto/wiki/v1/wiki.proto",
            ],
            &["proto"],
        )?;
    Ok(())
}
