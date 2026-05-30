fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let proto_root = std::path::Path::new(&manifest_dir)
        .join("../../../proto")
        .canonicalize()
        .expect("proto/ directory must exist relative to mp-contracts crate");

    let mp_protos = &[
        "model_plane/v1/ids.proto",
        "model_plane/v1/events.proto",
        "model_plane/v1/gateway.proto",
        "model_plane/v1/sessions.proto",
        "model_plane/v1/runs.proto",
        "model_plane/v1/capabilities.proto",
        "model_plane/v1/sandboxes.proto",
        "model_plane/v1/browser.proto",
        "model_plane/v1/memory.proto",
        "model_plane/v1/inference.proto",
        "model_plane/v1/execution.proto",
        "model_plane/v1/orchestration.proto",
        "model_plane/v1/finetune.proto",
    ];

    let dp_protos = &[
        "dataplane/retrieval/v2/retrieval_v2.proto",
        "dataplane/documents/v2/documents_v2.proto",
        "dataplane/knowledge/v2/knowledge_v2.proto",
        "dataplane/graph/v1/graph_v1.proto",
        "dataplane/wiki/v1/wiki_v1.proto",
    ];

    let proto_paths: Vec<std::path::PathBuf> = mp_protos
        .iter()
        .chain(dp_protos.iter())
        .map(|p| proto_root.join(p))
        .collect();

    for p in &proto_paths {
        println!("cargo:rerun-if-changed={}", p.display());
    }
    println!("cargo:rerun-if-changed={}", proto_root.display());

    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .compile_protos(&proto_paths, &[&proto_root])?;

    Ok(())
}
