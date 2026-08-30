/// Strip the Windows extended-length path prefix that `Path::canonicalize`
/// always adds.
///
/// `protoc` cannot read such a path: it reports the include directory as
/// "directory does not exist" and then `Cannot convert path ... to or from
/// Windows style` for every `.proto`, so the whole workspace fails to build on
/// Windows with `model_plane/v1/ids.proto: File not found` (reproduced with
/// libprotoc 35.1). Canonicalization itself is worth keeping — it resolves the
/// `../../../` and hands `protoc` an absolute include root — so only the prefix
/// goes. A no-op on Linux, where the prefix never appears, which is where CI
/// runs and why this went unnoticed.
fn strip_extended_prefix(path: std::path::PathBuf) -> std::path::PathBuf {
    let Some(rest) = path.to_str().and_then(|p| p.strip_prefix(r"\\?\")) else {
        return path;
    };
    // `\\?\UNC\server\share` denotes `\\server\share`; dropping only the
    // prefix would leave the bogus `UNC\server\share`.
    match rest.strip_prefix(r"UNC\") {
        Some(unc) => std::path::PathBuf::from(format!(r"\\{unc}")),
        None => std::path::PathBuf::from(rest),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let proto_root = strip_extended_prefix(
        std::path::Path::new(&manifest_dir)
            .join("../../../proto")
            .canonicalize()
            .expect("proto/ directory must exist relative to mp-contracts crate"),
    );

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
        // Listed explicitly even though orchestration.proto imports them:
        // relying on a transitive import is what let verified_outcome.proto
        // stay uncommitted and silently break the whole workspace build on a
        // clean checkout (fixed in cf117135).
        "model_plane/v1/verified_outcome.proto",
        "model_plane/v1/proof_bundle.proto",
        "model_plane/v1/finetune.proto",
        "model_plane/v1/routing_policy.proto",
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
