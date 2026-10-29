//! Build script — compiles vendored .proto files into Rust gRPC clients
//! when the `grpc` feature is enabled.
//!
//! Without `grpc`, this file is a no-op so the crate builds without protoc.

fn main() {
    if std::env::var("CARGO_FEATURE_GRPC").is_err() {
        return;
    }

    let protos = ["proto/browser.proto", "proto/documents_v2.proto"];
    for p in &protos {
        println!("cargo:rerun-if-changed={p}");
    }

    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR must be set");

    tonic_build::configure()
        .build_server(false)
        .build_client(true)
        .out_dir(&out_dir)
        .compile_protos(&protos, &["proto"])
        .expect("failed to compile protos — is protoc installed?");
}
