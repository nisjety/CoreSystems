#!/usr/bin/env bash
# =============================================================================
# run-postgres-contract-tests.sh  (CR-N1)
#
# Runs the Data Plane's Postgres-gated tests against a disposable Postgres 16 and
# FAILS, instead of silently skipping, when one of them does not execute.
#
# WHY THIS EXISTS
#   Before this runner the gated tests were compiled and then never run in CI:
#     * the Rust ones are `#[ignore = "requires TEST_DATABASE_URL ..."]`, and the
#       CI job that has a Postgres only runs retrieval-engine-rs, without
#       --include-ignored, against an empty database;
#     * the Go ones call t.Skip when TEST_DATABASE_URL is unset, and `go test`
#       then reports `ok` (measured: tests/contract/postgres exits 0 with 13
#       SKIP and 9 PASS when no database is reachable).
#   A contract that skips proves nothing. The 2026-10-01 live proof found six
#   defects that stubs and skipped suites had hidden.
#
# WHAT IT DOES (in order)
#   1. Validates TEST_DATABASE_URL: a LOCAL server, a database name ending in
#      _test, Postgres 16 or newer, a role that may CREATE DATABASE.
#   2. Builds tools/migrator from source and builds ONE template database the way
#      production does: infra/postgres/init.sql, then every migration in version
#      order through the real migrator. It then asserts that every migration file
#      is recorded in schema_migrations (a file the migrator silently ignores is
#      a failure, not a skip).
#   3. For each registered suite it creates fresh databases and runs the suite
#      with REQUIRE_CONTRACT=1, then asserts that the gated tests really ran:
#        Rust: every `#[ignore = "requires ...TEST_DATABASE_URL..."]` test found in
#              the crate source is also listed by libtest, is run with --exact,
#              and must report `ok` (a marker whose test is not compiled fails).
#        Go:   a first pass WITHOUT any database variable defines the gated set
#              (the tests that SKIP because a *DATABASE_URL is unset); a second
#              pass WITH the database must PASS every one of them, with no
#              database-gated SKIP, zero FAIL and at least the registered floor
#              of gated tests. The whole module runs, so a gated test added in a
#              new package is found without touching the registry.
#   4. Prints (and, in GitHub Actions, writes to the job summary) the counts.
#   It proves that the gated tests EXECUTE and PASS. It cannot prove that a test
#   asserts anything meaningful: that stays a review question.
#
# WHAT A TEST GETS (every variable is set for every suite)
#   REQUIRE_CONTRACT=1             absence of a database fails, never skips.
#   TEST_DATABASE_URL              a fresh BARE database (no tables): the legacy
#                                  Rust fixtures build and DROP their own tables
#                                  in `public`, so they must not meet a migrated
#                                  schema. The role is a superuser (as every Data
#                                  Plane service connects), so a test that checks
#                                  tenant isolation MUST drop to dataplane_app
#                                  inside the real org-scope helper
#                                  (pg-org-scope-rs / shared/go/orgscope); on any
#                                  other path RLS is inert and proves nothing.
#   MIGRATED_TEST_DATABASE_URL     a fresh clone of the migrated template
#                                  (init.sql + every migration, applied by the
#                                  real migrator, dataplane_app present). Use it
#                                  for tests that need the real schema in public.
#                                  Mark such a Rust test
#                                    #[ignore = "requires MIGRATED_TEST_DATABASE_URL ..."]
#   GRAPH_TEST_DATABASE_URL        graph-index-rs only: the bare database again,
#                                  under the name its fixtures read.
#   Suites that create their own scratch databases (tests/contract/postgres) get
#   only the admin connection in TEST_DATABASE_URL.
#
# ADDING A SUITE OR A TEST
#   * A new gated Rust test: use the marker above on an `#[ignore]`d test that
#     calls .expect(...) on the variable. Never gate with
#     `std::env::var("..._TEST_DATABASE_URL").ok()` and an early return: that is
#     the silent-pass trap, and --lint rejects it in a registered crate.
#   * A new crate or Go package with gated tests: add it to suite_load below (and
#     the floor, which may only go up) or, if it is not wired yet, to
#     EXEMPT_UNITS with the reason. --lint fails when a gate exists in neither.
#   * A suite that needs the migrated schema reads MIGRATED_TEST_DATABASE_URL.
#
# USAGE
#   bash tests/scripts/run-postgres-contract-tests.sh [SUITE ...]   run (default: all)
#   bash tests/scripts/run-postgres-contract-tests.sh --lint        no database
#   bash tests/scripts/run-postgres-contract-tests.sh --self-test   no database
#   bash tests/scripts/run-postgres-contract-tests.sh --list        no database
#
#   Needs bash 4+, psql (client 14+), go and, for Rust suites, cargo + protoc.
#   Run from anywhere; the runner locates apps/Data Plane v2 itself.
#
#   Local run against a throwaway server (never point it at a database that
#   holds real data; the guards below refuse non-local hosts and names that do
#   not end in _test):
#     docker run -d --rm --name dp-contract-pg -e POSTGRES_USER=dataplane \
#       -e POSTGRES_PASSWORD=dataplane -e POSTGRES_DB=dataplane_test \
#       -p 127.0.0.1:55432:5432 postgres:16
#     TEST_DATABASE_URL='postgres://dataplane:dataplane@127.0.0.1:55432/dataplane_test?sslmode=disable' \
#       bash tests/scripts/run-postgres-contract-tests.sh
#     docker stop dp-contract-pg
#
# ENVIRONMENT
#   TEST_DATABASE_URL   required in run mode (admin connection, see above)
#   KEEP_DATABASES=1    do not drop the databases this run created (debugging)
#   CONTRACT_LOG_DIR    keep per-suite logs here instead of a temporary directory
#   DP_ROOT, DP_WORKFLOW  override the plane root / workflow file (self-test)
#
# The script never prints a connection string: logs are scrubbed before they are
# shown. Exit status: 0 all suites passed, 1 a suite failed or a lint failed,
# 2 the environment is not usable (missing database, tool or bad URL).
# =============================================================================
set -euo pipefail

ROOT="${DP_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
WORKFLOW="${DP_WORKFLOW:-$ROOT/../../.github/workflows/dataplane-v2-ci.yml}"

TEMPLATE_DB="dp_migrated_template_test"
# Variables a gated test may read. Anything else in a registered unit is a lint
# failure: a gate this runner does not set can only ever skip.
ACCEPTED_GATE_VARS=" TEST_DATABASE_URL GRAPH_TEST_DATABASE_URL MIGRATED_TEST_DATABASE_URL "
# #[ignore] reasons that are not Postgres. They are exercised elsewhere (or by
# nobody yet) and this runner does not claim them. `live: ...` is a paid test against
# the live stack, run by its own proof script (never by this runner).
OTHER_INFRA_IGNORE='requires (TEST_QDRANT_URL|NEO4J_TEST_URL|MEILISEARCH_TEST_URL|DPV2_CACHE_TEST_URL|docker-compose stack)|live: '

# ── registry ─────────────────────────────────────────────────────────────────
SUITES="contract-postgres embedding-engine-rs graph-index-rs data-quality-go"

