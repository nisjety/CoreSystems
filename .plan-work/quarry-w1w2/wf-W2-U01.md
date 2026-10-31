# W2-U01 hand-over (WS10.S1 x-request-id intake)

Built to scope, committed in five commits (see W2-U01-progress.md). Tests written, compile-checked, not run.

## What is left

- Merge-time freeze: a one-day freeze on every route file, announced before the merge (queue U01).
- Stack evidence per the plan: curl through the gateway, `docker logs quarry-edge` carries the id, a 300-char id yields `req_`. Not run.
- Test phase: run `cargo test -p quarry-edge request_id`, `cargo test -p quarry-edge --test request_id`, `cargo test -p quarry-runtime request_context`, `go test ./internal/httpx/...`, the gateway `cargo test request_id browser_proxy_forwards`. Check the `log_field_discipline` lint (no baseline regeneration).
- Gateway sweep: about 20 remaining builders adopt `request_id::apply` in a follow-up (plan).

## Traps found

- `scratchpad/tools/qcommit-blobs.sh` commits a blob built from a HEAD snapshot taken before the commit. It reverted another unit's RUNBOOKS section once (1833ce44), repaired by f27bfab21. Re-read HEAD right before a blob commit, or use qcommit-patch.sh.
- `tests/request_id.rs` relies on `QUARRY_EDGE_AUTH_DEV_BYPASS`, cached once per process.
- The gateway `cargo check` waits on the package-cache lock while other agents run cargo; a 1.5-minute wait is normal.
- Shared-tree warnings "LF will be replaced by CRLF" appear on every touched file; the diffs stay minimal, so they are noise.

## Files outside the owned list (all small, reported in the progress file)

- `quarry-edge/src/queue_routes.rs`, `scrape_routes.rs` (codemod sites)
- `quarry-edge/src/main.rs` (one `mod` line, H3)
- `services/quarry-control/internal/httpx/httpx_test.go` (tests, owned)
