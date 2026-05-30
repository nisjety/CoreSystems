// Compile graph_v1 into tonic + prost types served by src/grpc.rs.
// Proto lives at `proto/graph/v1/graph.proto` (per-package subdir layout).
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let proto = "../../proto/graph/v1/graph.proto";
    let proto_dir = "../../proto";
    tonic_build::configure()
        .build_server(true)
        .build_client(false)
        .compile_protos(&[proto], &[proto_dir])?;
    println!("cargo:rerun-if-changed={proto}");
    Ok(())
}
