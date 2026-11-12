# Control PR train (Space plan slice `P-CFG`)

Source: `docs/plans/space-hybrid-build-plan-2026-10-14.md` sections 4.4.1 (hotspot row 19), 4.4.2 (`P-CFG`),
4.5.3 (R10), and cross-session row X29 / owner decision O16.

Three build sessions (Space, Model Plane chat, Quarry) all need new Control principals, scopes and NATS ACL
rows. Those live in a handful of shared files that every lane would otherwise edit by hand and conflict on.
**Control principals, `nats.conf` and compose credential edits go only through this train.** A lane never
edits these files in its own feature PR; it files a *scope request* (below) and the integrator batches it.

## Files the train owns

| File | What changes |
| --- | --- |
| `apps/Control Plane/config/plane-service-principals.json` | principal rows: audiences, scopes per audience, retention, org binding, `consumerEnv` |
| `apps/Control Plane/docker-compose.yml` | the credential JSON blocks (`USER_CORE_SERVICE_CREDENTIALS` and siblings) and one `${VAR:-default}` line per flag |
| `apps/Model Plane/deploy/nats.conf` | per-user publish/subscribe ACL rows (for example `mp.v1.space.*.event` for `session-core-runtime`) |
| `apps/Control Plane/auth-core/src/auth/fixtures/cross-plane-token-requests.json` (+ `.sha256`) | vendored copy of Quarry's token-request manifest; refresh with `scripts/control-train-repin.sh` |
| `apps/Control Plane/scripts/run-control-plane.sh`, `.env.example` files | only when a new `consumerEnv` variable is introduced (never edit a live `.env*`) |

## Rules

1. **One weekly batch.** The integrator opens one train PR per week (target branch `space`, then `dev`). It
   carries every scope request received that week. Six batches are planned over milestones M1 to M3 (table below).
2. **Rows added alphabetically, never reordered.** A new principal row is inserted in key order in
   `plane-service-principals.json`; a new scope is inserted in sorted position in its scope array; an existing row
   is never reordered or reformatted. A diff that moves lines is rejected.
3. **Each scope request names itself** in its slice PR text (template below). The train PR text lists every request
   it contains. A scope that nobody requested is not added.
4. **Least authority.** One principal, one audience, named scopes, no wildcard, no `allowAnyOrg`, no new
   `tokenExchange`. A new scope is held by exactly one principal on exactly one audience unless the request says why.
   Retention stays as in `config/README.md` ("Retention markers are a data-retention decision"); `quarry` stays `zdr`.
5. **Flags**: one `${VAR:-default}` line per flag, default off in production; a flag is never an authorization input.
6. **Secrets**: the train PR contains policy and ACL only. Never a credential, token or env value; `consumerEnv` names
   the variable, the launcher mints the value.
7. **Handlers assert their own scope.** An unmapped POST falls through to `users:write:self` in user-core, so a slice
   adding a route also adds its scope row to `scopes_spaces.go` and the handler asserts the scope itself.

## What the train PR must run (acceptance)

From the repo root:

```bash
bash scripts/tests/principal-registry-parity.test.sh          # policy file, launcher and bootstrap agree
bash scripts/tests/zdr-contract-sync.test.sh                  # goldens and vendored copies pinned
bash "apps/Control Plane/scripts/control-train-repin.sh"      # token-request manifest pins (--apply to fix)
cd "apps/Control Plane/auth-core" && pnpm exec jest plane-service-principals-policy.contract
cd "apps/Control Plane/user-core" && go test ./internal/http/...
```

**Golden re-pin**: when the batch changes a lane Quarry presents, edit the row in Quarry's manifest (the original),
then run `control-train-repin.sh --apply`; it re-vendors Auth Core's copy and rewrites the three pins (Quarry's
`.sha256`, Auth Core's `.sha256`, the `GOLDENS` row in `zdr-contract-sync.test.sh`). Other goldens are never
hand-edited: regenerate them with their owner's generator, then update their pin row.

## Deploy order (after merge)

1. `scripts/run-control-plane.sh config` once, discarding its output (it prints resolved secrets); the launcher mints
   credentials for new `consumerEnv` names and never rotates an existing one.
2. Recreate `auth-service` (the registry is read at start) and the services whose compose block changed. Use
   `scripts/run-control-plane.sh`; a bare `docker compose build` exits 0 and builds nothing.
3. NATS ACL changes: recreate the NATS container, then the provisioner (`apps/Model Plane/go/services/nats-provisioner`)
   when a stream or durable changed. Model Plane images built from a worktree need the live-env recreate recipe.
4. Verify the new lane with a real token mint, not only a green config check.

## Scope request template (paste into the slice PR text)

```text
Control train request
- slice:        WSnn-Sk
- file(s):      plane-service-principals.json | docker-compose.yml (block) | nats.conf | manifest
- principal:    <name> (new | existing)
- audience:     <audience>
- scopes:       <alphabetical list>
- retention:    zdr | persistent | tenant (and why)
- consumerEnv:  <variable the presenting end reads> | none
- nats ACL:     <user: publish/subscribe subjects> | none
- flag:         ${VAR:-default} | none
- needed by:    <batch number>, deploy order step
```

## Batch ledger

Assignments are derived from the plan's slice tables (rows quoted by slice id) and are provisional: the integrator
moves a request between batches, never within-file order. Mark a batch done by editing its Status cell only.

| Batch | Plan train | Requests (slice: change) | Status |
| --- | --- | --- | --- |
| 1 | T3 (M1 close) | WS03-S3 `spaces:policy:admin:*` on `verevon-gateway`; WS04-S3 `directory:read:self` on `verevon-gateway`; WS04-S4 `spaces:membership:act` on `verevon-gateway` and `spaces:membership:read` on `application-space-lifecycle`; WS02-S3 compose credential edit | open |
| 2 | T3 (M1 close) | WS02-S7 `mp.v1.space.*.event` publish for `session-core-runtime` in `nats.conf`; any M1 request filed after batch 1 closed | open |
| 3 | T4 (M2.1) | WS08-S1 `spaces:issue` (user-core `service_auth.go` scope for the new path); WS09-S7 scopes; WS10-S2 `spaces:tool-gate:view`, `spaces:approval-grant:use`, `spaces:approval-continuation:check` and the principals JSON row | open |
| 4 | T5 (M2.2) | WS11-S5 stream `MODEL_PLANE_SPACE_EVENTS` and durable `convex-space-attention` grants for `session-core-runtime` and `application-convex-model`; WS11-S5/S9 notification-core `space-attention` principal | open |
| 5 | T6/T7 (M2.3 to M3.1) | WS14-S2 service-credential scopes and gRPC writer audit; WS17-S4 entitlement scopes; WS18-S2 `capability:owner-action:health:write` and probe scope (execution-core blocks); WS18-S4 `SPACE_AGENT_ADMISSION` | open |
| 6 | T8 (M3.2) | WS20-S2 and WS20-S6 `browser:view` row (model-gateway block on `quarry`) | open |
| 7 | none (filed after batch 6) | W3-EP-9-W3-a: `org:browser-site:read:any` on audience `org-core` for `execution-core` and `model-gateway` (compose credentials) | open |
| 8 | none (filed after batch 6) | P0-D2-481: `session:memory-controls:read` on audience `session-core` for `orchestrator-core` | open |

The WS-level scope names above come from each workstream's file-level table; the authoritative text for a request
is its own slice row. When a slice builds, it adds its request to this table in the same PR (append a row cell,
do not reorder).
