//! Build script — compiles vendored .proto files into Rust gRPC clients
//! when the `grpc` feature is enabled.
//!
//! Without `grpc`, this file is a no-op so the crate builds without protoc.

use std::path::{Path, PathBuf};

fn main() {
    if std::env::var("CARGO_FEATURE_GRPC").is_err() {
        return;
    }

    let protos = ["proto/browser.proto", "proto/documents_v2.proto"];
    for p in &protos {
        println!("cargo:rerun-if-changed={p}");
    }

    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR must be set");

    // browser.proto / documents_v2.proto import `google/protobuf/timestamp.proto`
    // and `google/protobuf/struct.proto`. Those well-known types are not vendored
    // in this crate, so we must point protoc at the include directory that ships
    // alongside the system protoc (Debian apt: /usr/include; Homebrew: prefix/include).
    let mut includes: Vec<PathBuf> = vec![PathBuf::from("proto")];
    if let Some(wkt) = well_known_types_include() {
        println!("cargo:rerun-if-env-changed=PROTOC_INCLUDE");
        includes.push(wkt);
    }

    tonic_build::configure()
        .build_server(false)
        .build_client(true)
        .out_dir(&out_dir)
        .compile_protos(&protos, &includes)
        .expect("failed to compile protos — is protoc installed?");
}

fn well_known_types_include() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("PROTOC_INCLUDE") {
        let p = PathBuf::from(dir);
        if p.join("google/protobuf/timestamp.proto").exists() {
            return Some(p);
        }
    }
    let candidates = [
        "/usr/include",
        "/usr/local/include",
        "/opt/homebrew/include",
        "/usr/local/opt/protobuf/include",
    ];
    candidates
        .iter()
        .map(Path::new)
        .find(|p| p.join("google/protobuf/timestamp.proto").exists())
        .map(PathBuf::from)
}
