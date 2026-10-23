# A3 plan errata text (unit W1-A3-grant-leftovers, 2026-10-23)

The plan (`docs/plans/quarry-browser-build-plan-2026-10-14.md`) is NOT edited by this unit. Each block below is text for the single plan writer to apply. Line numbers are those of the plan at 2026-10-23; re-grep before use.

## E1. WS1.B status row (line 27, "Not done" cell)

Remove "`BROWSER_BROKER_MAX_RUN_DOMAINS` and `BROWSER_GRANT_ENFORCEMENT` are not set in compose". Replace with:

> `BROWSER_BROKER_MAX_RUN_DOMAINS` (`${...:-8}`, `apps/Model Plane/deploy/docker-compose.yml`, broker block) and `BROWSER_GRANT_ENFORCEMENT` (`${...:-}`, empty on purpose, `apps/Frontend Plane/verevonv3/docker-compose.yml`, gateway block) are set in compose since `3b13119e`; the L1 test pins both. The production overlay's explicit `BROWSER_GRANT_ENFORCEMENT: strict` is NOT added: it waits for the green L2 run on the production overlay.

## E2. WS1.B slice, the two test names that do not exist

The plan names `proxy_with_delegations_sets_x_browser_authorization_only_when_present` and `required_browser_token`. The code does it differently: `proxy_model_json_with_delegated` in `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/browser.rs` (about 4129; the header is set at about 1631, 4303 and 4355; tests at about 6556, 6808, 6829). Replace the two names with those. If the owner wants an L1 assertion kept, it is: the gateway sets `x-browser-authorization` only through `proxy_model_json_with_delegated` and `browser.rs` contains no `format!("session:` literal (not built; depth bullet).

## E3. WS1.A1 evidence text ("one AcquireGrant" in the broker log)

Add after the sentence that reads the broker log:

> The broker writes one `browser grant issued` line per `AcquireGrant` and one `browser grant revoked` line per `RevokeGrant` (`internal/server/grantlog.go`, `d7a04e07`): kind, zdr, host count, and the grant id for a non-ZDR grant. It never logs a host, scope URL, organization or owner, and leaves a ZDR grant's id out. `ValidateGrant` and `DeriveRunGrant` write no line (one per browser action would be noise; a derive is visible through the run's own events).

## E4. WS1.A1 definition of done, "ledger entry merged"

Add: the entry is in `apps/Model Plane/docs/decisions/ledger.md` (`02cbd74c`, "Browser broker grants for Quarry (WS1.A1) ..."). D5 is recorded as "no broker contract, no `browser.proto` field": the plan's R-6 lists none for it, and no D5 design document is in the repo to confirm it.

## E5. WS1.C evidence text, restart semantics

Add: the restart-semantics table and the side-by-side recipe are in the ledger (`d6b273e6`, "Browser broker restart semantics (WS1.C)"). L1 assertion 7 (no `zdr` column, `browser_grants_no_zdr_id_check` kept, no later migration drops it or adds a zdr column) is `the grants migration has no zdr column and keeps the z_ id CHECK` in `scripts/tests/browser-grant-contract.test.mjs`. The `depends_on` item is dropped: the database lives in the Ingestion compose project (`apps/Model Plane/deploy/docker-compose.yml`, the note above `DATABASE_URL`).

## E6. Plan line 3696 (config table), broker row

No change in values. Add the gateway row's compose spelling: `BROWSER_GRANT_ENFORCEMENT` is wired in the Frontend compose with an empty default; an explicit `permissive` or `strict` in the BASE file is refused by L1 because the production overlay inherits the base.

## E7. L1 test count

The L1 contract file `scripts/tests/browser-grant-contract.test.mjs` has 27 tests at `3b13119e` (25 before). Plan text quoting "15 tests" (the WS1.B row, at `be85fa94`) is a point-in-time count, not the current one.
