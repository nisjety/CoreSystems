# W5-S4 progress: F-t2-068 clear-data built behind a default-off flag, tested, not run live

Date: 2026-11-08. Branch `quarryv2-next` (local only), on top of `94325fc0c`. Code commits, in order: `feat(quarry-browser)`, `feat(quarry-edge)`, `feat(gateway)`, `feat(verevonv3)`, then this note.

## Built (committed)

- **Core**, `quarry-browser/src/session.rs`: `SessionSnapshot::browsing_data_counts` and `without_browsing_data`, and the `BrowsingDataCounts` type. A clear removes cookies, web storage and IndexedDB, flat and per origin. It keeps the user agent, viewport, locale, timezone and capture facts. Counts carry no name or value.
- **Edge**, `quarry-edge`: `POST /v1/profiles/:id/clear-data`.
  - `dry_run` defaults to true. A call that does not send `"dry_run": false` only counts.
  - Owner only, also on an org-shared profile (`ProfileOp::ClearData`).
  - A ZDR organization is not refused: the route is a wind-down, like delete (decision 3).
  - 409 `profile_in_use` while a run in the run registry holds the profile. The clear never reads or changes a live session.
  - 409 `profile_busy` after three stale attempts.
  - 501 `profile_clear_data_disabled` unless `QUARRY_EDGE__PROFILE_CLEAR_DATA` is on. Default off, read at boot.
  - `GET /v1/profiles` carries `clear_data_enabled`.
- **Gateway**: `POST /api/v1/browser/profiles/{profile_id}/clear-data`, `dryRun` defaulting to true. The edge's in-use refusal becomes `browser_profile_in_use`. Other refusals pass through.
- **Verevon**: a count-first control in the pre-session profile row. It shows only when the profile list says `clear_data_enabled`. The flow is arm, dry run, show counts, then Clear or Cancel. The clear is sent only on confirm. Also: the client call, an `Eraser` icon, a helper module with tests, and the `browser_profile_in_use` message.
- **Docs**: OpenAPI path `/v1/profiles/{profile_id}/clear-data`, an ERROR_REASONS table, a RUNBOOKS section, and the route-count re-pin in `scripts/openapi-route-allowlist.json` (120 to 121).

## Not built, and why (decisions for the orchestrator)

1. **Action contract in `src/shared/actions`: not added.** W5-ORCH asked for it first. Every registry row is pinned in the Model Plane golden `apps/Model Plane/api/contracts/agent-action-admission.json` (digest in `scripts/tests/zdr-contract-sync.test.sh`), and three Model Plane consumers vendor it. A new row changes that golden, which needs a Model Plane owner change and a re-vendor. That is outside W5. The UI calls the REST route directly, as rename and delete do. Owner decision: should clear-data be a model-visible, approval-required action at all?
2. **Live jar.** The clear never touches a live session. The guard refuses while a run holds the profile. Residual window, not closed: a run already removed from the registry but not yet released can save its jar again after a clear.
3. **ZDR.** W5-ORCH says "no-op under ZDR". This slice treats clear as a wind-down, like delete: it is served, and a ZDR organization holds no profile written under ZDR, so in practice nothing is cleared. Confirm.
4. **Owner-only on org-shared profiles.** W5-ORCH is silent. Members can attach and save a shared profile; only the owner can clear it.
5. **W5-S1b (gateway PDF arm, 069 step 1) is not needed by S4 and is not done.** It stays the open next step in W5-ORCH2.
6. **Base.** W5-ORCH2 says to cut W5 slices from `main`. This slice is on `quarryv2-next`, as instructed. It touches `profile_routes.rs`, `profile_guard.rs`, `routes.rs`, `session.rs`, the gateway `browser.rs` and `KnowledgeComposer.tsx`, so check for conflicts before cherry-picking onto `main`.

## Ran

