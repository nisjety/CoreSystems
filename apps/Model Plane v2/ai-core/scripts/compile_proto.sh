#!/usr/bin/env bash
# Compile ai_core.proto → Python stubs in app/grpc_gen/
# Run from the ai-core service root.
set -euo pipefail

PROTO_DIR="proto"
OUT_DIR="app/grpc_gen"

mkdir -p "$OUT_DIR"

python -m grpc_tools.protoc \
    -I "$PROTO_DIR" \
    --python_out="$OUT_DIR" \
    --pyi_out="$OUT_DIR" \
    --grpc_python_out="$OUT_DIR" \
    "$PROTO_DIR/ai_core.proto"

echo "✅  Proto stubs compiled to $OUT_DIR"
