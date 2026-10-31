# W2-U01 progress (WS10.S1 x-request-id intake), built 2026-10-23, NOT run

## Committed (branch quarryv2-next, local only)

| Commit | What |
|---|---|
| d762e8fcb | quarry-control `httpx.RequestID` validates the inbound id (1 to 128 bytes, `[A-Za-z0-9._:-]`), mints `req_<uuid>` otherwise; `httpx_test.go` gains `TestRequestIDRejectsOverlongAndForeignCharset`, `TestRequestIDAdoptsValid`. |
| e18328f50 | quarry-runtime `request_context.rs` (task-local, `scope`, `current_opt`); quarry-edge `request_id.rs` (layer, `RequestId`, `current()`, `make_request_span`, flag `QUARRY_EDGE__REQUEST_ID_ADOPT`); router wiring in `routes.rs`; `main.rs` and `lib.rs` one-line mods; codemod of 69 call sites in 16 files; `tests/request_id.rs`. |
| 1833ce445 | CONTRACTS.md request-id subsection (before section 3) and a RUNBOOKS section. Its RUNBOOKS blob dropped the WS12.S6 section (stale HEAD, see the depth file). |
| f27bfab21 | fix-up: restores the WS12.S6 section, so RUNBOOKS = b0c029f0a plus this unit's section. |
| 42a9a9e53 | gateway `request_id.rs` (layer, `apply`, `scope_for_test`), `main.rs` (mod, layer, span field), `config.rs` (CORS expose), `upstream.rs`, `domains/browser.rs`, `domains/chat/shared.rs` (apply at each builder), `Cargo.toml` (`rand = "0.8"`), `Cargo.lock`. |

## Verification (run)

- `cargo check -p quarry-runtime -p quarry-edge --tests` in quarry-rust-env: rc=0 (after the codemod, after the main.rs mod, and after the last import tidy; see check3 below).
- `gofmt` and `go vet ./internal/httpx/` in services/quarry-control: rc=0.
- `cargo check --tests` in the gateway crate (native host): rc=0; one pre-existing unused-import warning at browser.rs (model_plane_run_read_model), not in this change.
- rustfmt --check on the three new Rust files: rc=0 after formatting them.

## Not run (written, compile-checked only)

- quarry-edge unit tests in `request_id.rs`, the quarry-runtime `request_context` tests, `tests/request_id.rs` (3 tests).
- Gateway `request_id::tests` (6 tests) and `domains/browser.rs::browser_proxy_forwards_request_id_to_quarry`.
- Go `TestRequestIDRejectsOverlongAndForeignCharset`, `TestRequestIDAdoptsValid`.
- The `log_field_discipline` lint: not run, baseline NOT regenerated.

## Deviations from the plan (for the arbiter)

- The Quarry layer is the outermost layer of the router (after the timeout and body-limit), not directly after `TraceLayer`. Reason: 408 and 413 answers echo the header too.
- The codemod covers 69 sites. The plan counts 65; `scrape_routes.rs` (1) and `queue_routes.rs` (3, `Id::<RequestKind>::new()`) are outside the queue's owned list but were converted for consistency. Both edits are mechanical.
- The gateway builders adopt the id in 3 upstream, 3 browser, 1 chat builder. The other ~20 builders are the plan's follow-up sweep.
- Gateway minting uses `rand` 0.8 (already in Cargo.lock), not a new ULID crate.

## Not done

- One-day route-file freeze announcement before the merge (process).
- Stack evidence (curl through the gateway, `docker logs quarry-edge`), per the plan's Evidence paragraph.
- The `log_field_discipline` baseline (not regenerated, by rule).

## Hand-over for the next batch

- U25 (WS10.S3) and later units that touch `routes.rs` layers, `telemetry.rs`, `zdr_log.rs` must read the request-id layer position above.
- `request_id::current()` is the only id source for handlers now; a new handler must use it, not `RequestKind::new()`.
- The RUNBOOKS.md working-tree copy has this unit's section above the WS12.S6 section; HEAD has it last. Resync before the next RUNBOOKS edit.
- Trap: `qcommit-blobs.sh` can revert a unit's section when HEAD moved after its snapshot. Use `qcommit-patch.sh` or re-read HEAD right before the blob commit.
