# W2-U61 progress (WS8.C, broker side), 2026-10-24

Status: built to the plan's scope, committed, NOT run. Nothing is deployed.

## Committed (branch quarryv2-next)
- 30bd92a7d feat(browser-broker): proto 50-59 + generated Go, grant kinds human_browse and attach, Renew, policy resolver, server handlers, config/flags, migration 0004, broker tests (written), compose (dev 1, production overlay 0).
- e03e05045 feat(model-gateway): `browser_grants.rs` (kind, renew route, response fields) and the `browser_run.rs` hunk (human_browse parent in `resolve_browser_grant`; R-27 sibling-shared).
- 31d880a83 test(contract): `scripts/tests/browser-grant-contract.test.mjs` (50-53, RenewGrant, I3, defaults, migration idempotency).
- docs commit: Quarry `docs/RUNBOOKS.md` section "Browser broker: human_browse grants, renewal and attach grants (WS8.C ...)" (hunk-only), plus these notes.

## Not committed / not built
- Nothing of mine is left uncommitted.
- Not built: Quarry vendored `browser.proto` 50-53 (WS8.B), gateway keep-alive (WS8.D), execution-core attach consumer (WS8.G), the R-6 ledger entry in `apps/Model Plane/docs/decisions/ledger.md` (sibling-(b) file), any evidence run.

## Compile checks that were run (no test suite was run)
- `go vet ./...`, `go vet -tags integration ./...`, `go vet -tags e2e ./e2e/` in `apps/Model Plane/go/services/browser-broker`: clean.
- `go build ./...` in `apps/Model Plane/go/gen`: clean.
- `cargo check --tests -p model-gateway` through qtest (QROOT=Model Plane, QTARGET=mp-target, `cd rust && CARGO_TARGET_DIR=/src/target`; the image has no protobuf well-known includes, so the command ran `apt-get install -y libprotobuf-dev` first): finished, no errors.
- `node --check scripts/tests/browser-grant-contract.test.mjs`: ok.

## For the next batch / the test phase
- qtest recipe for the Model Plane workspace: mount `apps/Model Plane` (not `rust`: mp-contracts/build.rs reads `../../../proto`), `cd rust`, set `CARGO_TARGET_DIR=/src/target`, install `libprotobuf-dev` in the command.
- Proto regeneration (Go only) was done natively: `buf generate --template <scratch yaml with local protoc-gen-go and protoc-gen-go-grpc> --path model_plane/v1/browser.proto` from `apps/Model Plane/proto`, copied with CRLF. Re-run the same way if browser.proto changes.
- Migration number is 0004 (R-3); 0002 is still free for WS7.K. The Go test helper `newTestDB` applies 0001, 0003 and 0004; add 0002 when WS7.K lands.
- The Repository interface gained `CreateHumanBrowse` and `Renew`; any new fake must implement both (router_test.go's recordingRepo does).
- `PolicyResolver` gained `ResolveHumanBrowse`; the `denyAll` fake in derive_test.go has it.
- Details of what was deferred: `.plan-work/quarry-w1w2/depth/W2-U61.md`; hand-over: `.plan-work/quarry-w1w2/wf-W2-U61.md`.
