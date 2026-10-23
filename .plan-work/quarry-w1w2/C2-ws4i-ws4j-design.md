# C2 design note, round 5 (retry): finish WS4.J to the counsel gate

Reviewer: Opus design pass, 2026-10-23, HEAD b8b7495b. No repository file edited, no command run except git log/diff/status and reads. The original design (2026-10-21) is kept unchanged below the line; where they differ, this section wins.

## 0. Where it stands (from git and the hand-over `wf-C2-ws4i-ws4j.md`)

- Committed: `9ff290dd` (WS4.I), `90634d87` (Go half), `0ef06f3b` (edge half, **tests never run, gitleaks skipped**), `7849ff1b` (ledger, gitleaks skipped). RUNBOOKS section landed inside `720ae31f`.
- Uncommitted and C2's (round 4, scripts in `scratchpad/C2r4/`, cut off at 23:58 on 10-21): new `migrations/024_zdr_tombstone_erasures.sql`; erasure markers in `store/zdr_tombstones.go`, `store/pg/zdr_tombstones.go`, `store/pg/gdpr_purge.go`; the retention overflow fix in `janitor/tombstones.go` (+ tests); the signed no-read-route test in `resources/zdr_tombstone_test.go`; `TombstoneEmitter::with_limits` plus a `zdr-log-ok` comment in `zdr_tombstone.rs`; three new tests in `tests/zdr_tombstone.rs`.
- NOT C2's (leave alone, never commit): the `main.rs` hunk (`driver.shutdown()`), the RUNBOOKS round-4 D3 section, CONTRACTS.md / openapi.yaml (`taken_over_at`), every `agent_routes/*` and quarry-browser hunk.
- Gap: the round-4 Go code has **no test** for markers, migration 024 or the lock.

## 1. Contracts that change in round 5

- **Migration 024** (024 is free: 015, 019, 020, 023 used): `zdr_tombstone_erasures(org_id TEXT PK CHECK shape, erased_at TIMESTAMPTZ DEFAULT now())` plus an index on `erased_at`. A separate file is right: 015 may already be recorded in `schema_migrations`. It is idempotent (`IF NOT EXISTS` only). The runner applies by name, so on any DB it runs after 015.
- **Store interface:** `ZdrTombstoneStore` gains `DeleteErasureMarkersBefore(cutoff)`. `Put` returns `(false, nil)` for an erased org, the same answer as a duplicate run id. The receiver still answers 204, so there is no erasure oracle. Only the mem and pg stores implement the interface (grep found no fakes).
- **Sweeper:** `RETENTION_DAYS` above 36500 is invalid and falls back to 30. `tombstoneCutoff` uses `AddDate`, so no `Duration` overflow can put the cutoff in the future. Markers are swept at `max(retention, MinErasureMarkerDays=7)`. A dry run returns before the marker sweep, so it deletes nothing, which is correct.
- **Edge:** `with_limits(sink, in_flight, timeout)`. `with_capacity` keeps `SEND_TIMEOUT`. There is no wire change.

## 2. Security and concurrency argument

- **Late tombstone after erasure.** Without markers, a run that ends after `PurgeOrg` re-creates a row for an erased org. That breaks the GDPR purge. The fix is a marker written in the purge transaction, plus `INSERT … SELECT … WHERE NOT EXISTS(marker)` in `Put`.
- **The race.** A `Put` could check for a marker before the purge commits and insert after the purge's DELETE. Both paths take `pg_advisory_xact_lock(hashtext('zdr_tombstone_org:'||org))` first. A hashtext collision only serialises two orgs; it is never wrong. `Put` is now one short transaction per tombstone. That is acceptable: the edge is bounded at 64 in flight, and the feature is off.
- **Marker content.** An org id and a timestamp, no ZDR content. It is created only for an id of tombstone shape, since any other id can never hold a tombstone. A marker blocks the org's tombstones until it is swept, which is fail-closed in the privacy direction.
- **Retention.** A huge value can never delete fresh rows. An invalid value never disables the sweeper. These rules are unchanged from the original design.
- **Still OFF.** No env, compose file or default sets `QUARRY_EDGE__ZDR_TOMBSTONE`. Counsel gates enabling it. The RUNBOOKS text already says so; keep it.

## 3. Order of work (no hot file is edited this round)

