# W23 verify 5: W3 and W4 targeted lanes, flag-default audit, Model Plane status (2026-11-08)

Targeted cargo only, `-j 2`, no code edited. Builder files were not touched.

- W3 lane: worktree `innh-lane-integ` (branch `w23/integ`, HEAD `7a8f67b14`), target `C:\qv2t-w3v2`.
  - **The tree was not clean.** A builder had uncommitted work: `quarry-runtime/src/pipeline.rs` (+107/-19, staged, adds `catch_scanner_panic`), `quarry-runtime/src/artifact_store.rs` (staged, 2 lines, uses `catch_scanner_panic`) and `quarry-edge/src/routes.rs` (unstaged, +9/-1). The W3 numbers below are HEAD plus that WIP, not the commit alone.
- W4 lane: worktree `gracious-gagarin-087139` (branch `quarryv2`, HEAD `b1753710a`), target `C:\qv2t-w4v2`, `CARGO_INCREMENTAL=0`. Tree clean for `apps/`.

## Results

| Lane | Command | Result |
|---|---|---|
| W3 | `cargo test -p quarry-runtime --lib -- screenshot redaction otp one_time pipeline artifact_store` | 194 passed, **9 failed** (all `artifact_store::tests::fs_store_*` and `both_stores_list_the_same_shapes`, Windows only, see below) |
| W4 | `cargo test -p quarry-core -p quarry-edge --lib -- scrape_contract scrape emulation`, quarry-core lib | 67 passed, 0 failed |
| W4 | same command, quarry-edge lib | 95 passed, 0 failed |

Totals: 356 passed, 9 failed. Excluding the 9 environmental failures, W3 has 0 failures in the screenshot, redaction, otp and pipeline filters.

Not run in this pass: the quarry-edge integration tests (`scrape_actions`, `scrape_capture`, `scrape_max_age*`, `scrape_records_durable`). Verify 3 and verify 4 ran them at the same W4 tip, with 31 passed and the 4 known Windows failures. The tip has not changed since.

## The 9 W3 failures are not regressions

Each failing test fails on a filesystem write: `fs write ...\org=<org>/run=<run>/page=blake3:<hash>/html.html: Mappenavnet er ugyldig. (os error 267)`. The `FilesystemStore` builds a directory named `page=blake3:...`, and `:` is not a legal directory character on Windows. It is the same defect as the 4 `scrape_records_durable` failures in verify 4. The staged edit to `artifact_store.rs` is only an import swap to `catch_scanner_panic`, and the failures sit in the fs write path. I did not run the 9 tests at a clean HEAD to prove they failed before the WIP, so "pre-existing" rests on that reading. They should pass on Linux and CI. Fix, in a separate slice: encode or replace `:` in the page path segment when `cfg(windows)`. It is now 13 failing tests (4 edge, 9 runtime) behind one store-key bug.

## Flag-default audit

Compose files checked: `apps/Ingestion Plane/docker-compose.yml` and `Quarry-v2/deploy/compose/docker-compose.yml`, in both worktrees. They are identical for these flags.

| Flag | Code default | In compose? | Verdict |
|---|---|---|---|
| `QUARRY_EDGE__FILL_OTP` (W3 E3.1) | off (`matches!` on `1/true/yes/on`, unset is false) | no | OK, off |
| `QUARRY_EDGE__PII_REDACTION` (E3.6-4a) | `PiiRedactionMode::Off` (`#[default]`) | no | OK, off |
| `QUARRY_EDGE__PII_REDACTION_DEFAULT_ORGS` | empty, and matters only while the stage is on | no | OK |
| `QUARRY_EDGE__PER_PAGE_LOCALE` (W4 E4.4-a) | off (`matches!` on `on/1/true`) | no | OK, off |
| `QUARRY_EDGE__SCRAPE_CAPTURE` | off | compose `:-off` | OK |
| `QUARRY_EDGE__SCRAPE_ACTIONS` | off | compose `:-off` (both files) | OK |
| `QUARRY_EDGE__REDACTION_MODE` (WS12.S2 secret masking, a different flag) | enforce | compose `:-enforce` | OK, a tighten-only guard |
| `QUARRY_EDGE__SCREENSHOT_PREPROCESSING_ENABLED` | `false` in `config.rs` | `deploy/compose/docker-compose.yml:145` sets `"1"` | **Note.** Not W3/W4: the line arrived in baseline commit `9a9ac7fca`. The `main.rs` code and the `vision.rs` default (`true`) differ from the edge config default (`false`). Flagging it because it turns screenshot preprocessing on in the deploy stack. Not an E3.x flag. |

All W3 and W4 flags default off and none is set in compose. One more W3 thing to know: the CW-2 refusal (a redacting run refuses a capture) fires only for the org default `QUARRY_EDGE__PII_REDACTION_DEFAULT_ORGS` on the agent path. A per-run `redact_pii` is ignored there (`observation.rs` `applies(false, ..)`, from verify 3, still open).

## Environment

