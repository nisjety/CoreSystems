# F2 gateway ZDR log (WS4.E + WS4.F, gateway half) - done

Commits: 2042c391 (zdr_log.rs filter + wiring + golden), 63b64619 (log_field_discipline lint + baseline + two zdr-log-ok comments in gdpr_nats.rs).

Verified in apps/Frontend Plane/verevonv3/apps/gateway: `cargo test zdr_log` 18 passed; full `cargo test` 693 passed + 1 ignored (bin), 1+12+3+1 (integration), 0 failed; lint red proof (temporary url field in src/utils.rs failed the baseline test, removed again).

Left in the tree on purpose (not ours): config.rs, domains/browser.rs, domains/knowledge/quarry.rs, domains/search.rs, and the `browser_grant_mode:` test-state lines in main.rs, chat/shared.rs, orchestration.rs, crawl_preview/stream_e2e.rs (P2-ws1b). I committed main.rs and chat/shared.rs hunk-wise without the browser_grant_mode hunk (git apply --cached), so HEAD compiles without P2's config.rs change.

Traps: the working tree needs P2's config.rs for the tests to compile (the AppState field). Baseline still holds 22 R2 + 1 R4 sites (cache.rs 6, gdpr_nats.rs 6, mail_task.rs 3, handler.rs 3 R2 + 1 R4, browser.rs/mcp.rs/remote.rs/rate_limit.rs 1 each): candidates to rewrite to error_class. `LOG_FIELD_BASELINE_UPDATE=1` regenerates.

## Review round (done)
Commits d8a9708d (filter/mark/caps fix + tests + wiring) and 8af0d48f (lint reads span macros/event!).
- ZdrLogFilter is the one per-layer filter on the JSON layer (caps per event, env filter for events only, spans always pass, ZDR rule for sensitive events and spans). production_subscriber() is what init() installs and what tests build. NEVER put the EnvFilter back in the registry: at RUST_LOG=error the info HTTP span is not created and the mark is void.
- make_http_trace_span marks from the x-zdr header at creation (zdr_header is the pure read).
- Verified: cargo clean -p verevon-gateway-rs then cargo test in apps/gateway: bin 709 passed/1 ignored, integration 1+13+3+1, rc=0. zdr_log tests: 33.
- Trap: the reviewer's scratch builds left test binaries in the gateway target dir with another CARGO_MANIFEST_DIR baked in (tests that read src/ failed with os error 3). cargo clean -p verevon-gateway-rs fixes it.

## Correction of the test count (second review round)
The d8a9708d message says "bin 709 passed". That figure came from the shared working tree, which also holds 5 uncommitted #[test]/#[tokio::test] additions by other units (domains/search.rs 1, domains/knowledge/quarry.rs 4; confirmed with git diff -U0). At the committed tree the bin gives 704 passed, 0 failed, 1 ignored (the reviewer's number). Integration 1+13+3+1 and zdr_log 33 are exact. Commit messages cannot be amended, so this note is the correction. The earlier 693 figure in 2042c391/63b64619 is likewise superseded.
Observation (no defect): spans always pass ZdrLogFilter and there is no max_level_hint, so spans are created at every RUST_LOG level (small per-request allocation); that is intentional, see the NEVER-EnvFilter-in-registry trap above.
