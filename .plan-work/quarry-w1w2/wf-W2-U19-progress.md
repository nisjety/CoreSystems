# W2-U19 progress (WS9.C, devtools filters and gateway evidence proxies)

Status: built to scope, committed in three commits, tests written and compile-checked, NOT run (build-first wave).

## Committed (branch quarryv2-next, local only)

- `35688ef98` feat(quarry-edge): devtools filters and resume cursor. `agent_routes/types.rs` (`DevtoolsQuery` gains levels, filter, category, tabId; `DevtoolsEventsData` gains nextSequence) and `agent_routes/tabs.rs` (`DevtoolsFilter`, `filter_devtools_events`, 4 unit tests in `devtools_filter_tests`).
- `95a2d00b2` docs(quarry-v2): `docs/openapi.yaml` devtools path (additive, 89 lines) and a RUNBOOKS section (commit used the HEAD blob plus this unit's hunks only; the working copy still carries other units' uncommitted hunks in both files).
- `8db594349` feat(gateway): `apps/gateway/src/domains/browser_evidence.rs` (NEW: router with 3 routes, `safe_devtools_query`, projections, owner gate, `dispatch_compile_procedure`, 10 wiremock and pure tests); `domains/browser.rs` (helpers `pub(super)`, `get_devtools_events` uses the allowlist and returns nextSequence, test module `pub(crate)`); `main.rs` (one merge line); `domains/mod.rs` (one line); `domains/actions/handlers.rs` (one arm); `shared/actions/action-registry.ts` (schema and entry `browser_evidence.compile_procedure`, human-only, ingestion plane); `shared/api/browser-client.ts` (types, 4 devtools filter options, `listBrowserReceipts`, `listBrowserEgressReceipts`, `compileBrowserProcedure`).

## Compile checks run (rc 0)

- `cargo check -p quarry-edge --tests` through qtest (quarry-rust-env:2). Exit 0. Only pre-existing warnings in other test files were visible in the tail.
- `cargo check --tests` in `apps/Frontend Plane/verevonv3/apps/gateway` (native host). Exit 0. One pre-existing unused-import warning at `domains/browser.rs` test module (line ~5792), not this unit's.
- TypeScript transpile (syntax only) of `browser-client.ts` and `action-registry.ts`: 0 diagnostics.
- YAML: no parser on the host. Not validated.

## Not built, and why

- SPA typecheck, SPA unit tests and the Playwright spec: not in the plan's named tests for WS9.C; the panels belong to WS9.D.
- Capability `evidence` not advertised in `browser.rs` static list: WS9.D adds it after the stack is deployed (plan: "added only after the backing service is deployed").
- Deployment evidence (curl checks on a live stack): not run (build-first wave).
- `receipts.rs` unchanged: the gateway caps the receipt list at 200; Quarry's receipt stream is unpaginated.

## Decisions taken in the build (for the next batch)

- ZDR refusal for receipts and procedure: 409 with `browser_receipts_unavailable_zdr`, returned before Quarry is asked. Missing `zdr` in the owner projection is treated as ZDR (fail closed); Quarry's projection always sends a bool.
- Quarry 404 for receipts (empty stream after the owner gate) maps to an empty list, not an error.
- Procedure compile Quarry 409 maps to `browser_procedure_not_compilable` (409); other Quarry failures pass through `forward_quarry_failure`.
- Devtools filter validation runs before the run lookup, so a bad filter on an unknown run is a 400.
- The quality check receives the procedure as Quarry compiled it (internal call); only the browser view is redacted.
- Commit attribution: the harness reminder asked for `Co-Authored-By: Claude Haiku 5.5`, the workflow text asked for Sonnet 5.5. The Haiku line was used (system-level instruction).

## Traps for the next unit

- `browser.rs` helpers are now `pub(super)`; `test_app_state` and `mod tests` are `pub(crate)`. Any future `AppState` field is one edit in `test_app_state`.
- The gateway route table (`classed_routes`, H10) was NOT touched; the evidence routes live in their own `router()` with `RateClass::Interactive`.
- `openapi.yaml` and `RUNBOOKS.md` carry other units' uncommitted hunks in the working copy (U21 BudgetMeta in openapi). Commit via `qcommit-blobs.sh` when another hunk is present.
- `DevtoolsQuery` is camelCase on the wire; the egress openapi entry still says `after_sequence` (depth note).

## Depth notes

See `.plan-work/quarry-w1w2/depth/W2-U19.md` (13 bullets).

## Files this unit owns that are not committed

None. The working copy still shows others' uncommitted hunks in `RUNBOOKS.md`, `openapi.yaml`, `browser.rs`-adjacent files of other units; none of them are this unit's.
