#!/usr/bin/env bash
# Launcher for the vsv2 MCP server (stdio). Used by `claude mcp add`; safe to run by hand.
# Read-only by default: writes need VSV2_MCP_ENABLE_WRITES=1 (plus everything in docs/layeSystem/Layer4/layer4_mcp.md).
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"                                   # .env and config/{endpoints,tenants} are resolved from here
export PATH="/opt/homebrew/bin:/usr/local/bin:$PATH"   # `bq` for the warehouse tools
export VSV2_MCP_BQ_PROJECT="${VSV2_MCP_BQ_PROJECT:-aquatiq-datavarehus}"
BIN="$ROOT/target/release/vsv2-mcp"
[ -x "$BIN" ] || { echo "vsv2-mcp not built: run 'cargo build --release -p vsv2-mcp' in $ROOT" >&2; exit 1; }
exec "$BIN" "$@"
