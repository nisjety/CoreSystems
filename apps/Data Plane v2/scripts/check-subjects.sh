#!/usr/bin/env bash
# Bash 4+ required for mapfile; on macOS install via `brew install bash` then
# run as `/opt/homebrew/bin/bash scripts/check-subjects.sh`. The CI image
# already ships bash 5.
# §17.3.3 — NATS subject contract lint.
#
# Walks the source tree, extracts every NATS subject string ("dataplane.*"),
# and asserts each appears in infra/nats/SUBJECTS.md. Drift fails the build.
#
# Also catches inline string usage outside the canonical constants by
# looking for `publish("dataplane.` patterns that aren't on a `const`
# line — those are the "anti-patterns" the contract file calls out.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONTRACT="$REPO_ROOT/infra/nats/SUBJECTS.md"

if [[ ! -f "$CONTRACT" ]]; then
    echo "FATAL: $CONTRACT missing" >&2
    exit 1
fi

# 1. Collect every "dataplane.*" string used in source.
#    Excludes the contract file itself, docs, tests, and the schema lock.
# Strip out `include_proto!("dataplane.foo.v2")` and similar proto-package
# matches before scanning — those are protobuf package identifiers, not
# NATS subjects, and the contract file would only get polluted by them.
found=$(
    grep -rEh '"dataplane\.[a-z0-9_.-]+"' \
        "$REPO_ROOT/services" \
        --include='*.rs' --include='*.go' 2>/dev/null \
        | grep -v 'include_proto!\|file_descriptor_set' \
        | grep -oE '"dataplane\.[a-z0-9_.-]+"' \
        | sort -u \
        | tr -d '"'
)

if [[ -z "$found" ]]; then
    echo "no dataplane.* subjects found — repo state is suspicious" >&2
    exit 1
fi

# 2. For each, assert it appears (somewhere) in the contract file.
missing=""
count=0
while IFS= read -r subj; do
    [[ -z "$subj" ]] && continue
    count=$((count + 1))
    if ! grep -qF "$subj" "$CONTRACT"; then
        missing="$missing  - $subj"$'\n'
    fi
done <<< "$found"

if [[ -n "$missing" ]]; then
    echo "FAIL: subjects used in source but NOT in $CONTRACT:" >&2
    printf '%s' "$missing" >&2
    echo "" >&2
    echo "Add them to infra/nats/SUBJECTS.md or remove the source reference." >&2
    exit 1
fi

# 3. Reject inline publish(...) with a literal string. Producers MUST
#    publish via a named constant. We exclude DLQ subjects, which are
#    constructed (DLQ_SUBJECT = "dataplane.dlq.<consumer>") and are
#    naturally const-named already.
inline_publish_violations=$(
    grep -rEn '\.[Pp]ublish\([^,]*"dataplane\.' \
        "$REPO_ROOT/services" \
        --include='*.rs' --include='*.go' 2>/dev/null \
        | grep -v 'SUBJECT_\|Subject\|DLQ_SUBJECT' || true
)
if [[ -n "$inline_publish_violations" ]]; then
    echo "FAIL: publish() with inline subject literal — use a named constant:" >&2
    echo "$inline_publish_violations" >&2
    exit 1
fi

echo "subject contract check passed ($count subjects)"
