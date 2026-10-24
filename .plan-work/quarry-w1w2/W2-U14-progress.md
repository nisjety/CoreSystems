# W2-U14 progress (WS7.F: JobPolicy rename, preset requires, /v1/blocklists 410)

Unit: W2-U14, batch 3, slice WS7.F (plan `docs/plans/quarry-browser-build-plan-2026-10-14.md`, "Slice WS7.F detail"). Build strategy: build, write the named tests, compile-check only; no test suite run.

## Built (scope of the slice)

1. Rust rename. `quarry_core::policy::RunPolicy` is `JobPolicy` (`crates/quarry-core/src/policy.rs`); serde shape unchanged. `crates/quarry-core/tests/contracts.rs` imports and uses `JobPolicy`; the old test `default_run_policy_sane` is `default_job_policy_sane`; new serde golden `default_job_policy_serde_golden` pins the default wire shape. The runtime's own `quarry_runtime::RunPolicy` is untouched. Grep gate: only the runtime type keeps the name.
2. Go preset `requires`. `services/quarry-control/internal/resources/presets.go`: `Preset.Requires []string` (`json:"requires,omitempty"`); `fast` = `robots.max_mode=ignore`; `careful` and its deprecated alias `stealth` = `proxy.max_tier=pool`; alias copies `Requires`. Declared only, not enforced.
3. Go 410. `services/quarry-control/internal/resources/blocklists.go`: `MountBlocklistsGone(r)` answers every method on `/v1/blocklists` and below with 410, typed envelope, code `GONE`, `retryable` false, `details.reference` `docs/POLICY.md`. `MountBlocklists(r, db)` (the CRUD mount) is kept unchanged for rollback. `services/quarry-control/cmd/control/main.go` mounts the 410 (`resources.MountBlocklistsGone(r)`; it was line 200, not 172 as the plan says).
4. Docs: `docs/PRESETS.md` (job presets and requires section, appended), `docs/POLICY.md` (three policy names section, before the RunPolicy sub-policies section), `apps/Frontend Plane/verevonv3/docs/endpoint-map.md` (line 340 family list), `docs/RUNBOOKS.md` (WS7.F section appended, committed through a private index because the working copy carries another unit's uncommitted hunks).

## Tests written (compile-checked, not run)

- Rust: `crates/quarry-core/tests/contracts.rs` `default_job_policy_sane`, `default_job_policy_serde_golden`.
- Go: `presets_test.go` `TestEveryPresetThatExceedsTheBaselineDeclaresRequires`, `TestPresets_RequiresOnTheWire`; `blocklists_test.go` (new) `TestBlocklistsReturnsGone`.

## Verified so far

- `go vet ./internal/resources/ ./cmd/control/` in `services/quarry-control`: exit 0 (compiles the test files too). One earlier vet failure was in `internal/store/store.go` and `schedule_validate.go` (another unit mid-edit); it cleared on retry.
- `cargo check -p quarry-core --tests` through qtest: first attempt failed in `crates/quarry-transform/src/parse_cache.rs` (`ParseCacheKey` lacks `Debug`), a file this unit does not own. Retried; see the result line below.

## Not done / next batch needs to know

- `requires` is not enforced at job creation (WS7.C, WS7.I). The plan named only `stealth`; `careful` also declares it because the alias shares its policy (depth bullet, medium decision).
- The 410 code `GONE` is outside the closed Rust and Go `ErrorCode` sets; the handler writes the envelope itself. A proper variant needs a cross-crate change in `error.rs` (U21 edits it) and `pkg/quarrycontracts/envelope.go`.
- The openapi step in the plan is a no-op: `docs/openapi.yaml` never documented `/v1/blocklists` and has no preset schema.
- Working-tree line endings: the worktree was read as CRLF at the start of the unit (git autocrlf); `git status` shows the Quarry docs and code as usual. `grep -c $'\r'` returned 0 in this shell even for CRLF files, so use `file` for EOL checks.
- Evidence owed (depth): rebuild quarry-control and run the HMAC-signed 410 / presets / polite 201 checks.
- The edge `/v1/security/blocklist` shim described in `docs/GAP.md` has no code path in the tree.

## Files of this unit

Committed (when the commits below land): see the W2-U14 commit list in the final answer. Uncommitted and not ours: `docs/RUNBOOKS.md` hunks other than the WS7.F section (another unit's, still in the working copy).