| Check | Where | Result (pass / fail) |
|---|---|---|
| `cargo test -p quarry-browser --lib -j 2` | snapshot | 214 / 0 |
| `cargo test -p quarry-edge --lib -j 2 -- profile` | snapshot | 25 / 0 |
| `cargo test -p quarry-edge --test profile_clear_data --test profile_owner --test profile_guard --test profile_roundtrip --test profile_attach --test api_key_fence --no-fail-fast -j 2` | snapshot | profile_clear_data 7/0; profile_owner 8/0; profile_roundtrip 11/0; profile_attach 10/0; api_key_fence 7/0; profile_guard 7/1 (see below) |
| `cargo test --manifest-path "apps/Frontend Plane/verevonv3/apps/gateway/Cargo.toml" clear_data -j 2` | worktree | 2 / 0 |
| `pnpm exec vitest run src/features/dashboard/home/browser-profile-clear.test.ts` | worktree | 4 / 0 |
| `pnpm exec vitest run src/features/dashboard/home src/shared/api/browser-client.errors.test.ts src/shared/api/browser-client.test.ts src/shared/i18n --maxWorkers=2` | worktree | 63 files, 876 / 0 (run before a type-only fix; the helper file was re-run after it, 4 / 0) |
| `pnpm exec tsc -b` | worktree | 0 errors in touched files; 22 pre-existing lines (`@quarry/client` build step not run here) |
| `pnpm exec eslint` on the six touched SPA files | worktree | 0 errors; 7 warnings, all on pre-existing lines of `KnowledgeComposer.tsx` |
| `cargo test -p quarry-edge --lib --no-run` warnings | snapshot | 7 warnings, all pre-existing (`attach.rs`, `fleet_routes.rs`, existing tests in `agent_routes/tests.rs`); none in touched code |
| `node --test` on `openapi-route-coverage`, `openapi-no-bare-null`, `openapi-internal-guard` | Quarry-v2 | 21 tests, 19 pass, 2 fail (both pre-existing, below) |

"Snapshot" is a clean `git archive` of HEAD with this slice's 15 Quarry files overlaid, checked by SHA-256 against the worktree. The shared worktree was briefly in a state that did not compile (another writer's `FillOtpRef` edit in `quarry-core` and `quarry-runtime`), so the Quarry runs use the snapshot. The gateway and SPA runs were in the worktree; the only changes there are this slice's.

## Failing, and not caused by this slice

- `profile_guard::import_over_cap_is_413`: a 5 MiB POST gets `ConnectionAborted` (10053) on Windows, because the server answers 413 and closes before it reads the body. With my overlay reverted to HEAD in the snapshot, it fails the same way. Pre-existing and platform-specific. Not in this diff.
- `openapi-route-coverage`, `every_registered_route_is_documented_or_allowlisted_with_a_reason`: `/v1/audit/page` is registered at HEAD and is in neither `docs/openapi.yaml` nor the allowlist. Red at HEAD.
- `openapi-internal-guard`, SDK check: untracked generated `sdks/typescript/src/apis/InternalApi.ts` and two `InternalRunPage` models, from another writer's generation. Not mine, not touched.

## Not run

- Live stack, real browser, Postgres, Redis, S3 and envelope stores.
- SDK regeneration (needs Docker). The generated SDK does not yet include the new route.
- Full quarry-edge suites (about 100 binaries; a full link ran out of Windows page file in W5-VERIFY).
- Gateway `cargo fmt --check` and `clippy -D warnings` (CI parity).
- `docs-contract.test.mjs` and `zdr-contract-sync.test.sh`.

## Hygiene

- Line endings: the index is LF. My edited files had CRLF checkout copies, so I converted those files to LF. The commits carry only real changes (880 insertions, 13 deletions).
- A `rustfmt` write run reformatted `tests/support/*`. I restored `fixture_site.rs` and `support/mod.rs` to HEAD and re-applied the one-line change. Pre-existing rustfmt drift in the files I edited is left alone.
- Not mine, not committed: `quarry-runtime/src/step_receipts.rs` (modified), untracked `sdks/typescript/src/**`, `rb.patch`, `.plan-work/next-slices.md`, `.plan-work/quarry-w1w2/W5-VERIFY.md` and `W5-VERIFY2.md`.

## ADR-5 fence

No UA change, no fingerprint or `navigator` masking, no identity change on a block, and no CAPTCHA or consent automation. The clear acts only on the stored snapshot of a profile that no live run holds.

## Proven

Tests only (counts above). Nothing is proven live.
