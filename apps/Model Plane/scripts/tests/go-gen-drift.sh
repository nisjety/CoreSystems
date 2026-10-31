#!/usr/bin/env bash
# Drift guard for the checked-in Model Plane Go bindings (go/gen/model_plane).
#
# Regenerates the model_plane protos into a temporary directory with the native
# buf and the local protoc-gen-go and protoc-gen-go-grpc plugins, using the same
# managed-mode and paths=source_relative setup as proto/buf.gen.go-only.yaml.
# Every generated *.go file is then compared with its checked-in copy, ignoring
# CR characters because go/gen is checked out as CRLF on Windows.
#
# A checked-in *.pb.go that no proto generates any more is reported as well, so
# a removed or renamed proto cannot leave stale bindings behind. Hand-written
# *_test.go files in the same directory are not bindings and are not checked.
#
# Prints "DRIFT <file>" (relative to go/gen) for each difference and exits 1 if
# there is any. Exits 2 when a tool is missing, buf fails or generates nothing.
# It never writes into go/gen, and the temporary directory is removed on exit.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MODEL_PLANE_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
PROTO_DIR="$MODEL_PLANE_DIR/proto"
GEN_DIR="$MODEL_PLANE_DIR/go/gen"
GEN_PACKAGE="model_plane"

die() {
  echo "go-gen-drift: $*" >&2
  exit 2
}

for tool in buf protoc-gen-go protoc-gen-go-grpc; do
  command -v "$tool" >/dev/null 2>&1 || die "$tool is not on PATH"
done
[[ -d "$GEN_DIR/$GEN_PACKAGE" ]] || die "missing $GEN_DIR/$GEN_PACKAGE"

# buf is a native Windows binary under Git Bash, so the paths it reads and
# writes must be Windows paths. cygpath is absent on Linux and macOS, where the
# POSIX path is already the native one.
native_path() {
  if command -v cygpath >/dev/null 2>&1; then
    cygpath -m "$1"
  else
    printf '%s\n' "$1"
  fi
}

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

OUT_DIR="$TMP_DIR/out"
TEMPLATE="$TMP_DIR/buf.gen.yaml"
mkdir -p "$OUT_DIR"

cat >"$TEMPLATE" <<EOF
version: v2
managed:
  enabled: true
plugins:
  - local: protoc-gen-go
    out: "$(native_path "$OUT_DIR")"
    opt:
      - paths=source_relative
  - local: protoc-gen-go-grpc
    out: "$(native_path "$OUT_DIR")"
    opt:
      - paths=source_relative
EOF

(
  cd "$PROTO_DIR"
  buf generate --template "$(native_path "$TEMPLATE")" --path "$GEN_PACKAGE"
) || die "buf generate failed; the guard could not run"

drift=0
generated=0
while IFS= read -r file; do
  rel="${file#"$OUT_DIR"/}"
  generated=$((generated + 1))
  if [[ ! -f "$GEN_DIR/$rel" ]] ||
    ! cmp -s <(tr -d '\r' <"$file") <(tr -d '\r' <"$GEN_DIR/$rel"); then
    echo "DRIFT $rel"
    drift=1
  fi
done < <(find "$OUT_DIR" -type f -name '*.go' | LC_ALL=C sort)
[[ "$generated" -gt 0 ]] || die "buf generated no Go files"

while IFS= read -r file; do
  rel="${file#"$GEN_DIR"/}"
  if [[ ! -f "$OUT_DIR/$rel" ]]; then
    echo "DRIFT $rel"
    drift=1
  fi
done < <(find "$GEN_DIR/$GEN_PACKAGE" -type f -name '*.pb.go' | LC_ALL=C sort)

if [[ "$drift" -ne 0 ]]; then
  exit 1
fi
echo "go-gen-drift: $generated generated Go files match go/gen/$GEN_PACKAGE"
