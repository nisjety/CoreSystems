# W23 verify 4: W3 and W4 gate lanes (2026-11-08)

Targeted cargo only, `-j 2`, isolated target dirs. No code edited. `w23/integ` `quarry-edge/routes.rs` (Haiku writer) not touched.

- W3 lane: worktree `innh-lane-integ` (branch `w23/integ`, HEAD `7a8f67b14`), target `C:\qv2t-w3v2`. Tree clean for `apps/Ingestion Plane`.
- W4 lane: worktree `gracious-gagarin-087139` (branch `quarryv2`, HEAD `b1753710a`), target `C:\qv2t-w4v2`. Tree clean for tracked files. An untracked `quarry-edge/tests/scrape_dbg_tmp.rs` is not mine and was left alone.

## Results

| Lane | Command | Result |
|---|---|---|
| W3 | `cargo test -p quarry-runtime --lib -- screenshot redaction pipeline` | 150 passed, 0 failed |
| W4 | `cargo test -p quarry-core -- scrape` (lib 63, `contracts` 1) | 64 passed, 0 failed |
| W4 | `cargo test -p quarry-edge --lib -- scrape emulation per_page_locale page_emulation` | 95 passed, 0 failed |
| W4 | `quarry-edge --test scrape_actions` | 9 passed |
| W4 | `quarry-edge --test scrape_capture` | 4 passed |
| W4 | `quarry-edge --test scrape_max_age` | 15 passed |
| W4 | `quarry-edge --test scrape_max_age_off` | 3 passed |
| W4 | `quarry-edge --test scrape_records_durable` | 0 passed, **4 failed** (Windows only, see below) |

Totals: 340 passed, 4 failed (environmental).

## The 4 failures are not W4 regressions

`scrape_records_durable` gets a 500 `INTERNAL` from `POST /v1/scrape`: `fs write ...\org=...\run=...\page=blake3:<hash>\html.html: os error 267` (invalid directory name). The `FilesystemStore` path contains `blake3:<hash>`, and `:` is not a legal directory character on Windows. It is the test's `FilesystemStore` under a Windows temp dir. It is not E4.4-a. The 4 tests last changed in WS5.F / WS13.4a. They should pass on Linux/macOS/CI. Fix options for the store owner: encode the `:` in the page segment when `cfg(windows)`, or put the digest into the key without `:`. Not done here (out of scope).

## Environment

- First W3 run hit `rustc-LLVM ERROR: no space on device` (concurrent builds). `C:\` had 9.6 GB free, and the sibling dirs were `C:\qv2t` 80.5 GB, `C:\qv2t-w3v2` 19.7 GB, `C:\qv2t-w4v2` 32.3 GB. The rerun alone passed. C: is at about 5.6 GB free after the rerun. Another session cleaning `C:\qv2t` would help. I did not touch it.
- Live Chromium tests (`#[ignore]`) not run. Stream-route refusal for emulated scrape is only covered by a pure helper (from W4-VERIFY2).

## Model Plane status (W2 / W3 rows)

`apps/Model Plane/docs/PLAN.md` has no W2/W3 rows. It is the phase 0-9 plan (last edit 2026-10-31, no wave columns). The W2/W3 rows live in `.plan-work/quarry-w1w2/MP-W2W3-STATUS.md` and the 2026-11-02 entries in `apps/Model Plane/docs/decisions/ledger.md`. Both worktrees contain `930958d73`, the last W3 MP commit, so branch `mp-w2w3` is merged in.

- W2 rows: 40 rows built (U03, U61, F12, WS11.S0-S7, WS16.S1/S8, INNH-10 MP-1..3, WS10.S2/S5/S6/S9, WS7.L, WS12.S4a/b, WS8.G, WS9.E, WS8.J). Tested in process with unit and scratch-Postgres integration tests. Every behaviour is behind a flag that defaults off. Tighten-only guards default on.
- W3 rows: 11 rows built (E3.2-a/c/d/e1/e2, E3.3-f/i/l, E3.6-8, E3.7.10, E3.9.3/4). Compile-checked only, plus the named tests. The user tests after merge.
- Last regression, 2026-11-02 (not rerun today): `execution-core --lib` 1763 pass, 2 fail (one load flake, one real W3 bug fixed in `4eae88bd3`). `model-gateway --lib` 2650 pass, 1 pre-existing fail (`auth::tests::service_inference_bearer_may_hold_a_different_posture_by_audience`, also fails at base `cc8c43389`). `session-core` bin 705 pass. Go modules pass. zdr-contract-sync 140 pass, 10 pre-existing Windows CRLF reds.
- Open: agent-action-admission golden held for the Quarry SPA file (W2-FE-L5a). Cross-plane asks in `MP-ASKS.md`. Depth findings in `mp-w2w3/DEPTH.md`. Compose requires `MODEL_QUARRY_TRACE_NATS_PASSWORD` (set in the main checkout `.env` on 2026-11-01). The `model-nats` container picks up the new user on its next recreate.
- Not verified live: Quarry-side halves (mark route, `warnings[]`, `settle_ms`, `crawl:write`), real JetStream consumers, real Auth Core key tokens.

This session did not rerun any Model Plane tests. The status above is from the status file.
