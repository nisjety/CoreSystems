# W2-U27 status: reload receipt ZDR review and Playwright error-card spec (D2-063, D2-133)

Branch w23/lane-c. Date 2026-11-02. Built (Rust tests run and green; Playwright spec written, type-checked, not run: no stack).

## Built
- Code check first: `agent_routes/receipts.rs` held only the read routes. The reload action itself carries no URL; the page URL on a reload receipt is the post-action `observation.url`. Two facts found: (1) a ZDR run already writes NO receipt (`human_browse::receipt_plan` returns `Skip` when `zdr_active`, in both the HTTP and WS step paths), so the write side was already safe; (2) a non-ZDR receipt is masked at write (`ReceiptBuilder::complete`, Durable URL rules) and again on read (`redact_for_read`). The gap was the read side: nothing reduced a URL for a ZDR reader.
- `receipts.rs`: `receipt_for_read(receipt, zdr_active)` (masks, then under ZDR reduces `observation.url` to scheme://host[:port]/) and `host_only_url` (non-http(s), unparsable or hostless URLs become `[redacted]`; exactly `about:blank` is kept). `list_receipts` now uses it with `zdr_active = claims.zdr_floor() || live run zdr`. Non-ZDR keeps the redacted URL as before. Covers rows written before a run was ZDR or a floor raised since, and any future metadata record.
- `tests/e2e/browser-workspace-omnibox.spec.ts` (D2-133): three new tests in a second describe. Error card (role alert, data-reason, title, the asked URL, no raw error words, Prøv igjen posts navigate {type, url} with actor human, Lukk removes it); reload buttons (soft reload posts a navigate, hard reload posts exactly {type: reload, ignore_cache: true} with actor human, skipped when hard_reload is not advertised); gateway refusals (agent reload 403 browser_action_denied, reload with url 400 invalid_browser_action).

## Commits (w23/lane-c)
See git log: the receipts.rs commit, the spec commit, then this status/depth commit.

## Tests
- Rust (qtest lane c): `cargo test -p quarry-edge --features browser-agent --lib receipt_url_tests` 4 passed, 0 failed (ZDR reload host only, non-ZDR redacted URL, no-observation receipt unchanged, host_only_url table). Regression: `--lib agent_routes` 181 passed, `--test redaction_wire` 8 passed, `--test zdr_feature_matrix` 14 passed.
- Playwright spec: NOT RUN (needs the stack). Type-checked with tsc6 against the main checkout's @playwright/test (strict, 0 errors). The lane worktree has no node_modules.

## Flag and default
No flag (leak fix); the read behaviour changes only for ZDR readers.

## Assumed decisions
- "Host only" = `scheme://host[:port]/` so the field stays a parseable URL for the SPA; userinfo, path, query and fragment dropped.
- Posture on a durable (post-restart) read = the caller's ZDR floor (a run that was not ZDR but is read by a ZDR-floor caller gets the stricter view). A live run adds its own `zdr`.
- Only `observation.url` is reduced. Title and a Navigate action URL are not (see depth).
- The Playwright expectations on the card (4 raw words that must not appear) and the 400/403 codes come from the gateway source (`sanitize_action`, `reload_refused_for_actor`).

## Model Plane asks
None.

## Not verified
- The Playwright spec against a live stack (selectors, the unresolvable host really producing the card, hard_reload advertised by the edge).
- That a ZDR-floor caller reading a non-ZDR run's receipts through the SPA copes with a host-only observation URL.
