// Compile graph_v1 into tonic + prost types served by src/grpc.rs, plus the
// Model Plane InferenceCore CLIENT used by the extractor to route LLM
// entity/relationship extraction through inference-core (no direct Azure).
// Proto lives at `proto/graph/v1/graph.proto` (per-package subdir layout).
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let proto = "../../proto/graph/v1/graph.proto";
    let proto_dir = "../../proto";
    tonic_build::configure()
        .build_server(true)
        .build_client(false)
        .compile_protos(&[proto], &[proto_dir])?;
    println!("cargo:rerun-if-changed={proto}");

    // Model Plane inference client (model_plane.v1). Same cross-plane proto the
    // embedding-engine consumes, so the graph extractor speaks the identical
    // InferenceCore contract.
    let inference_proto = "../../../Model Plane/proto/model_plane/v1/inference.proto";
    let inference_dir = "../../../Model Plane/proto";
    tonic_build::configure()
        .build_server(false)
        .build_client(true)
        .compile_protos(&[inference_proto], &[inference_dir])?;
    println!("cargo:rerun-if-changed={inference_proto}");
    Ok(())
}
