# W5 verification (Quarry, branch quarryv2-next)

Date 2026-11-07. Worktree `.claude/worktrees/vigorous-perlman-e7ae5b`, HEAD `cc8c43389`. Local only, nothing pushed, no writer file edited.

Scope note: no document in the tree defines "Wave 5" (STATUS-2026-10-24.md lists "W4, W5: not started, on hold"). I read it as the honest-identity slice: WS3.E (stealth code deleted, `2e62ad5ae`) and WS3.I (`QUARRY_BLOCK_ESCALATION`, default flip to `honest`, `89097598a`, plus its review fixes). Checks cover the invariants in the request, not a plan-defined W5 checklist.

## Ran vs read

| Item | How |
|---|---|
| Rust tests | RAN, native Windows `cargo test`, separate `CARGO_TARGET_DIR` (temp), not the Docker `qtest` path |
| Go test | RAN, `go test ./internal/resources/...` in `services/quarry-control` |
| Invariant greps | RAN (`git grep` / grep over the tree) |
| Evaluate denial, flag defaults, honest matrix | READ (source) plus RAN (the tests named below) |
| Live stack, browser, 403-origin measurement | NOT RUN |

## Invariants

| Invariant | Result | Evidence |
|---|---|---|
| `stealth.rs` deleted | HOLDS | `git ls-files` has no path containing `stealth`; deleted by `2e62ad5ae` (WS3.E) |
| No `FingerprintRotator` | HOLDS | Only 2 text hits, both prose: a comment in `fingerprint_rotation.rs` saying it was removed, and a historical line in `docs/PARITY_IMPLEMENTATION_PLAN.md`. No type, no caller. `fingerprint_rotation.rs` now holds only `is_block_status` (classification, no rotation) |
| Evaluate denied | HOLDS | Edge: `agent_routes/admission.rs::deny_raw_script` runs before approval, budget and receipt (403, `raw_script_denied`). Runtime: `action_runtime.rs` refuses `Action::Evaluate` even in dry-run. Scrape actions: `evaluate` is in the refused list (`scrape_actions.rs`). Tests: `agent_evaluate_denied` 6/6 pass (step, interact and WebSocket; user and service tokens; no receipt, no budget step, no script in logs), `web_policy_agent` 7/7 |
| `stealth` grep | EMPTY of mechanism | Code hits are all refusals or tests: reserved request key `"stealth"` refused in `scrape.rs`, `crawl_request.rs`, `scrape_contract.rs`, `routes.rs:3055`; a typo-fails-closed test string in `escalation.rs`; a comment in `quarry-tls`; the Go deprecated preset alias `stealth -> careful` (`presets.go`, intentionally kept one release) and its tests, which also assert no preset text says stealth/anti-bot/evade/undetect |
| captcha-solve grep | EMPTY | Only docs: competitor/gap descriptions and "No CAPTCHA solving as a silent default" (`SOVEREIGN_SCRAPING_RELIABILITY.md`). No solver vendor names or solve code in `crates/` or `services/` |
| consent-accept grep | EMPTY of action | Hits are detection only: `challenge.rs` `CONSENT_PHRASES`/`CMP_MARKERS` classify a page as `consent_required`; the other hits are test fixtures. `ChallengeKind::Consent` is only produced, never acted on by a click |
| Other spoofing patterns | NONE | No `navigator.*` / `webdriver` overrides, no UA pool or rotation code. `addScriptToEvaluateOnNewDocument` is used only for profile storage flush/restore |

## Honest-mode matrix (read and tested)

`escalation.rs`: under `honest` only `TransportError` may change identity; block status, strong challenge and caller `prior_block_signals` may not. Wired in `fetch.rs`, `fallback_driver.rs`, `egress_broker.rs` (`!is_honest()` gate), `driver_plan.rs`. Unknown env values fail closed to `honest`.

## Tests run

