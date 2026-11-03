#!/usr/bin/env bash
# WS06-S10: the production-profile verification script and the execution-core code it measures
# hard-code the same numbers. This is a pure text contract (no docker, no cargo). It fails when the
# script stops reproducing the prlimit argv shape, the CPU hard gap, the bwrap flags, the exit codes
# or the compose keys that execution-core uses, so a change on either side cannot silently skew the
# calibration the script prints.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SCRIPT="$ROOT_DIR/scripts/verify-sandbox-production-profile.sh"
DISPATCH="$ROOT_DIR/scripts/verify-sandbox-isolation.sh"
BASEYML="$ROOT_DIR/deploy/docker-compose.yml"
POLICY="$ROOT_DIR/rust/services/execution-core/src/policy.rs"
SANDBOX="$ROOT_DIR/rust/services/execution-core/src/sandbox.rs"
EXECUTOR="$ROOT_DIR/rust/services/execution-core/src/executor.rs"
CI="$ROOT_DIR/../../.github/workflows/model-plane-rust-ci.yml"

fail() { echo "sandbox production-profile contract: $*" >&2; exit 1; }

# need FILE LITERAL LABEL: LITERAL (fixed string) must appear in FILE.
need() {
  grep -F -- "$2" "$1" >/dev/null || fail "$3 is missing from $(basename "$1"): $2"
}

# 1. Both scripts parse (the production profile needs docker, so this is the cheap gate).
bash -n "$SCRIPT" || fail "verify-sandbox-production-profile.sh does not parse"
bash -n "$DISPATCH" || fail "verify-sandbox-isolation.sh does not parse"

# 2. The dispatcher still routes --production-profile to the script.
need "$DISPATCH" "--production-profile" "the dispatcher"
need "$DISPATCH" "verify-sandbox-production-profile.sh" "the dispatcher"

# 3. The CPU hard gap: the script's CPU_GAP is execution-core's CPU_HARD_GAP_SECS.
script_gap="$(sed -n 's/^CPU_GAP=\([0-9][0-9]*\).*/\1/p' "$SCRIPT" | head -1)"
rust_gap="$(sed -n 's/^pub const CPU_HARD_GAP_SECS: u64 = \([0-9][0-9]*\);.*/\1/p' "$POLICY" | head -1)"
if [ -z "$script_gap" ] || [ "$script_gap" != "$rust_gap" ]; then
  fail "CPU_GAP ($script_gap) must equal CPU_HARD_GAP_SECS ($rust_gap)"
fi

# 4. The prlimit flags the script builds are the ones prlimit_args() renders, in the same wrapper shape.
for flag in '--cpu=' '--as=' '--fsize=' '--nofile=' '--core=0'; do
  need "$SCRIPT" "$flag" "the script's limits()"
done
for literal in '--cpu={}' '--as={}' '--fsize={}' '--nofile={}' '"--core=0"'; do
  need "$POLICY" "$literal" "prlimit_args()"
done
need "$SCRIPT" 'prlimit $lim -- bwrap' "the sandbox call"

# 5. Every bwrap flag in the script's BASE argv is a literal that assemble_argv emits.
for flag in --die-with-parent --unshare-pid --unshare-uts --unshare-ipc --new-session --dev --tmpfs \
            --ro-bind --clearenv --setenv --bind --unshare-net --chdir; do
  need "$SCRIPT" "$flag" "the script's BASE argv"
  need "$SANDBOX" "\"$flag\"" "assemble_argv"
done

# 6. The exit codes the script classifies are the ones classify_exit reads: SIGXCPU 24, SIGXFSZ 25.
need "$EXECUTOR" "const SIGXCPU: i32 = 24;" "classify_exit"
need "$EXECUTOR" "const SIGXFSZ: i32 = 25;" "classify_exit"
need "$SCRIPT" "152|24|137)" "the CPU spin classification"

# 7. The compose keys the script reads its profile from exist in the base file in the documented form.
for key in EXECUTION_SANDBOX_MAX_ADDRESS_SPACE_MB EXECUTION_SANDBOX_MAX_FILE_MB EXECUTION_SANDBOX_MAX_OPEN_FILES \
           EXECUTION_SANDBOX_CPU_FACTOR EXECUTION_SANDBOX_MAX_OUTPUT_BYTES; do
  need "$SCRIPT" "$key" "the script's profile reads"
  need "$BASEYML" "${key}: \${${key}:-" "the base compose file"
done

# 8. The CI job runs the production-profile mode of the isolation script.
need "$CI" "sandbox-production-profile:" "the CI workflow"
need "$CI" 'verify-sandbox-isolation.sh" --production-profile' "the CI workflow"

echo "sandbox production-profile contract passed"