# suite_load <name>: sets S_KIND (go|rust), S_DIR (relative to the plane root),
# S_CRATE (rust), S_PKGS (go), S_MIN_GATED (floor: raise it when gated tests are
# added, never lower it), S_BARE_DB, S_MIGRATED_DB (empty: no clone) and
# S_BARE_ALIASES (extra variable names that carry the bare URL).
suite_load() {
  S_KIND="" S_DIR="" S_CRATE="" S_PKGS="" S_MIN_GATED=0 S_BARE_DB="" S_MIGRATED_DB="" S_BARE_ALIASES=""
  case $1 in
    contract-postgres)
      # Creates and drops its own scratch databases (init.sql + the real
      # migrator, up/down/up); it only needs the admin connection.
      # 25 = the 18 before CR-T12 + its 7 (graph_extraction_state_db_test.go: lifecycle and
      # backfill, post-condition, constraints, cascade, work index, RLS, privileges).
      S_KIND=go S_DIR=tests/contract/postgres S_PKGS="./..." S_MIN_GATED=25
      S_BARE_DB=contract_postgres_test ;;
    embedding-engine-rs)
      # The two legacy tests DROP and CREATE knowledge_units/documents in public:
      # bare database. Tests that need the real schema use the migrated clone: the
      # batch pipeline's contract tests (src/batch/tests.rs: CR-T8, the source stamp of
      # QB-M1b, the operator policy of CR-T11, the usage ledger's wiring of CR-T10, the
      # verification and refresh of CR-T9a and the background budget gate of CR-T9b, which adds 12
      # (the stop, the throttle, the lift, the spend statement and the rest: 64) and the context
      # reconciler's (src/context_reconciler/tests.rs: CR-T9a, and CR-T9b's 11 more: 52) are 116 of
      # the 118.
      S_KIND=rust S_DIR=services/embedding-engine-rs S_CRATE=embedding-engine-rs S_MIN_GATED=118
      S_BARE_DB=embedding_engine_test S_MIGRATED_DB=embedding_engine_migrated_test ;;
    graph-index-rs)
      # Fixtures refuse a URL that lacks "/graph_test" and a non-local host. 26 = the 4 legacy
      # tests of store.rs (bare database) + CR-T12's 22 on the migrated clone: 15 pipeline
      # scenarios run on the real tables (src/pipeline/tests.rs, `database_*`) and 7 of the SQL
      # itself as the runtime role (src/state_store/tests.rs).
      S_KIND=rust S_DIR=services/graph-index-rs S_CRATE=graph-index-rs S_MIN_GATED=26
      S_BARE_DB=graph_test S_MIGRATED_DB=graph_test_migrated_test S_BARE_ALIASES="GRAPH_TEST_DATABASE_URL" ;;
    data-quality-go)
      # The whole module: a gated test added in a new package (CR-T11 adds
      # internal/policy) is picked up by pass 1 without touching this registry.
      # 20 = the 18 of CR-T11 + CR-T14's 2 (internal/eval/store_record_integration_test.go: the harness's
      # recorded results, once and immutable, private to their organization, and the table's CHECK).
      S_KIND=go S_DIR=services/data-quality-go S_PKGS="./..." S_MIN_GATED=20
      S_BARE_DB=data_quality_test S_MIGRATED_DB=data_quality_migrated_test ;;
    *) die "unknown suite '$1' (registered: $SUITES)" ;;
  esac
}

# Units that carry a Postgres gate and are NOT run by this runner. Each is run
# by another job or is a known gap; --lint fails on an exempt unit that no longer
# has a gate (stale entry) and on any gate that is neither registered nor listed.
EXEMPT_UNITS="
services/retrieval-engine-rs|rust-integration-tests job runs it on an empty database; its #[ignore]d canaries are not run there (follow-up)
services/documents-api-go|documents-api-postgres-tests job runs it with -tags=integration
services/wiki-store-go|WIKI_TEST_DATABASE_URL suites are not wired yet (follow-up)
services/data-orchestrator-go|not wired yet (follow-up)
services/index-engine-rs|not wired yet (follow-up)
services/quickwit-adapter-rs|not wired yet (follow-up)
services/pg-org-scope-rs|not wired yet: its ignored test passes vacuously on an empty documents table (follow-up: seed rows)
shared/go|orgscope's real-database test passes vacuously on an empty documents table; tests/contract/postgres covers the role and RLS with seeded rows
"

# ── small helpers ────────────────────────────────────────────────────────────
warn() { printf '%s\n' "$*" >&2; }
die() { printf 'FATAL: %s\n' "$*" >&2; exit 2; }

group_start() {
  if [[ -n ${GITHUB_ACTIONS:-} ]]; then printf '::group::%s\n' "$*"; else printf '\n== %s\n' "$*"; fi
}
group_end() { if [[ -n ${GITHUB_ACTIONS:-} ]]; then printf '::endgroup::\n'; fi; }

# Never show a connection string, even one a driver echoes in an error.
scrub() { sed -E 's#postgres(ql)?://[^[:space:]"'"'"'`]*#postgres://<redacted>#g'; }
show_tail() { scrub <"$1" | tail -n "${2:-80}" >&2 || true; }

# Strip CR so CRLF checkouts and Windows-built tools compare equal.
norm() { tr -d '\r' <"$1" >"$1.norm" && mv "$1.norm" "$1"; }

count_lines() { local n; n=$(grep -c . "$1" 2>/dev/null || true); printf '%s' "${n:-0}"; }

# ── Rust discovery ───────────────────────────────────────────────────────────
# rust_source_files <dir>: every .rs file under dir, skipping build output.
rust_source_files() {
  [[ -d $1 ]] || return 0
  find "$1" \( -name target -o -name node_modules \) -prune -o -type f -name '*.rs' -print | LC_ALL=C sort
}

# rust_gated_tests <dir>: one "<fn>\t<VAR>\t<file relative to the plane root>"
# line per `#[ignore = "requires ...TEST_DATABASE_URL..."]` test. The attribute
# may be separated from the fn by other attributes.
rust_gated_tests() {
  local f
  rust_source_files "$1" | while IFS= read -r f; do
    awk -v root="$ROOT/" '
      { gsub(/\r/, "") }
      /#\[ignore[ \t]*=[ \t]*"requires [A-Z_]*TEST_DATABASE_URL/ {
        match($0, /requires [A-Z_]*TEST_DATABASE_URL/)
        pending = substr($0, RSTART + 9, RLENGTH - 9)
        next
      }
      # The attribute belongs to the next item: if that is not a fn, forget it
      # rather than attach it to a later, unrelated fn.
      pending != "" && /^[ \t]*(pub[ \t]+)?(struct|enum|mod|impl|trait|const|static|use|type)[ \t]/ { pending = ""; next }
      pending != "" && /^[ \t]*(pub[ \t]+)?(async[ \t]+)?fn[ \t]+[A-Za-z_][A-Za-z0-9_]*/ {
        s = $0
        sub(/^[ \t]*(pub[ \t]+)?(async[ \t]+)?fn[ \t]+/, "", s)
        sub(/[^A-Za-z0-9_].*$/, "", s)
        rel = FILENAME
        if (index(rel, root) == 1) rel = substr(rel, length(root) + 1)
        printf "%s\t%s\t%s\n", s, pending, rel
        pending = ""
      }' "$f"
  done
}

# resolve_rust_test <list-file> <fn>: the full libtest path(s) of a test fn.
resolve_rust_test() {
  awk -v n="$2" '
    /: test$/ {
      p = $0; sub(/: test$/, "", p)
      if (p == n || (length(p) > length(n) + 2 && substr(p, length(p) - length(n) - 1) == "::" n)) print p
    }' "$1"
}

# assert_rust_run <log> <paths-file> <exit-code>: every gated test passed.
assert_rust_run() {
  local log=$1 paths=$2 rc=$3 bad=0 p
  if (( rc != 0 )); then warn "cargo test exited with status $rc"; bad=1; fi
  while IFS= read -r p; do
    [[ -n $p ]] || continue
    if ! grep -Fxq "test $p ... ok" "$log"; then warn "gated test did not pass: $p"; bad=1; fi
  done <"$paths"
  if grep -Eq '^test .* \.\.\. (FAILED|ignored)' "$log"; then
    warn "a selected test failed or stayed ignored:"
    grep -E '^test .* \.\.\. (FAILED|ignored)' "$log" | sed 's/^/  /' >&2
    bad=1
  fi
  return "$bad"
}

# ── Go parsing (go test -v output) ───────────────────────────────────────────
# go_skipped_for_database <log>: names of top-level tests that SKIPped because a
# *DATABASE_URL variable was unset. t.Skip prints its reason on the line before
# `--- SKIP`; no test in the registered packages runs in parallel.
go_skipped_for_database() {
  awk '/^--- SKIP: / { if (prev ~ /DATABASE_URL/) print $3 } { prev = $0 }' "$1"
}

