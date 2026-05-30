#!/usr/bin/env bash
set -euo pipefail

# Generate client stubs from Data Plane v2 proto definitions.
# Outputs:
#   gen/go/       — Go client + server stubs (for Model Plane import)
#   gen/rust/     — Rust client crate (dataplane-client-rs)

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(dirname "$SCRIPT_DIR")"
PROTO_DIR="$ROOT/proto"
GEN_GO="$ROOT/gen/go"
GEN_RUST="$ROOT/gen/rust"

PROTOS=(
  "$PROTO_DIR/retrieval_v2.proto"
  "$PROTO_DIR/documents_v2.proto"
  "$PROTO_DIR/knowledge_v2.proto"
  "$PROTO_DIR/graph/v1/graph.proto"
  "$PROTO_DIR/wiki/v1/wiki.proto"
)

echo "Generating client stubs from protos..."
echo "  Proto dir: $PROTO_DIR"
echo ""

# ─── Go Client ────────────────────────────────────────────────────

echo "─── Go ───"
rm -rf "$GEN_GO"
mkdir -p "$GEN_GO"

protoc \
  --proto_path="$PROTO_DIR" \
  --go_out="$GEN_GO" \
  --go_opt=paths=source_relative \
  --go-grpc_out="$GEN_GO" \
  --go-grpc_opt=paths=source_relative \
  "${PROTOS[@]}"

echo "  Generated Go stubs:"
find "$GEN_GO" -name "*.go" | while read -r f; do echo "    $f"; done

# Create go.mod if not present
if [ ! -f "$GEN_GO/go.mod" ]; then
  (cd "$GEN_GO" && go mod init github.com/triodelab/dataplane/gen/go && go mod tidy)
  echo "  Initialized go.mod"
fi

echo ""

# ─── Rust Client Crate ───────────────────────────────────────────

echo "─── Rust ───"
rm -rf "$GEN_RUST"
mkdir -p "$GEN_RUST/src" "$GEN_RUST/proto"

# Copy protos into the crate for self-contained build
cp "$PROTO_DIR"/retrieval_v2.proto "$GEN_RUST/proto/"
cp "$PROTO_DIR"/documents_v2.proto "$GEN_RUST/proto/"
cp "$PROTO_DIR"/knowledge_v2.proto "$GEN_RUST/proto/"
mkdir -p "$GEN_RUST/proto/graph/v1" "$GEN_RUST/proto/wiki/v1"
cp "$PROTO_DIR"/graph/v1/graph.proto "$GEN_RUST/proto/graph/v1/"
cp "$PROTO_DIR"/wiki/v1/wiki.proto "$GEN_RUST/proto/wiki/v1/"

cat > "$GEN_RUST/Cargo.toml" << 'TOML'
[package]
name = "dataplane-client"
version = "0.1.0"
edition = "2021"
description = "gRPC client stubs for Data Plane v2"

[dependencies]
tonic = "0.12"
prost = "0.13"
prost-types = "0.13"

[build-dependencies]
tonic-build = "0.12"
TOML

cat > "$GEN_RUST/build.rs" << 'RUST'
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
RUST

cat > "$GEN_RUST/src/lib.rs" << 'RUST'
pub mod retrieval {
    tonic::include_proto!("dataplane.retrieval.v2");
}

pub mod documents {
    tonic::include_proto!("dataplane.documents.v2");
}

pub mod knowledge {
    tonic::include_proto!("dataplane.knowledge.v2");
}

pub mod graph {
    tonic::include_proto!("dataplane.graph.v1");
}

pub mod wiki {
    tonic::include_proto!("dataplane.wiki.v1");
}
RUST

echo "  Created dataplane-client crate at $GEN_RUST"

# Verify it compiles
(cd "$GEN_RUST" && cargo check 2>&1) && echo "  Rust crate compiles ✓" || echo "  Rust crate compile failed ✗"

echo ""
echo "Done. Model Plane can import:"
echo "  Go:   github.com/triodelab/dataplane/gen/go"
echo "  Rust: path dep to gen/rust or publish dataplane-client crate"
