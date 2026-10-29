#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
LEDGER="$ROOT_DIR/docs/CAPABILITY_PROMOTION_LEDGER.tsv"
MIGRATIONS="$ROOT_DIR/go/services/capability-core/migrations"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

[[ -f "$LEDGER" ]] || {
  echo "missing capability promotion ledger: $LEDGER" >&2
  exit 1
}

header="$(head -n 1 "$LEDGER")"
expected_header=$'capability_id\tstate\tsource_revision\tcandidate_ref\tlive_evidence_ref\trollback_ref\texpires_at\treviewer\tnotes'
[[ "$header" == "$expected_header" ]] || {
  echo "capability promotion ledger header drifted" >&2
  exit 1
}

awk -F '\t' '
  NR == 1 { next }
  NF != 9 { printf "invalid ledger column count at line %d\n", NR; exit 1 }
  $1 !~ /^cap\.[a-z0-9_.-]+$/ { printf "invalid capability id at line %d\n", NR; exit 1 }
  $2 !~ /^(source_only|candidate|live_proven|allowlisted|rolled_back)$/ { printf "invalid state at line %d\n", NR; exit 1 }
  $2 == "source_only" && ($4 != "-" || $5 != "-" || $6 != "-" || $7 != "-") { printf "source-only row has promotion evidence at line %d\n", NR; exit 1 }
  $2 == "candidate" && ($4 == "-" || $5 != "-" || $6 != "-" || $7 == "-") { printf "candidate row has invalid evidence at line %d\n", NR; exit 1 }
  ($2 == "live_proven" || $2 == "allowlisted") && ($4 == "-" || $5 == "-" || $7 == "-") { printf "live row has incomplete evidence at line %d\n", NR; exit 1 }
  $2 == "allowlisted" && $6 == "-" { printf "allowlisted row has no rollback evidence at line %d\n", NR; exit 1 }
  { print $1 }
' "$LEDGER" | sort >"$TMP_DIR/ledger.ids"

rg -o "'cap\.[A-Za-z0-9_.-]+" "$MIGRATIONS" --glob '*.sql' |
  awk -F: '{ print $NF }' | tr -d "'" | sort -u >"$TMP_DIR/migration.ids"

if ! diff -u "$TMP_DIR/migration.ids" "$TMP_DIR/ledger.ids" >/dev/null; then
  echo "capability promotion ledger does not cover exactly the seeded migration IDs" >&2
  diff -u "$TMP_DIR/migration.ids" "$TMP_DIR/ledger.ids" >&2 || true
  exit 1
fi

ticket_state="$(awk -F '\t' '$1 == "cap.tool.ticket.create" { print $2 }' "$LEDGER")"
[[ "$ticket_state" == "source_only" ]] || {
  echo "tickets.create must remain source_only until owner and release evidence exists" >&2
  exit 1
}

echo "capability promotion ledger contracts: ok"