# assert_go_run <log> <gated-names-file> <floor> <exit-code>
assert_go_run() {
  local log=$1 gated=$2 floor=$3 rc=$4 bad=0 n skipped failed name
  n=$(count_lines "$gated")
  # Only a skip caused by a missing database variable is this runner's business;
  # a test that skips for another reason (a platform, another service) is not.
  skipped=$(go_skipped_for_database "$log" | grep -c . || true)
  failed=$(grep -c '^--- FAIL: ' "$log" || true)
  if (( rc != 0 )); then warn "go test exited with status $rc"; bad=1; fi
  if (( failed != 0 )); then warn "$failed test(s) failed"; bad=1; fi
  if (( skipped != 0 )); then
    warn "$skipped database-gated test(s) SKIPPED although the database was provided (a gated test must run, never skip):"
    go_skipped_for_database "$log" | sed 's/^/  --- SKIP: /' >&2
    bad=1
  fi
  if (( n < floor )); then
    warn "found $n database-gated test(s), the registered floor is $floor: tests were removed or their gate changed"
    bad=1
  fi
  if (( n == 0 )); then warn "no database-gated test was found at all"; bad=1; fi
  while IFS= read -r name; do
    [[ -n $name ]] || continue
    if ! grep -Eq "^--- PASS: ${name} \(" "$log"; then warn "gated test did not pass: $name"; bad=1; fi
  done <"$gated"
  return "$bad"
}

# ── lint (no database) ───────────────────────────────────────────────────────
lint_bad=0
lint_fail() { warn "LINT: $*"; lint_bad=1; }

registered_dirs() {
  local s
  for s in $SUITES; do suite_load "$s"; printf '%s\n' "$S_DIR"; done
}

# lint_coverage <root> <registered dirs> <exempt "dir|reason" lines>
lint_coverage() {
  local root=$1 registered=$2 exempt=$3 f d reason match kind used="" count=0
  local files
  files=$(
    cd "$root" || exit 1
    {
      grep -rlE --include='*.rs' --exclude-dir=target --exclude-dir=node_modules \
        -e '#\[ignore[[:space:]]*=[[:space:]]*"requires [A-Z_]*TEST_DATABASE_URL' \
        -e 'env::var\("[A-Z_]*TEST_DATABASE_URL"\)' \
        services shared tools tests 2>/dev/null || true
      grep -rlE --include='*_test.go' --exclude-dir=target --exclude-dir=node_modules \
        'Getenv\("[A-Z_]*TEST_DATABASE_URL"\)' \
        services shared tools tests 2>/dev/null || true
    } | LC_ALL=C sort -u
  )
  while IFS= read -r f; do
    [[ -n $f ]] || continue
    count=$((count + 1))
    match="" kind=""
    while IFS= read -r d; do
      [[ -n $d ]] || continue
      if [[ $f == "$d"/* && ${#d} -gt ${#match} ]]; then match=$d kind=registered; fi
    done <<<"$registered"
    while IFS='|' read -r d reason; do
      [[ -n $d ]] || continue
      if [[ $f == "$d"/* && ${#d} -gt ${#match} ]]; then match=$d kind=exempt; fi
    done <<<"$exempt"
    if [[ -z $match ]]; then
      lint_fail "$f has a Postgres gate but belongs to no registered suite and no exemption: add it to suite_load (or to EXEMPT_UNITS with the reason) in run-postgres-contract-tests.sh"
    else
      used="$used$match"$'\n'
    fi
  done <<<"$files"
  while IFS= read -r d; do
    [[ -n $d ]] || continue
    grep -Fxq "$d" <<<"$used" || lint_fail "registered suite directory $d has no Postgres gate: the suite would pass without running anything"
  done <<<"$registered"
  while IFS='|' read -r d reason; do
    [[ -n $d ]] || continue
    grep -Fxq "$d" <<<"$used" || lint_fail "exempt unit $d no longer has a Postgres gate: remove the stale entry"
  done <<<"$exempt"
  printf 'lint: %s file(s) carry a Postgres gate; every one is registered or exempt\n' "$count"
}

# lint_registered_rust <dir>: the Rust rules for a crate this runner owns.
lint_registered_rust() {
  local dir=$1 f hit var line
  while IFS= read -r f; do
    # 1. A silent self-gate (optional env var, early return) passes without a database.
    if hit=$(grep -nE 'env::var\("[A-Z_]*TEST_DATABASE_URL"\)[[:space:]]*\.ok\(\)' "$f"); then
      lint_fail "$f: $(printf '%s' "$hit" | head -n 1 | tr -d '\r') reads the database variable with .ok(): gate with #[ignore = \"requires ...\"] and .expect(...) so absence fails"
    fi
    # 2. Every #[ignore] must say what it needs.
    while IFS= read -r line; do
      [[ -n $line ]] || continue
      line=${line%$'\r'}
      if [[ $line =~ requires\ ([A-Z_]*TEST_DATABASE_URL) ]]; then
        var=${BASH_REMATCH[1]}
        [[ $ACCEPTED_GATE_VARS == *" $var "* ]] || lint_fail "$f: gate variable $var is not set by this runner (accepted:$ACCEPTED_GATE_VARS)"
      elif [[ ! $line =~ $OTHER_INFRA_IGNORE ]]; then
        lint_fail "$f: unclassified #[ignore] ($(printf '%s' "$line" | tr -s ' ')): say what it requires; a Postgres gate must read 'requires TEST_DATABASE_URL ...'"
      fi
    done < <(grep -E '^[[:space:]]*#\[ignore' "$f" || true)
  done < <(rust_source_files "$dir")
}

# lint_registered_go <dir>: gate variables in a Go suite must be ones this runner
# sets, and a gate must say which variable is missing when it skips: pass 1 of a
# Go suite recognises its gated tests by a SKIP whose reason names *DATABASE_URL.
lint_registered_go() {
  local dir=$1 var f hit
  [[ -d $dir ]] || return 0
  while IFS= read -r var; do
    [[ -n $var ]] || continue
    [[ $ACCEPTED_GATE_VARS == *" $var "* ]] || lint_fail "$dir: gate variable $var is not set by this runner (accepted:$ACCEPTED_GATE_VARS)"
  done < <(grep -rhoE --include='*_test.go' 'Getenv\("[A-Z_]*TEST_DATABASE_URL"\)' "$dir" 2>/dev/null | sed -E 's/Getenv\("([A-Z_]*)"\)/\1/' | LC_ALL=C sort -u || true)
  while IFS= read -r f; do
    [[ -n $f ]] || continue
    if hit=$(grep -nE 't\.Skipf?\(' "$f" | grep -v 'DATABASE_URL'); then
      lint_fail "$f reads a database variable but skips without naming it ($(printf '%s' "$hit" | head -n 1 | tr -d '\r\t' | tr -s ' ')): the runner could not tell this gated test from any other skip"
    fi
  done < <(grep -rlE --include='*_test.go' 'Getenv\("[A-Z_]*TEST_DATABASE_URL"\)' "$dir" 2>/dev/null || true)
}

lint_registry() {
  local s
  for s in $SUITES; do
    suite_load "$s"
    [[ -d $ROOT/$S_DIR ]] || { lint_fail "suite $s: directory $S_DIR does not exist"; continue; }
    case $S_KIND in
      rust)
        grep -Eq "^name[[:space:]]*=[[:space:]]*\"$S_CRATE\"" "$ROOT/$S_DIR/Cargo.toml" 2>/dev/null \
          || lint_fail "suite $s: $S_DIR/Cargo.toml is not package $S_CRATE"
        lint_registered_rust "$ROOT/$S_DIR" ;;
      go)
        [[ -f $ROOT/$S_DIR/go.mod ]] || lint_fail "suite $s: $S_DIR/go.mod is missing"
        lint_registered_go "$ROOT/$S_DIR" ;;
      *) lint_fail "suite $s: unknown kind '$S_KIND'" ;;
    esac
    (( S_MIN_GATED > 0 )) || lint_fail "suite $s: S_MIN_GATED must be a positive floor"
    [[ $S_BARE_DB =~ ^[a-z][a-z0-9_]*_test$ ]] || lint_fail "suite $s: bare database name '$S_BARE_DB' must be a lowercase *_test identifier"
    [[ -z $S_MIGRATED_DB || $S_MIGRATED_DB =~ ^[a-z][a-z0-9_]*_test$ ]] || lint_fail "suite $s: migrated database name '$S_MIGRATED_DB' must be a lowercase *_test identifier"
    if [[ $S_KIND == rust ]]; then
      local found
      found=$(rust_gated_tests "$ROOT/$S_DIR" | grep -c . || true)
      (( found >= S_MIN_GATED )) || lint_fail "suite $s: found $found gated test(s), floor is $S_MIN_GATED (tests were removed, or the #[ignore = \"requires ...\"] marker changed)"
    fi
  done
}

# lint_migrations <dir>: a file the migrator silently ignores is a migration that
# never runs. The migrator applies ^[0-9]{14}_.+\.sql$ and skips *.disabled.
lint_migrations() {
  local dir=$1 f name forward=0
  [[ -d $dir ]] || { lint_fail "migrations directory $dir is missing"; return; }
  while IFS= read -r f; do
    name=${f##*/}
    case $name in
      *.disabled) continue ;;
      *.sql) ;;
      *) continue ;;
    esac
    if [[ ! $name =~ ^[0-9]{14}_.+\.sql$ ]]; then
      lint_fail "migration file $name does not match NNNNNNNNNNNNNN_name.sql (14-digit version): the migrator would ignore it silently"
    elif [[ $name != *.down.sql ]]; then
      forward=$((forward + 1))
    fi
  done < <(find "$dir" -maxdepth 1 -type f | LC_ALL=C sort)
  (( forward > 0 )) || lint_fail "no forward migration found in $dir"
  printf 'lint: %s forward migration(s), every file name is applied by the migrator\n' "$forward"
}

