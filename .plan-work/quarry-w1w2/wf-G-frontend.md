# G-frontend (round 4 stranded frontend/gateway work) - done

Commits: f3f6a367 (gateway quarry.rs flat 429 hint, hunk-only), d58b7183 (errors.ts copy + knowledge-client invalid_url, hunk-only, WITHOUT WS5.G's unsupported_media line), ab0b8ff6 (U4 tests + BrowserChrome export), 52cfbf6d (U3 gateway browser.rs test).

Left uncommitted on purpose (WS5.G, another session): gateway quarry.rs (formats insert, 415 branch, 4 tests), search.rs (maxPages removal + test), errors.ts `unsupported_media` line, knowledge-client.ts formats/raw_html fields.

Traps:
- The emitted-code test (browser-errors.test.ts) sliced Rust fn bodies with "\n}\n"; on a CRLF copy (git archive of quarry.rs came out CRLF) it swept the rest of the file in. Fixed by normalising CRLF in productionHalf.
- Coupling: the "every emitted code has copy" scan needs WS5.G's unsupported_media copy line in errors.ts to land no later than its gateway 415 code.
- Never run `pnpm` inside an isolated copy whose node_modules is a junction: pnpm tries to purge/reinstall the modules dir (aborted, no TTY, nothing lost). Use `node node_modules/vitest/vitest.mjs`.
- Isolated gateway extract: zdr_log::the_sensitive_field_list_equals_quarrys and tests/sse_relay_no_allowlist fail only because they read sibling planes by relative path.
- Scratch: scratchpad/G-frontend (iso/ extract is ~850 MB, can be deleted).

## Round 4 review fixes (eb5a3f9e)
- HIGH (d58b7183 message wrongly said invalid_idempotency_key had no copy): real, but a message claim only; corrected in eb5a3f9e's message (cannot amend). Replay flags five codes: invalid_url, invalid_crawl_request, invalid_browser_action, zdr_persistent_profile_forbidden, model_gateway_unreachable.
- MEDIUM scan shapes: added "code": "..." and code: "..." patterns; found browser_artifact_unavailable (no copy); gateway now emits browser_artifact_failed (has copy).
- MEDIUM unsupported_media: exempt from both closed-list comparisons; probe confirmed.
- Trap: gateway is a bin crate: cargo test --bin verevon-gateway-rs domains::browser (not --lib). Probes: edit by hand and undo by hand (probe.js/probe2.js in scratchpad/G-frontend).
