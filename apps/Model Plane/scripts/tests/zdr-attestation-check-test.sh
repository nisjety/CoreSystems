#!/usr/bin/env bash
set -euo pipefail

# Contract test for scripts/zdr-attestation-check.sh (Z-40 attestation, pass 2 preflight) against a
# FIXTURE attestation. Every value below is invented and says so in its resource id
# ("fixture-non-production"): it is not a provider contract, it does not attest anything, and it
# must never be copied into a signed runtime public policy. The test proves what the operator check
# says about an Azure OpenAI claim (the surface the first real attestation covers, DEC-33 D5), and
# that it refuses what the release gate refuses. Real attestation needs a written contract
# (docs/ZDR.md Step 0); this script does not and cannot supply one.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CHECK="$ROOT_DIR/scripts/zdr-attestation-check.sh"
TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/zdr-attestation-check-test.XXXXXX")"
trap 'rm -rf "$TMP_DIR"' EXIT

TODAY=2026-10-23
FIXTURE_RESOURCE=/subscriptions/fixture-non-production/openai-attestation-fixture
FIXTURE_APPROVAL=FIXTURE-NOT-A-CONTRACT
FIXTURE_REVIEWER=fixture-reviewer@example.invalid

sha256_stdin() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum | awk '{print $1}'
  else
    shasum -a 256 | awk '{print $1}'
  fi
}

# The five-field digest, in the order provider/zdr.rs documents.
write_attestation() { # <surface> <effective> <review_by>
  local surface="$1" effective="$2" review_by="$3"
  printf '%s_ZDR_RESOURCE_ID=%s\n%s_ZDR_APPROVAL_REF=%s\n' "$surface" "$FIXTURE_RESOURCE" "$surface" "$FIXTURE_APPROVAL"
  printf '%s_ZDR_EFFECTIVE_DATE=%s\n%s_ZDR_REVIEW_BY=%s\n' "$surface" "$effective" "$surface" "$review_by"
  printf '%s_ZDR_REVIEWER=%s\n' "$surface" "$FIXTURE_REVIEWER"
  printf '%s_ZDR_DIGEST=%s\n' "$surface" \
    "$(printf '%s\n%s\n%s\n%s\n%s\n' "$FIXTURE_RESOURCE" "$FIXTURE_APPROVAL" "$effective" "$review_by" "$FIXTURE_REVIEWER" | sha256_stdin)"
}

# The check must refuse or accept, and say why in words an operator can act on.
expect_rejection() { # <name> <expected text> <command...>
  local name="$1" expected="$2" output
  shift 2
  output="$TMP_DIR/$name.out"
  if "$@" >"$output" 2>&1; then
    echo "zdr-attestation-check unexpectedly accepted $name" >&2
    exit 1
  fi
  grep -F -- "$expected" "$output" >/dev/null || {
    echo "zdr-attestation-check returned the wrong rejection for $name" >&2
    sed -n '1,10p' "$output" >&2
    exit 1
  }
}

expect_line() { # <name> <file> <exact line>
  grep -Fx -- "$3" "$2" >/dev/null || {
    echo "zdr-attestation-check output for $1 lacks the line: $3" >&2
    cat "$2" >&2
    exit 1
  }
}

policy() { # <file> then the lines on stdin
  cat >"$1"
  chmod 644 "$1"
}

# 1. A complete, in-force Azure OpenAI attestation: valid, days counted to the review date, and
#    the unclaimed surface is reported as such rather than invented.
valid="$TMP_DIR/valid.env"
{
  printf 'AZURE_OPENAI_ZDR_CONFIRMED=true\n'
  write_attestation AZURE_OPENAI 2026-10-01 2027-01-01
} | policy "$valid"
"$CHECK" "$valid" --today "$TODAY" >"$TMP_DIR/valid.out"
expect_line valid "$TMP_DIR/valid.out" 'AZURE_OPENAI: attestation valid, review by 2027-01-01 (70 days left)'
expect_line valid "$TMP_DIR/valid.out" 'AZURE_ANTHROPIC: no ZDR claim'

# 2. The review-by day itself is still inside the window (inference-core uses <=), and the
#    day after is not.
edge="$TMP_DIR/edge.env"
write_attestation AZURE_OPENAI 2026-10-01 "$TODAY" | policy "$edge"
"$CHECK" "$edge" --today "$TODAY" >"$TMP_DIR/edge.out"
expect_line edge "$TMP_DIR/edge.out" "AZURE_OPENAI: attestation valid, review by $TODAY (0 days left)"
expect_rejection edge-next-day "lapsed on $TODAY" "$CHECK" "$edge" --today 2026-10-24