# lint_wiring <workflow-file>: the CI job exists, runs this script against a real
# Postgres 16 with REQUIRE_CONTRACT=1, and cannot be made to pass by an ignored failure.
lint_wiring() {
  local wf=$1 block wiring
  [[ -f $wf ]] || { lint_fail "workflow file $wf is missing"; return; }
  block=$(tr -d '\r' <"$wf" | awk '
    /^  postgres-contract-tests:[ \t]*$/ { on = 1; print; next }
    on && /^  [A-Za-z0-9_-]+:[ \t]*$/ { on = 0 }
    on { print }')
  if [[ -z $block ]]; then
    lint_fail "job postgres-contract-tests is missing from $(basename "$wf"): the gated tests would not run in CI"
  else
    grep -Eq 'image:[[:space:]]*postgres:16([[:space:]]|$)' <<<"$block" || lint_fail "job postgres-contract-tests needs a postgres:16 service"
    grep -Eq 'REQUIRE_CONTRACT:[[:space:]]*"?1"?' <<<"$block" || lint_fail "job postgres-contract-tests must set REQUIRE_CONTRACT: \"1\""
    grep -Fq 'tests/scripts/run-postgres-contract-tests.sh' <<<"$block" || lint_fail "job postgres-contract-tests must run tests/scripts/run-postgres-contract-tests.sh"
    grep -Fq 'TEST_DATABASE_URL' <<<"$block" || lint_fail "job postgres-contract-tests must set TEST_DATABASE_URL"
    if grep -Eq 'continue-on-error:[[:space:]]*true' <<<"$block"; then lint_fail "job postgres-contract-tests must not set continue-on-error: a failing contract would pass"; fi
  fi
  wiring=$(tr -d '\r' <"$wf" | awk '
    /^  postgres-contract-wiring:[ \t]*$/ { on = 1; print; next }
    on && /^  [A-Za-z0-9_-]+:[ \t]*$/ { on = 0 }
    on { print }')
  if [[ -z $wiring ]]; then
    lint_fail "job postgres-contract-wiring is missing from $(basename "$wf"): nothing would run --self-test and --lint"
  else
    grep -Fq -e '--self-test' <<<"$wiring" || lint_fail "job postgres-contract-wiring must run --self-test"
    grep -Fq -e '--lint' <<<"$wiring" || lint_fail "job postgres-contract-wiring must run --lint"
  fi
  printf 'lint: workflow wiring checked (%s)\n' "$(basename "$wf")"
}

lint_all() {
  lint_bad=0
  lint_registry
  lint_coverage "$ROOT" "$(registered_dirs)" "$EXEMPT_UNITS"
  lint_migrations "$ROOT/infra/postgres/migrations"
  lint_wiring "$WORKFLOW"
  if (( lint_bad != 0 )); then warn "lint FAILED"; return 1; fi
  echo "lint OK"
}

# ── --list ───────────────────────────────────────────────────────────────────
list_all() {
  local s fn var file
  for s in $SUITES; do
    suite_load "$s"
    printf '%s  kind=%s dir=%s floor=%s bare=%s migrated=%s\n' "$s" "$S_KIND" "$S_DIR" "$S_MIN_GATED" "$S_BARE_DB" "${S_MIGRATED_DB:-none}"
    if [[ $S_KIND == rust ]]; then
      while IFS=$'\t' read -r fn var file; do
        if [[ -n $fn ]]; then printf '    gated: %s  (%s, %s)\n' "$fn" "$var" "$file"; fi
      done < <(rust_gated_tests "$ROOT/$S_DIR")
    else
      printf '    gated tests are discovered at run time: the tests that SKIP without a database\n'
    fi
  done
  echo "exempt (a gate exists, run elsewhere or not wired):"
  while IFS='|' read -r file var; do
    if [[ -n $file ]]; then printf '    %s  - %s\n' "$file" "$var"; fi
  done <<<"$EXEMPT_UNITS"
}

# ── run mode: database ───────────────────────────────────────────────────────
ADMIN_URL="" ADMIN_DB="" URL_PREFIX="" URL_QUERY=""
CREATED_DBS=()
WORK="" WORK_OWNED=0
MIGRATOR=""
RESULTS=()
FAILED=0

# parse_admin_url: validates TEST_DATABASE_URL (the same guards as the Go harness).
parse_admin_url() {
  local raw=${TEST_DATABASE_URL:-}
  local re='^(postgres(ql)?://([^/@]*@)?(\[[0-9a-fA-F:]+\]|[^/:?@]+)(:[0-9]+)?)/([^/?]+)(\?.*)?$'
  [[ -n $raw ]] || die "TEST_DATABASE_URL is not set: a contract run that cannot reach its database must fail, never skip (see the header of this script)"
  [[ $raw =~ $re ]] || die "TEST_DATABASE_URL must look like postgres://user:password@127.0.0.1:5432/<name>_test[?sslmode=disable]"
  local host=${BASH_REMATCH[4]}
  URL_PREFIX=${BASH_REMATCH[1]} ADMIN_DB=${BASH_REMATCH[6]} URL_QUERY=${BASH_REMATCH[7]:-}
  case $host in
    localhost | 127.0.0.1 | '[::1]') ;;
    *) die "refusing non-local test database host '$host'" ;;
  esac
  [[ $ADMIN_DB == *_test ]] || die "refusing database '$ADMIN_DB': the name of a disposable test database must end in _test"
  ADMIN_URL=$raw
}

db_url() { printf '%s/%s%s' "$URL_PREFIX" "$1" "$URL_QUERY"; }

psql_on() {
  local url=$1
  shift
  PGOPTIONS='-c client_min_messages=warning' psql "$url" -X -q -v ON_ERROR_STOP=1 "$@"
}

need_tool() {
  command -v "$1" >/dev/null 2>&1 || die "'$1' is required on PATH ($2): a contract run that cannot start must fail, never skip"
}