| Target | Pass | Fail | Ignored |
|---|---|---|---|
| quarry-runtime (all 31 test binaries, `--no-fail-fast`) | 1201 | 9 | 1 |
| quarry-core (all) | 379 | 0 | 1 |
| quarry-browser (all) | 225 | 0 | 0 |
| quarry-edge, targeted: lib, `agent_evaluate_denied`, `cache_policy`, `web_policy_agent` | 876 | 0 | 0 |
| Go quarry-control `internal/resources` | ok (rc 0) | 0 | - |
| Rust total | 2681 | 9 | 2 |

The 9 failures are all `quarry-runtime` `artifact_store::tests::fs_store_*`. Cause: Windows rejects the `:` in the directory name `page=blake3:deadbeef` (`os error 267`, invalid directory name). The file is not in any WS3.E/WS3.I commit. It is a platform failure of this native run, not an honesty regression; I did not run it under the Linux image to confirm it passes there.

Escalation tests that passed: `matrix_honest_allows_only_transport_error`, `matrix_legacy_allows_every_trigger`, `unset_is_build_default_and_unknown_is_honest`, `build_default_matches_the_documented_default`, `the_env_var_reaches_current_and_legacy_is_the_rollback`, plus the `fetch`, `fallback_driver`, `egress_broker`, `driver_plan` honest/legacy tests (all in the 1201).

Not run: full `quarry-edge` integration suite (about 100 test binaries). The first attempt hit `os error 1455` (Windows page file too small) when linking them all, so I reran the four relevant targets with `-j 2`. Also not run: `model-gateway` (its WS3.I fix `8566f2c12` is in Model Plane), the Go `quarry-control` packages other than `resources`, live/stack tests.

## Flag audit (read of source and compose files, nothing executed)

| Flag | Default | Source |
|---|---|---|
| `QUARRY_BLOCK_ESCALATION` | `honest` (`BUILD_DEFAULT`); `legacy` is the rollback; blank or typo gives `honest` | `escalation.rs`, RUNBOOKS "Block escalation" |
| `QUARRY_EDGE__SCRAPE_ACTIONS` | `off` (anything but `on`/`user` is off); compose `off` | `scrape_actions.rs`, both compose files |
| `QUARRY_EDGE__API_KEY_AGENT_RUNS` | off (`unwrap_or(false)`; bad value is a boot error) | `run_limits.rs` |
| `QUARRY_EDGE__PARSE_ROUTE` | off (501 naming the variable) | `parse_routes.rs` |
| `QUARRY_EDGE__CRAWL_ROBOTS` | `observe` in the Ingestion compose | `docker-compose.yml:46` |
| `QUARRY_PROXY_POOL` | empty | `docker-compose.yml:309` |

No flag that enables stealth, captcha solving or consent accepting exists. The docs (RUNBOOKS, ARCHITECTURE, POLICY, openapi) state `honest` as the default and `prior_block_signals` as deprecated.

## ADR-5 gate status

- ADR-5 approved at its default (W1-work-breakdown, W1-closeout-baseline). Mechanism in tree; default flipped to `honest` in its own commit `89097598a`.
- Gate test `build_default_matches_the_documented_default`: PASS (ran).
- Open item: the plan asked for a before/after measurement of pages a TLS or browser tier used to rescue before the flip. `escalation.rs` says none was possible, so the flip rests on tests, not measurement. RUNBOOKS also says no rebuilt edge was run against a public 403 origin with a two-entry proxy pool. Still open and not something I could do here.
- Verdict: invariants hold and the gate is green. The measurement caveat is the one soft spot.

## Worktree state (not mine)

- `crates/quarry-runtime/src/step_receipts.rs` has an uncommitted change: 1 real line plus CRLF churn (1954 lines in the raw diff). Another writer's file; I did not touch it.
- Untracked: `rb.patch`, `.plan-work/next-slices.md`, and many `sdks/typescript/src/**` generated files.
- Logs: `C:\Users\ImaFernandesDaCosta\AppData\Local\Temp\opencode\w5verify\` (`runtime.log`, `core.log`, `browser.log`, `edge2.log`, `go.log`).