- `C:` had 0.5 GB free when I started. I deleted `C:\qv2t-w3v2\debug\incremental` (9.7 GB, a rebuildable cache in the W3 verify target dir, no cargo or rustc process running) and left `qv2t`, `qv2t-w4v2` and everything else alone. Free space was 8.9 GB after both builds. `qv2t-w3v2` was 27.5 GB before the delete, `qv2t-w4v2` 38.5 GB, `qv2t` 80.5 GB.
- Live Chromium tests (`#[ignore]`) did not run.

## Model Plane status

Read from files only. No Model Plane tests were run in this pass.

- `apps/Model Plane/docs/PLAN.md`: the phase 0-9 plan. Its last commit is `0c2d43580` (2026-08-05, the Velion to Verevon rename). Phase markers, all stale against the ledger:
  - Phase 0 (Canonical truth): 🟡 in progress.
  - Phases 1 to 8 (orchestration shell, capability platform, plan mode, tasks/cron, multimodal, clients, knowledge plane, token efficiency): ❌.
  - Phase 9 (Verification and cutover): 🟡 partial.
  - It has no W2/W3 rows. A reader of PLAN.md alone would think nothing was built. The progress lives in the ledger and the status files below. I did not edit PLAN.md.
- Ledger `apps/Model Plane/docs/decisions/ledger.md` (572 KB, last edited 2026-11-05). Latest entries:
  - 2026-11-05: a goal continues unattended only read-only, in service identity (EP-7 S7).
  - 2026-11-04: wave 3 owner decisions taken at their recommended options.
  - 2026-11-03: `mp-w2w3` merged into `model-plane`; `host_class` moves to field 11.
  - Also 2026-11-03: EP-13 S4 side-task model roles, EP-6 S2 Run now, EP-6 S0 Automations hub.
  - 2026-11-02: W3 Model Plane halves built and compile-checked only; W2 halves built and tested.
  - Recent Model Plane commits on main: `b569929d0` (2026-11-05, main merged into model-plane), `8042b2d02` (EP-10-S8 `suggest_connector` tool).
- `MP-W2W3-STATUS.md` (from verify 4, unchanged): W2 rows 40 built and tested in process, flags default off, tighten-only guards on. W3 rows 11 built and compile-checked only (E3.2-a/c/d/e1/e2, E3.3-f/i/l, E3.6-8, E3.7.10, E3.9.3/4). Last regression run was 2026-11-02: `execution-core --lib` 1763 pass and 2 fail (one load flake, one real W3 bug fixed in `4eae88bd3`); `model-gateway --lib` 2650 pass and 1 pre-existing fail; `session-core` bin 705 pass. Not rerun today.
- `MP-ASKS.md`: 38 `##` sections (the file lives in `.plan-work/quarry-w1w2`, last commit `77a14bf28`, 2026-11-07). It has two halves:
  - Asks from the Frontend and Quarry lanes to the Model Plane owner: one section per unit, W2-FE-L5a through W3-E3.2-g1.
  - Asks from the Model Plane to other planes: lane batches from `mp-w2w3`, ending at the E3.2-c, E3.3-f, E3.3-l, E3.2-e2 and E3.2-a execution-core halves, then the `fill_otp` approval record fields (E3.1-i), E3.7.10-CP and E3.2-e2 attach consent. No section is marked closed in the file, so I count all 38 as open, but treat that count as unreliable.
  - Newest ask (W3-E3.2-g1): copy the regenerated SPA admission golden (sha256 `fac37b71...3cb9`, two new org browser policy rows) over the canonical `agent-action-admission.json` plus its 3 vendored copies, and move the digest pins. Until the owner does, the `zdr-contract-sync` rows and `agent-action-admission-proof.sh` stay red. This is needed before `BROWSER_POLICY_ADMIN_API` or `VITE_BROWSER_POLICY_SETTINGS` is turned on anywhere.
  - Other asks that gate flags: the broker `RenewGrant` principal decision (W2-U39b), the `api_key_revoked` transport (W2-CP-INNH10B), and OD13 counsel sign-off before E3.2-e1 and the attach consent check can ship.
- Not verified live, unchanged from verify 4: the Quarry-side halves (mark route, `warnings[]`, `settle_ms`, `crawl:write`), real JetStream consumers, real Auth Core key tokens.

## Open items for the lead

1. Builders' uncommitted W3 work (pipeline.rs, artifact_store.rs, routes.rs) was not verified as a commit. Re-run after they commit.
2. One Windows `:` store-key bug now fails 13 tests (4 edge, 9 runtime). Decide whether to fix it for Windows dev or leave it to CI.
3. PLAN.md phase markers are stale (Phases 1 to 8 show ❌). Model Plane owner decision whether to refresh them.
4. Still open from verify 3: CW-2 ignores a per-run `redact_pii` on the agent path, the CW-2 Pdf arm has no test, and a cosmetic `unused import: TEST_PII_REDACTION` warning.
5. `deploy/compose` turns screenshot preprocessing on while the config default is off (pre-existing, not W3/W4).