# provision_db <name> [template]: a fresh database; dropped again at exit.
provision_db() {
  local name=$1 template=${2:-}
  [[ $name =~ ^[a-z][a-z0-9_]*_test$ ]] || die "internal: database name '$name' is not a lowercase *_test identifier"
  [[ $name != "$ADMIN_DB" ]] || die "internal: refusing to recreate the admin database '$name'"
  local hint="the TEST_DATABASE_URL role must be a superuser of a disposable PostgreSQL 16 (CREATE DATABASE, CREATE ROLE, CREATE EXTENSION)"
  CREATED_DBS+=("$name")
  psql_on "$ADMIN_URL" -c "DROP DATABASE IF EXISTS \"$name\" WITH (FORCE)" >/dev/null || die "cannot drop database '$name': $hint"
  if [[ -n $template ]]; then
    # A backend that served the previous connection to the template may still be
    # exiting; Postgres then refuses the copy ("source database is being accessed
    # by other users"). Retry briefly instead of failing the whole run on a race.
    local attempt created=0
    for attempt in 1 2 3 4 5 6 7 8 9 10; do
      if psql_on "$ADMIN_URL" -c "CREATE DATABASE \"$name\" TEMPLATE \"$template\"" >/dev/null 2>"$WORK/clone.err"; then created=1; break; fi
      sleep 1
    done
    if (( created == 0 )); then
      show_tail "$WORK/clone.err" 10
      die "cannot create database '$name' from template '$template' after 10 attempts: $hint"
    fi
  else
    psql_on "$ADMIN_URL" -c "CREATE DATABASE \"$name\"" >/dev/null || die "cannot create database '$name': $hint"
  fi
  local enc
  enc=$(psql_on "$ADMIN_URL" -tA -c "SELECT pg_encoding_to_char(encoding) FROM pg_database WHERE datname = '$name'") || die "cannot read the encoding of database '$name'"
  [[ $enc == UTF8 ]] || die "the test server creates $enc databases; the contracts need UTF8 (initdb --encoding=UTF8)"
}

