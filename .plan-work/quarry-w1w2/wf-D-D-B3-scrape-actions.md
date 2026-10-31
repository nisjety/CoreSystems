# D-D-B3-scrape-actions hand-over (WS13.5 remainder: D2-023, D2-024, D2-025, D2-069)

## Done
- D2-023 `data.actions[]` on the wire: `ScrapeResponse.actions` (quarry-core), filled from the captures' `ActionOutcome`s
  in `PageRunner::persist_captures`, returned by `/v1/scrape` and stored in the scrape record (`GET /v1/scrape/{id}`).
  Absent when empty, so a default response is byte-identical.
- D2-024 action screenshot/PDF bytes: stored under `#cap:action.<i>.<kind>` (own key, request org, one content-free
  `ArtifactWritten` with `action_index`) when not ZDR; inline `data_base64` under the same cap the final captures draw from
  when ZDR; over the cap: output kept without bytes, `omitted: ["actions[i].output"]`, warning `capture_exceeds_inline_cap`.
  `apply_actions` now sets the capture limits and the inline cap for action captures.
- D2-025 safe mode: `scrape_actions::safe_mode_runner` clears ingest, local_index, page_renderer, source_registrar,
  title_enricher in one place; test `safe_mode_clears_every_evidence_hop` (exhaustive `PageRunner` destructure + Some -> None).
- D2-069 gateway: 125 s per-request ceiling for scrapes with actions / object-form / screenshot / pdf formats
  (`proxy_bearer_json_with_timeout`, additive edit in `upstream.rs`), `stamp_scrape_origin` from the verified `CallerClass`.
- Two defects found and fixed: FallbackDriver replaced typed error details (so the 502 `action_failed` body lost its ordered
  outcomes); actions with no browser registered returned 200 without running them (now 501).

## Left (see depth/D-D-B3-scrape-actions.md)
openapi schema addition (D-B3-openapi-sdk), meter metadata, dispatcher stamping (D2-012), page_renderer live proof,
live Chromium tests, a real-token route test.

## Traps
- `scrape_actions::actions_mode()` and `ScrapeSettings::current()` are process-wide `OnceLock`s: a test binary that wants
  actions `on` must set `QUARRY_EDGE__SCRAPE_ACTIONS=on` before its first request (see `tests/scrape_actions.rs`); a binary that
  never sets it runs with `off` (`tests/scrape_capture.rs`).
- `ValidatedActions` is built with `validate_actions(Vec<Value>, on_error, &ActionLimits, &CaptureLimits)`.
- Gateway bin target is `verevon-gateway-rs`: `cargo test --no-fail-fast --bin verevon-gateway-rs scrape`.