# 3. Both surfaces claimed: each is reported on its own.
both="$TMP_DIR/both.env"
{
  write_attestation AZURE_OPENAI 2026-10-01 2027-01-01
  write_attestation AZURE_ANTHROPIC 2026-10-01 2026-12-01
} | policy "$both"
"$CHECK" "$both" --today "$TODAY" >"$TMP_DIR/both.out"
expect_line both "$TMP_DIR/both.out" 'AZURE_OPENAI: attestation valid, review by 2027-01-01 (70 days left)'
expect_line both "$TMP_DIR/both.out" 'AZURE_ANTHROPIC: attestation valid, review by 2026-12-01 (39 days left)'

# 4. No claim at all is a valid policy (the state before Z-40): both surfaces say so.
none="$TMP_DIR/none.env"
printf 'AZURE_OPENAI_ZDR_CONFIRMED=false\nINFERENCE_PROVIDER_ORDER=azure,anthropic,openai\n' | policy "$none"
"$CHECK" "$none" --today "$TODAY" >"$TMP_DIR/none.out"
expect_line none "$TMP_DIR/none.out" 'AZURE_OPENAI: no ZDR claim'
expect_line none "$TMP_DIR/none.out" 'AZURE_ANTHROPIC: no ZDR claim'

# 5. What it refuses, each fixture differing from the valid one in one way.
boolean_only="$TMP_DIR/boolean-only.env"
printf 'AZURE_OPENAI_ZDR_CONFIRMED=true\n' | policy "$boolean_only"
expect_rejection boolean-only 'ZDR attestation for AZURE_OPENAI is incomplete: missing or empty AZURE_OPENAI_ZDR_RESOURCE_ID' \
  "$CHECK" "$boolean_only" --today "$TODAY"

for field in RESOURCE_ID APPROVAL_REF EFFECTIVE_DATE REVIEW_BY REVIEWER DIGEST; do
  missing="$TMP_DIR/missing-$field.env"
  grep -v "^AZURE_OPENAI_ZDR_${field}=" "$valid" | policy "$missing"
  expect_rejection "missing-$field" "AZURE_OPENAI_ZDR_${field}" "$CHECK" "$missing" --today "$TODAY"
done

edited="$TMP_DIR/edited.env"
sed 's/^AZURE_OPENAI_ZDR_REVIEW_BY=.*/AZURE_OPENAI_ZDR_REVIEW_BY=2099-01-01/' "$valid" | policy "$edited"
expect_rejection edited 'AZURE_OPENAI_ZDR_DIGEST does not match the digest of the supplied fields' \
  "$CHECK" "$edited" --today "$TODAY"

lapsed="$TMP_DIR/lapsed.env"
write_attestation AZURE_OPENAI 2020-01-01 2020-06-01 | policy "$lapsed"
expect_rejection lapsed 'ZDR attestation for AZURE_OPENAI lapsed on 2020-06-01' "$CHECK" "$lapsed" --today "$TODAY"

future="$TMP_DIR/future.env"
write_attestation AZURE_OPENAI 2098-01-01 2099-01-01 | policy "$future"
expect_rejection future 'ZDR attestation for AZURE_OPENAI is not yet in force' "$CHECK" "$future" --today "$TODAY"

backwards="$TMP_DIR/backwards.env"
write_attestation AZURE_OPENAI 2026-10-01 2026-09-01 | policy "$backwards"
expect_rejection backwards 'is not after the effective date' "$CHECK" "$backwards" --today "$TODAY"

# 6. A secret has no business in the public policy the check reads, and the check never echoes a
#    field value on failure paths it does not need to.
hostile="$TMP_DIR/hostile.env"
printf 'AZURE_OPENAI_ZDR_CONFIRMED="true"\n' | policy "$hostile"
expect_rejection hostile 'restricted dotenv' "$CHECK" "$hostile" --today "$TODAY"
if grep -F -- "$FIXTURE_APPROVAL" "$TMP_DIR/valid.out" >/dev/null; then
  echo "zdr-attestation-check echoed the approval reference" >&2
  exit 1
fi

# 7. Usage: exactly one policy file, optional --today with a real date.
expect_rejection no-arguments 'usage:' "$CHECK"
expect_rejection bad-today '--today must be a YYYY-MM-DD date' "$CHECK" "$valid" --today 2026-02-30
expect_rejection missing-file '' "$CHECK" "$TMP_DIR/does-not-exist.env" --today "$TODAY"

# 8. The fixture says it is a fixture: nothing signed may carry it.
grep -F 'fixture-non-production' "$valid" >/dev/null || {
  echo "the fixture lost its non-production marker" >&2
  exit 1
}

echo "zdr attestation check contract (fixture, non-production): ok"