cleanup() {
  local rc=$? db
  if [[ -n $ADMIN_URL && ${KEEP_DATABASES:-} != 1 && ${#CREATED_DBS[@]} -gt 0 ]]; then
    for db in "${CREATED_DBS[@]}"; do
      psql_on "$ADMIN_URL" -c "DROP DATABASE IF EXISTS \"$db\" WITH (FORCE)" >/dev/null 2>&1 || true
    done
  fi
  if [[ $WORK_OWNED == 1 && -n $WORK ]]; then rm -rf "$WORK"; fi
  exit "$rc"
}

# build_template: init.sql, then every migration through the REAL migrator.
build_template() {
  local mdir="$ROOT/infra/postgres/migrations" url forward applied
  group_start "migrations: init.sql + tools/migrator up on a fresh Postgres"
  MIGRATOR="$WORK/migrator"
  (cd "$ROOT/tools/migrator" && CGO_ENABLED=0 go build -trimpath -o "$MIGRATOR" .) || die "could not build tools/migrator"
  provision_db "$TEMPLATE_DB"
  url=$(db_url "$TEMPLATE_DB")
  # The fresh-database path production uses: docker-entrypoint-initdb runs init.sql
  # once, then the migrate service applies every migration in version order.
  psql_on "$url" -f "$ROOT/infra/postgres/init.sql" >/dev/null || die "init.sql did not apply"
  DATABASE_URL=$url "$MIGRATOR" up --migrations-dir "$mdir" || die "the real migrator failed to apply the migrations"
  forward=$(find "$mdir" -maxdepth 1 -type f -name '*.sql' ! -name '*.down.sql' | grep -Ec '/[0-9]{14}_[^/]+\.sql$' || true)
  applied=$(psql_on "$url" -tA -c "SELECT count(*) FROM schema_migrations") || die "cannot read schema_migrations after the migrator ran"
  if [[ $applied != "$forward" ]]; then
    warn "schema_migrations holds $applied row(s) but $forward forward migration file(s) exist"
    DATABASE_URL=$url "$MIGRATOR" status | scrub >&2 || true
    die "the migrator did not record every migration"
  fi
  # The role the real org-scope helper switches to must exist, and must be subject to RLS.
  local role
  role=$(psql_on "$url" -tA -c "SELECT rolcanlogin::text || ':' || rolbypassrls::text || ':' || rolsuper::text FROM pg_roles WHERE rolname = 'dataplane_app'") || die "cannot read pg_roles"
  [[ $role == "false:false:false" ]] || die "dataplane_app is missing or is not NOLOGIN NOBYPASSRLS NOSUPERUSER (got '${role:-absent}'): RLS would be inert"
  echo "migrated template ready: $applied migration(s) applied by the real migrator, dataplane_app subject to RLS"
  group_end
}

# ── run mode: suites ─────────────────────────────────────────────────────────
record() { RESULTS+=("$1|$2|$3|$4"); }   # suite|gated|result|detail

suite_fail() {
  FAILED=1
  warn "SUITE FAILED: $1: $2"
}

# suite_env: exports the variables every suite gets (call inside a subshell).
suite_env() {
  local bare_url=$1 migrated_url=$2 alias
  export REQUIRE_CONTRACT=1 TEST_DATABASE_URL=$bare_url
  [[ -z $migrated_url ]] || export MIGRATED_TEST_DATABASE_URL=$migrated_url
  for alias in $S_BARE_ALIASES; do export "$alias=$bare_url"; done
}

run_rust_suite() {
  local name=$1 bare_url=$2 migrated_url=$3
  local disc="$WORK/$name.gated" list="$WORK/$name.list" paths="$WORK/$name.paths" log="$WORK/$name.log"
  local rc=0 n fn var file found resolved=0 p
  local -a args=()
  rust_gated_tests "$ROOT/$S_DIR" >"$disc"
  n=$(count_lines "$disc")
  if (( n == 0 || n < S_MIN_GATED )); then
    suite_fail "$name" "found $n gated test(s) in $S_DIR, the registered floor is $S_MIN_GATED"
    record "$name" "$n" FAIL "gated tests missing"
    return
  fi
  # What libtest actually compiled: a marker whose test is cfg'd out would otherwise pass unseen.
  ( suite_env "$bare_url" "$migrated_url"; cd "$ROOT" && cargo test -p "$S_CRATE" --tests -- --ignored --list ) >"$list" 2>"$list.err" || rc=$?
  if (( rc != 0 )); then
    show_tail "$list.err" 60
    suite_fail "$name" "cargo could not build or list the tests of $S_CRATE"
    record "$name" "$n" FAIL "build or list failed"
    return
  fi
  norm "$list"
  : >"$paths"
  while IFS=$'\t' read -r fn var file; do
    [[ -n $fn ]] || continue
    found=$(resolve_rust_test "$list" "$fn")
    if [[ -z $found ]]; then
      suite_fail "$name" "$fn ($file) carries a Postgres gate but libtest does not list it as an ignored test: it is not compiled, so it could never run"
      record "$name" "$n" FAIL "$fn not compiled"
      return
    fi
    printf '%s\n' "$found" >>"$paths"
    resolved=$((resolved + 1))
  done <"$disc"
  while IFS= read -r p; do [[ -n $p ]] && args+=("$p"); done <"$paths"
  rc=0
  # --test-threads=1: the legacy fixtures DROP and CREATE the same tables.
  ( suite_env "$bare_url" "$migrated_url"; cd "$ROOT" && cargo test -p "$S_CRATE" --tests --no-fail-fast -- --ignored --exact --test-threads=1 "${args[@]}" ) >"$log" 2>&1 || rc=$?
  norm "$log"
  if assert_rust_run "$log" "$paths" "$rc"; then
    echo "  $name: $n gated test(s) ran and passed (exact paths: $(count_lines "$paths"))"
    record "$name" "$n" PASS "all gated tests passed"
  else
    show_tail "$log" 80
    suite_fail "$name" "gated tests did not all pass"
    record "$name" "$n" FAIL "see log"
  fi
}

run_go_suite() {
  local name=$1 bare_url=$2 migrated_url=$3
  local log1="$WORK/$name.pass1.log" log2="$WORK/$name.log" gated="$WORK/$name.gated" rc=0 n
  local -a pkgs
  read -r -a pkgs <<<"$S_PKGS"
  # Pass 1, no database variable and no REQUIRE_CONTRACT/CI: whatever SKIPs because
  # a *DATABASE_URL is unset IS the gated set. No source parsing can drift from it.
  ( cd "$ROOT/$S_DIR" && env -u CI -u REQUIRE_CONTRACT -u TEST_DATABASE_URL -u MIGRATED_TEST_DATABASE_URL -u GRAPH_TEST_DATABASE_URL \
      go test -count=1 -v "${pkgs[@]}" ) >"$log1" 2>&1 || rc=$?
  norm "$log1"
  if (( rc != 0 )); then
    show_tail "$log1" 60
    suite_fail "$name" "pass 1 (no database) failed: a non-database test fails or the packages do not build"
    record "$name" 0 FAIL "pass 1 failed"
    return
  fi
  go_skipped_for_database "$log1" | LC_ALL=C sort -u >"$gated"
  n=$(count_lines "$gated")
  rc=0
  ( suite_env "$bare_url" "$migrated_url"; cd "$ROOT/$S_DIR" && go test -count=1 -v "${pkgs[@]}" ) >"$log2" 2>&1 || rc=$?
  norm "$log2"
  if assert_go_run "$log2" "$gated" "$S_MIN_GATED" "$rc"; then
    echo "  $name: $n gated test(s) ran and passed, 0 skipped, $(grep -c '^--- PASS: ' "$log2" || true) top-level test(s) passed in total"
    record "$name" "$n" PASS "all gated tests passed"
  else
    show_tail "$log2" 80
    suite_fail "$name" "gated tests did not all pass"
    record "$name" "$n" FAIL "see log"
  fi
}

write_summary() {
  local r suite gated result detail
  echo
  printf '%-22s %-7s %-6s %s\n' SUITE GATED RESULT DETAIL
  for r in "${RESULTS[@]}"; do
    IFS='|' read -r suite gated result detail <<<"$r"
    printf '%-22s %-7s %-6s %s\n' "$suite" "$gated" "$result" "$detail"
  done
  if [[ -n ${GITHUB_STEP_SUMMARY:-} ]]; then
    {
      echo "### Postgres contract tests (real migrator, REQUIRE_CONTRACT=1)"
      echo
      echo "| suite | gated tests executed | result | detail |"
      echo "| --- | ---: | --- | --- |"
      for r in "${RESULTS[@]}"; do
        IFS='|' read -r suite gated result detail <<<"$r"
        echo "| $suite | $gated | $result | $detail |"
      done
    } >>"$GITHUB_STEP_SUMMARY"
  fi
}

run_mode() {
  local -a chosen=("$@")
  local s need_template=0 need_cargo=0 bare_url migrated_url
  if (( ${#chosen[@]} == 0 )); then read -r -a chosen <<<"$SUITES"; fi
  for s in "${chosen[@]}"; do
    suite_load "$s"
    [[ -z $S_MIGRATED_DB ]] || need_template=1
    [[ $S_KIND != rust ]] || need_cargo=1
  done
  export REQUIRE_CONTRACT=1
  parse_admin_url
  need_tool psql "the PostgreSQL client, version 14 or newer"
  need_tool go "needed to build tools/migrator and run the Go suites"
  (( need_cargo == 0 )) || need_tool cargo "needed to run the Rust suites"

  if [[ -n ${CONTRACT_LOG_DIR:-} ]]; then WORK=$CONTRACT_LOG_DIR; mkdir -p "$WORK"; else WORK=$(mktemp -d); WORK_OWNED=1; fi
  trap cleanup EXIT

  local version
  version=$(psql_on "$ADMIN_URL" -tA -c "SHOW server_version_num") || die "cannot connect to the test database server"
  (( version >= 160000 )) || die "the contracts need PostgreSQL 16 or newer, the server reports $version"
  echo "test server: PostgreSQL $((version / 10000)), admin database '$ADMIN_DB', logs in $WORK"

  # Cheap static guard first: a migration file the migrator would ignore.
  lint_bad=0
  lint_migrations "$ROOT/infra/postgres/migrations"
  (( lint_bad == 0 )) || die "migration file names are not all applied by the migrator"

  (( need_template == 0 )) || build_template

  for s in "${chosen[@]}"; do
    suite_load "$s"
    group_start "suite $s ($S_KIND, floor $S_MIN_GATED gated)"
    provision_db "$S_BARE_DB"
    bare_url=$(db_url "$S_BARE_DB")
    migrated_url=""
    if [[ -n $S_MIGRATED_DB ]]; then
      provision_db "$S_MIGRATED_DB" "$TEMPLATE_DB"
      migrated_url=$(db_url "$S_MIGRATED_DB")
    fi
    case $S_KIND in
      rust) run_rust_suite "$s" "$bare_url" "$migrated_url" ;;
      go) run_go_suite "$s" "$bare_url" "$migrated_url" ;;
    esac
    group_end
  done

  write_summary
  if (( FAILED != 0 )); then warn "FAILED: at least one gated suite did not run and pass"; return 1; fi
  echo "OK: every gated test ran and passed"
}

# ── --self-test (no database) ────────────────────────────────────────────────
ST_FAIL=0
st_check() {   # st_check <ok|fail> <description> <command...>
  local want=$1 desc=$2 got=ok
  shift 2
  if ( "$@" ) >/dev/null 2>&1; then got=ok; else got=fail; fi
  if [[ $got == "$want" ]]; then printf '  ok    %s\n' "$desc"; else printf '  FAIL  %s (wanted %s, got %s)\n' "$desc" "$want" "$got" >&2; ST_FAIL=1; fi
}

self_test() {
  local t fx
  t=$(mktemp -d)
  # shellcheck disable=SC2064
  trap "rm -rf '$t'" EXIT

  echo "go output (real go test -v format)"
  printf '=== RUN   TestPostgresA\n    a_test.go:22: TEST_DATABASE_URL is required for disposable PostgreSQL integration tests\n--- SKIP: TestPostgresA (0.00s)\n=== RUN   TestPlain\n--- PASS: TestPlain (0.00s)\n=== RUN   TestPostgresB\n    b_test.go:58: TEST_DATABASE_URL is not set: point it at a disposable local Postgres 16\n--- SKIP: TestPostgresB (0.00s)\nPASS\nok  \texample.com/x\t0.1s\n' >"$t/pass1.log"
  go_skipped_for_database "$t/pass1.log" >"$t/gated.txt"
  st_check ok "pass 1 finds exactly the tests skipped for a database variable" test "$(tr '\n' ' ' <"$t/gated.txt")" = "TestPostgresA TestPostgresB "
  printf '=== RUN   TestPlain\n--- SKIP: TestPlain (0.00s)\n' >"$t/other-skip.log"
  st_check ok "a skip for another reason is not part of the gated set" test -z "$(go_skipped_for_database "$t/other-skip.log")"
  # the defect: all gated tests skip and go reports ok
  cp "$t/pass1.log" "$t/silent.log"
  st_check fail "pass 2 where the gated tests SKIP is rejected, although go exited 0" assert_go_run "$t/silent.log" "$t/gated.txt" 2 0
  printf -- '--- PASS: TestPostgresA (0.10s)\n--- PASS: TestPlain (0.00s)\n--- PASS: TestPostgresB (0.20s)\nPASS\n' >"$t/good.log"
  st_check ok "pass 2 where every gated test passed is accepted" assert_go_run "$t/good.log" "$t/gated.txt" 2 0
  printf 'TestPostgresA\nTestPostgresMissing\n' >"$t/gated-missing.txt"
  st_check fail "a gated test missing from pass 2 is rejected" assert_go_run "$t/good.log" "$t/gated-missing.txt" 2 0
  st_check fail "fewer gated tests than the registered floor is rejected" assert_go_run "$t/good.log" "$t/gated.txt" 3 0
  : >"$t/gated-empty.txt"
  st_check fail "an empty gated set is rejected" assert_go_run "$t/good.log" "$t/gated-empty.txt" 0 0
  printf -- '--- PASS: TestPostgresA (0.10s)\n--- FAIL: TestPostgresB (0.20s)\nFAIL\n' >"$t/failed.log"
  st_check fail "a failing gated test is rejected" assert_go_run "$t/failed.log" "$t/gated.txt" 2 1
  st_check fail "a non-zero go exit is rejected even if the log looks clean" assert_go_run "$t/good.log" "$t/gated.txt" 2 1
  printf '=== RUN   TestWindowsOnly\n    w_test.go:5: runs on windows only\n--- SKIP: TestWindowsOnly (0.00s)\n--- PASS: TestPostgresA (0.10s)\n--- PASS: TestPostgresB (0.20s)\nPASS\n' >"$t/other-skip2.log"
  st_check ok "pass 2 tolerates a skip that has nothing to do with the database" assert_go_run "$t/other-skip2.log" "$t/gated.txt" 2 0

  echo "lint (go gates)"
  mkdir -p "$t/gor"
  printf 'package x\nfunc TestA(t *testing.T) {\n\tif os.Getenv("TEST_DATABASE_URL") == "" {\n\t\tt.Skip("TEST_DATABASE_URL is required")\n\t}\n}\n' >"$t/gor/a_test.go"
  st_check ok "a Go gate that names its variable when it skips is accepted" bash -c "$(declare -f lint_fail lint_registered_go warn); ACCEPTED_GATE_VARS='$ACCEPTED_GATE_VARS'; lint_bad=0; lint_registered_go '$t/gor'; exit \$lint_bad"
  printf 'package x\nfunc TestB(t *testing.T) {\n\tif os.Getenv("TEST_DATABASE_URL") == "" {\n\t\tt.Skip("no database here")\n\t}\n}\n' >"$t/gor/b_test.go"
  st_check fail "a Go gate that skips without naming the variable is rejected (pass 1 could not recognise it)" bash -c "$(declare -f lint_fail lint_registered_go warn); ACCEPTED_GATE_VARS='$ACCEPTED_GATE_VARS'; lint_bad=0; lint_registered_go '$t/gor'; exit \$lint_bad"
  rm -f "$t/gor/b_test.go"
  printf 'package x\nfunc TestC(t *testing.T) {\n\t_ = os.Getenv("WIKI_TEST_DATABASE_URL")\n\tt.Skip("WIKI_TEST_DATABASE_URL is required")\n}\n' >"$t/gor/c_test.go"
  st_check fail "a Go gate variable the runner does not set is rejected" bash -c "$(declare -f lint_fail lint_registered_go warn); ACCEPTED_GATE_VARS='$ACCEPTED_GATE_VARS'; lint_bad=0; lint_registered_go '$t/gor'; exit \$lint_bad"

  echo "rust output (real libtest format)"
  printf 'batch::terminal_failure_tests::a_later_success_reconciles: test\nbatch::terminal_failure_tests::an_already_indexed: test\ngdpr::tests::needs_qdrant: test\n\n3 tests, 0 benchmarks\n' >"$t/list.txt"
  st_check ok "a gated fn resolves to its full libtest path" test "$(resolve_rust_test "$t/list.txt" a_later_success_reconciles)" = "batch::terminal_failure_tests::a_later_success_reconciles"
  st_check ok "a fn that is not listed resolves to nothing (compiled out)" test -z "$(resolve_rust_test "$t/list.txt" not_compiled)"
  st_check ok "a suffix of a fn name does not match another test" test -z "$(resolve_rust_test "$t/list.txt" reconciles)"
  printf 'batch::terminal_failure_tests::a_later_success_reconciles\n' >"$t/paths.txt"
  printf 'running 1 test\ntest batch::terminal_failure_tests::a_later_success_reconciles ... ok\n\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 100 filtered out; finished in 0.31s\n' >"$t/rust-ok.log"
  st_check ok "a gated test that reports ok is accepted" assert_rust_run "$t/rust-ok.log" "$t/paths.txt" 0
  printf 'running 0 tests\n\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 101 filtered out; finished in 0.00s\n' >"$t/rust-none.log"
  st_check fail "zero tests run (everything filtered out) is rejected" assert_rust_run "$t/rust-none.log" "$t/paths.txt" 0
  printf 'running 1 test\ntest batch::terminal_failure_tests::a_later_success_reconciles ... FAILED\n\ntest result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 100 filtered out; finished in 0.01s\n' >"$t/rust-fail.log"
  st_check fail "a failing gated test is rejected" assert_rust_run "$t/rust-fail.log" "$t/paths.txt" 101
  printf 'running 1 test\ntest batch::terminal_failure_tests::a_later_success_reconciles ... ignored\n' >"$t/rust-ign.log"
  st_check fail "a gated test that stays ignored is rejected" assert_rust_run "$t/rust-ign.log" "$t/paths.txt" 0

  echo "rust discovery"
  mkdir -p "$t/disc/src"
  printf '#[cfg(test)]\r\nmod tests {\r\n    #[tokio::test]\r\n    #[ignore = "requires TEST_DATABASE_URL pointing to disposable PostgreSQL"]\r\n    async fn crlf_gated() {}\r\n\r\n    #[ignore = "requires GRAPH_TEST_DATABASE_URL pointing to disposable PostgreSQL"]\r\n    #[tokio::test]\r\n    async fn attribute_between() {}\r\n\r\n    #[ignore = "requires NEO4J_TEST_URL + NEO4J_TEST_PASSWORD pointing to disposable Neo4j"]\r\n    async fn not_postgres() {}\r\n}\r\n' >"$t/disc/src/lib.rs"
  rust_gated_tests "$t/disc" >"$t/disc.out"
  st_check ok "discovery finds both Postgres gates, with CRLF and attributes in between" test "$(cut -f1,2 "$t/disc.out" | tr '\t\n' ' :')" = "crlf_gated TEST_DATABASE_URL:attribute_between GRAPH_TEST_DATABASE_URL:"

  printf '    #[ignore = "requires TEST_DATABASE_URL pointing to disposable PostgreSQL"]\n    mod not_a_test {}\n\n    async fn unrelated_later_fn() {}\n' >"$t/disc/src/lib.rs"
  st_check ok "a gate marker on a non-fn item is not attached to a later unrelated fn" test -z "$(rust_gated_tests "$t/disc")"

  echo "lint (fixture trees)"
  fx="$t/fx"
  mkdir -p "$fx/services/reg-rs/src" "$fx/services/other-go" "$fx/services/exempt-rs/src" "$fx/tests/contract/pg"
  printf '    #[ignore = "requires TEST_DATABASE_URL pointing to disposable PostgreSQL"]\n    async fn gated() {}\n' >"$fx/services/reg-rs/src/lib.rs"
  printf '    #[ignore = "requires TEST_DATABASE_URL pointing to disposable PostgreSQL"]\n    async fn gated() {}\n' >"$fx/services/exempt-rs/src/lib.rs"
  printf 'package x\nfunc t() { _ = os.Getenv("TEST_DATABASE_URL") }\n' >"$fx/tests/contract/pg/x_test.go"
  st_check ok "registered + exempt gates are accepted" bash -c "$(declare -f lint_fail lint_coverage warn); lint_bad=0; lint_coverage '$fx' 'services/reg-rs'\$'\n''tests/contract/pg' 'services/exempt-rs|run elsewhere'; exit \$lint_bad"
  st_check fail "a gate that is neither registered nor exempt is rejected" bash -c "$(declare -f lint_fail lint_coverage warn); lint_bad=0; lint_coverage '$fx' 'services/reg-rs'\$'\n''tests/contract/pg' ''; exit \$lint_bad"
  printf 'package x\nfunc t() { _ = os.Getenv("WIKI_TEST_DATABASE_URL") }\n' >"$fx/services/other-go/y_test.go"
  st_check fail "a new Go gate in an unlisted service is rejected" bash -c "$(declare -f lint_fail lint_coverage warn); lint_bad=0; lint_coverage '$fx' 'services/reg-rs'\$'\n''tests/contract/pg' 'services/exempt-rs|run elsewhere'; exit \$lint_bad"
  rm -f "$fx/services/other-go/y_test.go"
  st_check fail "a stale exemption (the unit has no gate any more) is rejected" bash -c "$(declare -f lint_fail lint_coverage warn); lint_bad=0; lint_coverage '$fx' 'services/reg-rs'\$'\n''tests/contract/pg' 'services/exempt-rs|run elsewhere'\$'\n''services/gone-rs|stale'; exit \$lint_bad"
  st_check fail "a registered directory without a gate is rejected" bash -c "$(declare -f lint_fail lint_coverage warn); lint_bad=0; lint_coverage '$fx' 'services/reg-rs'\$'\n''tests/contract/pg'\$'\n''services/other-go' 'services/exempt-rs|run elsewhere'; exit \$lint_bad"
  mkdir -p "$t/rr/src"
  printf '    #[ignore = "requires TEST_DATABASE_URL pointing to disposable PostgreSQL"]\n    async fn ok_gate() {}\n    #[ignore = "requires NEO4J_TEST_URL pointing to disposable Neo4j"]\n    fn other_infra() {}\n' >"$t/rr/src/a.rs"
  st_check ok "a registered crate with a recognised gate and an infra ignore is accepted" bash -c "$(declare -f lint_fail lint_registered_rust rust_source_files warn); ACCEPTED_GATE_VARS='$ACCEPTED_GATE_VARS'; OTHER_INFRA_IGNORE='$OTHER_INFRA_IGNORE'; lint_bad=0; lint_registered_rust '$t/rr'; exit \$lint_bad"
  printf '    #[ignore = "requires WIKI_TEST_DATABASE_URL pointing to disposable PostgreSQL"]\n    async fn unknown_var() {}\n' >"$t/rr/src/b.rs"
  st_check fail "a gate variable the runner does not set is rejected" bash -c "$(declare -f lint_fail lint_registered_rust rust_source_files warn); ACCEPTED_GATE_VARS='$ACCEPTED_GATE_VARS'; OTHER_INFRA_IGNORE='$OTHER_INFRA_IGNORE'; lint_bad=0; lint_registered_rust '$t/rr'; exit \$lint_bad"
  rm -f "$t/rr/src/b.rs"
  printf '    fn url() -> Option<String> { std::env::var("TEST_DATABASE_URL").ok() }\n' >"$t/rr/src/c.rs"
  st_check fail "the silent self-gate (env var .ok() then return) is rejected" bash -c "$(declare -f lint_fail lint_registered_rust rust_source_files warn); ACCEPTED_GATE_VARS='$ACCEPTED_GATE_VARS'; OTHER_INFRA_IGNORE='$OTHER_INFRA_IGNORE'; lint_bad=0; lint_registered_rust '$t/rr'; exit \$lint_bad"
  rm -f "$t/rr/src/c.rs"
  printf '    #[ignore]\n    fn bare_ignore() {}\n' >"$t/rr/src/d.rs"
  st_check fail "a bare #[ignore] (it might be a Postgres test that never runs) is rejected" bash -c "$(declare -f lint_fail lint_registered_rust rust_source_files warn); ACCEPTED_GATE_VARS='$ACCEPTED_GATE_VARS'; OTHER_INFRA_IGNORE='$OTHER_INFRA_IGNORE'; lint_bad=0; lint_registered_rust '$t/rr'; exit \$lint_bad"
  rm -f "$t/rr/src/d.rs"

  echo "lint (migrations and workflow)"
  mkdir -p "$t/mig"
  printf 'select 1;\n' >"$t/mig/20260101000000_ok.sql"
  printf 'select 1;\n' >"$t/mig/20260101000000_ok.down.sql"
  printf 'x\n' >"$t/mig/20260101000001_init.sql.disabled"
  st_check ok "well-formed migration names are accepted" bash -c "$(declare -f lint_fail lint_migrations warn); lint_bad=0; lint_migrations '$t/mig'; exit \$lint_bad"
  printf 'select 1;\n' >"$t/mig/2026010100000_short.sql"
  st_check fail "a 13-digit migration (silently ignored by the migrator) is rejected" bash -c "$(declare -f lint_fail lint_migrations warn); lint_bad=0; lint_migrations '$t/mig'; exit \$lint_bad"
  rm -f "$t/mig/2026010100000_short.sql"

  printf 'jobs:\n  postgres-contract-wiring:\n    steps:\n      - run: bash tests/scripts/run-postgres-contract-tests.sh --self-test\n      - run: bash tests/scripts/run-postgres-contract-tests.sh --lint\n  postgres-contract-tests:\n    services:\n      postgres:\n        image: postgres:16\n    steps:\n      - env:\n          REQUIRE_CONTRACT: "1"\n          TEST_DATABASE_URL: postgres://u:p@127.0.0.1:5432/x_test\n        run: bash tests/scripts/run-postgres-contract-tests.sh\n  other:\n    steps: []\n' >"$t/wf-good.yml"
  st_check ok "a correctly wired workflow is accepted" bash -c "$(declare -f lint_fail lint_wiring warn); lint_bad=0; lint_wiring '$t/wf-good.yml'; exit \$lint_bad"
  printf 'jobs:\n  other:\n    steps: []\n' >"$t/wf-missing.yml"
  st_check fail "a workflow without the job is rejected" bash -c "$(declare -f lint_fail lint_wiring warn); lint_bad=0; lint_wiring '$t/wf-missing.yml'; exit \$lint_bad"
  sed 's/REQUIRE_CONTRACT: "1"/REQUIRE_CONTRACT: ""/' "$t/wf-good.yml" >"$t/wf-norequire.yml"
  st_check fail "a job without REQUIRE_CONTRACT=1 is rejected" bash -c "$(declare -f lint_fail lint_wiring warn); lint_bad=0; lint_wiring '$t/wf-norequire.yml'; exit \$lint_bad"
  sed 's/^  postgres-contract-tests:/  postgres-contract-tests:\n    continue-on-error: true/' "$t/wf-good.yml" >"$t/wf-coe.yml"
  st_check fail "continue-on-error on the job is rejected" bash -c "$(declare -f lint_fail lint_wiring warn); lint_bad=0; lint_wiring '$t/wf-coe.yml'; exit \$lint_bad"
  sed 's#image: postgres:16#image: postgres:15#' "$t/wf-good.yml" >"$t/wf-pg15.yml"
  st_check fail "a Postgres 15 service is rejected" bash -c "$(declare -f lint_fail lint_wiring warn); lint_bad=0; lint_wiring '$t/wf-pg15.yml'; exit \$lint_bad"

  echo "admin URL guards"
  st_check ok "a local _test database is accepted" bash -c "$(declare -f die parse_admin_url); export TEST_DATABASE_URL='postgres://u:p@127.0.0.1:5432/x_test?sslmode=disable'; parse_admin_url"
  st_check ok "localhost is accepted" bash -c "$(declare -f die parse_admin_url); export TEST_DATABASE_URL='postgres://u:p@localhost/x_test'; parse_admin_url"
  st_check fail "an unset URL fails instead of skipping" bash -c "$(declare -f die parse_admin_url); unset TEST_DATABASE_URL; parse_admin_url"
  st_check fail "a remote host is refused" bash -c "$(declare -f die parse_admin_url); export TEST_DATABASE_URL='postgres://u:p@db.example.com:5432/x_test'; parse_admin_url"
  st_check fail "a database not ending in _test is refused" bash -c "$(declare -f die parse_admin_url); export TEST_DATABASE_URL='postgres://u:p@127.0.0.1:5432/dataplane'; parse_admin_url"
  st_check ok "db_url swaps the database and keeps the query" test "$(URL_PREFIX='postgres://u:p@127.0.0.1:5432' URL_QUERY='?sslmode=disable'; db_url graph_test)" = "postgres://u:p@127.0.0.1:5432/graph_test?sslmode=disable"
  st_check ok "scrub hides a connection string" test "$(printf 'dial postgres://u:secret@h:5432/db?x=1 failed' | scrub)" = "dial postgres://<redacted> failed"

  echo "registry"
  st_check ok "every registered suite loads with a floor and a bare database" bash -c "$(declare -f die suite_load); SUITES='$SUITES'; for s in \$SUITES; do suite_load \$s; [ \"\$S_MIN_GATED\" -gt 0 ] && [ -n \"\$S_BARE_DB\" ] || exit 1; done"

  if (( ST_FAIL != 0 )); then warn "self-test FAILED"; return 1; fi
  echo "self-test OK"
}

# ── dispatch ─────────────────────────────────────────────────────────────────
main() {
  local mode=run
  local -a chosen=()
  while (( $# > 0 )); do
    case $1 in
      --lint) mode=lint ;;
      --self-test) mode=self-test ;;
      --list) mode=list ;;
      -h | --help) sed -n '2,/^# =====/{s/^# \{0,1\}//;p;}' "${BASH_SOURCE[0]}"; return 0 ;;
      -*) die "unknown option '$1' (see --help)" ;;
      *) chosen+=("$1") ;;
    esac
    shift
  done
  case $mode in
    lint) lint_all ;;
    self-test) self_test ;;
    list) list_all ;;
    run) run_mode "${chosen[@]+"${chosen[@]}"}" ;;
  esac
}

# Sourcing the file (to unit-test its functions) must not run it.
if [[ ${BASH_SOURCE[0]} == "$0" ]]; then
  main "$@"
fi