1. **Go tests for the round-4 code** (owned files):
   - Memory store: `TestErasedOrgGetsNoLateTombstone`, `TestErasureMarkerOutlivesAZeroRetention` (retention 0, marker 1 day old, survives), `TestSweepDeletesMarkersPastTheFloor`, `TestErasureMarkerOnlyForATombstoneShapedOrgID`.
   - pg (`QUARRY_REQUIRE_DB`): `TestMigration024AppliesTwice`, `TestPostgresLateTombstoneAfterErasureIsRefused`, `TestPostgresPurgeAndPutSerialiseOnTheOrgLock` (tx A holds the lock; a goroutine's `Put` must not return before A commits; after the commit the Put reports `inserted=false`), `TestPostgresErasureMarkerSweep`.
   - Run natively: `go vet ./... && go test ./... -count=1` in quarry-control. Then `scratchpad/C2/run-pg.sh` with `-run 'Zdr|Tombstone|Erasure|Migration01|Migration02|PurgeOrg'` under the docker lock.
2. **Mutations on a COPY:**
   - Drop `NOT EXISTS`.
   - Drop the lock in `Put` (the serialisation test must fail).
   - Drop the marker INSERT.
   - Set `MinErasureMarkerDays` to 0.
   - Revert `AddDate` to a `Duration`.
   - Revert the upper bound.
   - Make the dry run reach the marker sweep.

   Each must be killed. Record survivors honestly.
3. **Commit A** (Go): migration 024, the three store files, `gdpr_purge.go`, janitor and resources tests. Check `git diff` first: today every hunk in them is C2's.
4. **Edge verification, owed since 10-21.** One qtest call:
   ```
   set -o pipefail; cargo test -p quarry-edge --test zdr_tombstone --test log_field_discipline --test zdr_feature_matrix; cargo check -p quarry-edge --bins; echo QTEST_DONE rc=$?
   ```
   `--bins` matters because the `main.rs` tombstone wiring has never compiled. Then run the full `cargo test -p quarry-edge --no-fail-fast`. If a failure is in a foreign file, wait and re-run, at most 5 times.
5. **Commit B** (edge): `zdr_tombstone.rs` and `tests/zdr_tombstone.rs` only.
6. **Rust mutations** with `scratchpad/C2/mut-rs-inner.sh` on a copy, with its own volume removed afterwards:
   - An extra field on the wire struct.
   - Emission for a standard run.
   - Awaiting instead of spawning.
   - Removing the semaphore drop.
   - Removing the `with_limits` timeout.
7. **Owed gitleaks scan** of `0ef06f3b` and `7849ff1b`, under `with-lock.sh docker`:
   ```
   git show <sha> | docker run -i --rm -v "$PWD:/repo" ghcr.io/gitleaks/gitleaks detect --pipe --redact --config /repo/.gitleaks.toml
   ```
   Record the result in RUNBOOKS.
8. **RUNBOOKS, append only**, committed via a filtered patch because the file has foreign hunks. Record the erasure marker, the 024 migration, the overflow fix, the gitleaks result and the test counts.
9. Update `C2-ws4i-ws4j-progress.md` and `wf-C2-ws4i-ws4j.md` after each step.

## 4. Will NOT do

- Enable the tombstone or add any default that turns it on.
- Run the evidence on a stack (it needs the flag on, which happens after counsel).
- Touch `main.rs` (its diff is another unit's), CONTRACTS, openapi or `agent_routes`.
- Add new catalogue keys, wire `errors.ts` (D1), or merge `model-plane`.
- Add an HTTP read of tombstones.
- Amend old commits; their claims are corrected in RUNBOOKS.
- Run Docker recovery if the engine wedges again: stop, and report it in not_done.

---

# Original design (2026-10-21, superseded where section above differs)


Reviewer: Opus design pass, 2026-10-21. No repository file edited. Sources: plan lines 954-955, 1018-1023, 3667, R-2 (507); breakdown item h; C1 progress; the files named below as read at HEAD 96c81447.

## 1. Facts that change the plan text

- Migration 015 is still free (`migrations/` holds 001-014, 019, 020, 023; C1 took 023). Use `015_zdr_run_tombstones.sql`. The runner (`pg.go` 160-205) applies by name against `schema_migrations`, so a late 015 runs on databases already at 023. It takes no advisory lock: two replicas racing is pre-existing; the migration must be idempotent anyway.
- The catalogue source is `5ae7fe13` on `model-plane`: `apps/Model Plane/api/contracts/zdr-refusals.json` (sha256 `9c2b5458…6d1e`, its `.sha256` agrees), SPA `src/shared/api/{zdr-refusals.ts, zdr-refusals.test.ts, contracts/zdr-refusals.json(.sha256)}`. None exists on this branch. 10 codes, 30 features; the four new Quarry keys are absent (X-16: existing codes only).
- The sync script's VENDORED check (`check_vendored`) fails when the golden is missing, so a Quarry copy alone cannot be vendored. The other three MP goldens are byte-identical on both branches, so vendoring the catalogue golden at its own MP path is merge-safe.
- `cmd/control/main.go` lines moved: the HMAC group is 167-208, `janitor.Run` is 217. `janitor.go` 14-18 forbids arming the artifact janitor; the tombstone sweeper is separate.
- `AppState` struct literals that list every field: 10 sites in 8 files (the same set that carries `zdr_refuse_durable_resources:`). The rest use `..base_state`.
- No `jsonschema` crate or Go module is present. The schema check is a small key/enum/pattern checker driven by the schema file, plus typed structs. No new dependency.

## 2. WS4.I contracts

Copy these with `git show 5ae7fe13:<path>`, byte-identical and LF, recording the commit in the sync rows and the commit message: the MP golden plus `.sha256` at its own path, the SPA four files, and `crates/quarry-runtime/tests/fixtures/zdr-refusals/zdr-refusals.json(.sha256)`. Sync script rows (append, one block per array): GOLDENS gets the MP row verbatim from model-plane line 109. VENDORED gets the Quarry pair and the SPA pair (model-plane 228-229 verbatim). CONSUMERS gets `…|quarry-edge|apps/Ingestion Plane/Quarry-v2/|WS4.I` and the SPA row (model-plane 288). Once the golden is in GOLDENS, `check_schema` passes `--refusals`, and this branch's `zdr-contract-schema.mjs` (1004-1039) then requires every posture-matrix code to be in the catalogue. Run it. Do not port model-plane's `ZDR_REFUSALS_CATALOGUE` env change unless the run fails. `zdr_feature_matrix.rs`: replace `CATALOGUE_CODES`/`CATALOGUE_FEATURES` (342-397) with a parse of the vendored file via `include_str!("../../quarry-runtime/tests/fixtures/zdr-refusals/zdr-refusals.json")`, add a sha256 check against its pin (sha2 is already a dev-dep), and keep the `PENDING_*` lists and the stale/adopted checks. The SPA files are added but not wired: `errors.ts` belongs to D1-ws5g.

## 3. WS4.J contracts

Allowlist (schema `quarry-runtime/contracts/zdr-tombstone.schema.json`, `additionalProperties:false`):
- `run_id` ^[A-Za-z0-9_-]{1,64}$
- `org_id` same pattern
- `kind` enum scrape|search|answer|extract|agent_run
- `retention_class` const zdr
- `outcome` enum ok|error|refused|cancelled|timeout
- `driver` closed enum of DriverRegistry names (grep at build)
- `started_at`, `ended_at` RFC3339
- `step_count` int ≥0
- `cost_units` number ≥0
- `error_class` enum from the existing `zdr_log` classes

There is no `zdr` key, no URL, title, query, user id, text, digest or fingerprint. Required fields: `run_id`, `org_id`, `kind`, `retention_class`, `outcome`, `ended_at`.

Wire: `POST /v1/internal/zdr-tombstones` is mounted inside the `hmacVerifier.Middleware` group (signature, skew and nonce come from that). It reads a 4 KiB `MaxBytesReader` body into a typed struct with `DisallowUnknownFields`. Any string containing `://` is 400 `BAD_REQUEST`, and so is a pattern or enum miss. A body over the cap is 413. The handler returns 204 for both insert and duplicate (`ON CONFLICT (run_id) DO NOTHING`, first write wins, no existence oracle). No HTTP read route: `ListByOrg` is store-only. Errors use the existing envelope and existing codes only.

SQL: `CREATE TABLE IF NOT EXISTS` with the CHECKs from the plan detail, `CREATE INDEX IF NOT EXISTS` on `(org_id, ended_at DESC)` and `(ended_at)`, `CREATE OR REPLACE FUNCTION` that RAISEs, `DROP TRIGGER IF EXISTS` and then `CREATE TRIGGER … BEFORE UPDATE … FOR EACH ROW`. The trigger refuses UPDATE only, so DELETE stays allowed for the sweeper and for erasure. There is no FK to anything outside the table.

Store: `ZdrTombstones()` on `store.DB` with `Put`, `DeleteBefore(cutoff, limit)` and `ListByOrg`, in both the memory and pg stores. `PurgeOrg` gains `DELETE FROM zdr_run_tombstones WHERE org_id=$1` and `PurgeResult.ZdrTombstonesDeleted`.

Sweeper (`janitor/tombstones.go`): runs hourly with batched `DeleteBefore(now-days, 1000)`. `QUARRY_ZDR_TOMBSTONE_RETENTION_DAYS` is 30 when unset and 0 deletes everything older than now (that is how the evidence step works). An invalid value logs and falls back to 30; it never disables the sweeper. `QUARRY_ZDR_TOMBSTONE_DRY_RUN` defaults to false. This is a deliberate departure from the artifact janitor: deleting metadata is the privacy-safe direction and nothing references a tombstone. The ledger must record this decision.

Edge: `zdr_tombstone.rs` has a `TombstoneSink` trait and an HTTP impl that signs with `internal_signer`. Add one `AppState.tombstones: Option<Arc<dyn TombstoneSink>>` field: `None` at all 10 literals, and `Some` only in `main.rs` when `QUARRY_EDGE__ZDR_TOMBSTONE=1` and both `control_base_url` and the signer are set. The config default is off in `config.rs`, append-only.

## 4. Security and concurrency argument

- **Egress is closed by construction.** The edge serializes a typed struct, so it cannot name a field outside the allowlist. Control rejects unknown fields and `://` values. Both struct key sets are tested equal to the schema `properties`.
- **Emission only when ZDR is in effect.** The trigger is `claims.effective_zdr(...)` being true; a standard run emits nothing. The `retention_class='zdr'` CHECK makes a standard row impossible to store.
- **Fail-open, and bounded.** Emission is `tokio::spawn` with a 2 s timeout behind a `Semaphore(64)`. When the semaphore is full, the tombstone is dropped and counted, never awaited. The edge logs only counters and classes, which the log-field lint enforces. A lost tombstone loses audit evidence; it enforces nothing.
- **Races.** A scrape emits once at handler end. An agent run emits only from the `close_run` path that wins removal: the reaper and an explicit close can race, and the primary key backs that up. Retries are idempotent.
- **Authority.** Only the HMAC-holding edge can write. A forged `org_id` needs the shared secret, the same trust as the change webhook.
- **ZDR consequence.** A tombstone retains that a ZDR run happened. That is metadata retention, and it is why counsel gates enabling it. Nothing in any env file, compose file or default turns it on.

## 5. Order of edits (hot files last, exact-anchor)

1. WS4.I fixtures, sync rows and matrix test; commit.
2. Go: migration, store (memory and pg), handler, sweeper, purge, then the `main.go` mount and start; commit.
3. Schema plus pin, the vendored testdata copy and the sync rows.
4. `zdr_tombstone.rs` and `lib.rs` `pub mod`, then the `state.rs` field and the 10 literals in one sweep, then the emission calls in `routes.rs` (scrape, re-read first), search, answer and extract, and `agent_routes/lifecycle.rs` `close_run`. The last one is a lane-B file: one anchor edit, reported.
5. RUNBOOKS (append) with the counsel gate stated.

## 6. Tests

- **Go:** `TestZdrTombstoneRejectsAnyFieldOutsideTheAllowlist`, `TestZdrTombstoneRejectsUrlLookingValues`, `TestZdrTombstoneIsIdempotentOnRunID`, `TestZdrTombstoneStructMatchesSchema`, `TestZdrTombstoneRowCannotBeUpdated` and `TestZdrTombstoneMigrationAppliesTwice` (both on Postgres, under `QUARRY_REQUIRE_DB`, throwaway container), `TestSweepDeletesOnlyPastRetention`, `TestSweepZeroDaysDeletesAll`, `TestOrgErasureRemovesTombstones`, `TestUnsignedTombstoneIs401`.
- **Rust** (`tests/zdr_tombstone.rs`, capturing sink): `a_zdr_scrape_emits_exactly_one_tombstone_without_any_url`, `a_standard_scrape_emits_none`, `a_tombstone_failure_never_fails_the_request`, `the_emitted_body_validates_against_the_schema`, `the_flag_defaults_off`, `a_closed_agent_run_emits_once`.
- **Mutations** on a workspace copy, each of which must fail a test: add a field to the edge struct, remove `://` rejection, drop the trigger, remove the purge line.

## 7. Not done by this unit

- Enabling the tombstone anywhere.
- New catalogue keys (Model Plane owner).
- SPA wiring of Quarry refusals (`errors.ts`, D1).
- Plan-based retention from Control Plane.
- An HTTP read of tombstones.
- Merging `model-plane`.
- Counsel review.
