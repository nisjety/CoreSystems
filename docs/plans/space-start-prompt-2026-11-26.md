# Space system: start prompt and wiring map

Written 2026-11-26 for whoever continues the Space work, on any machine. Paths are relative to the repository root. Branch: `space-w3`, pushed to `origin` (`https://github.com/nisjety/CoreSystems`).

## Paste this to start a session

> Continue the Verevon Space work on branch `space-w3`. Read `docs/plans/space-start-prompt-2026-11-26.md` first, then `.plan-work/space-build/w123-gap-ledger.md` and `.plan-work/space-build/w3-decisions.md`. Check each open item against git before you build it. Work in a worktree of `space-w3`, run one builder per lock group at a time, and stop at a wave boundary for owner review. Do not merge to `main`.

## 1. Where to start

1. Get the branch:

```powershell
git fetch origin
```

```powershell
git switch space-w3
```

If the branch is already checked out in a worktree, use that worktree instead.

2. Confirm the state:

```powershell
git status --short
```

```powershell
git log --oneline -8
```

Expect a clean tree. The top commits should match section 2.

3. Read, in this order:
- `01-verevon-space-audit.md`, `02-quarry-and-browser-audit.md`, `03-model-plane-chat-and-agents-audit.md` (repository root): the audits the Space work follows. They are also on `main`.
- `.plan-work/audit.md` and `.plan-work/audit-sections/`: the audit the W1–3 gap ledger was built from. Section 3.A–3.E is the Space scope; 3.Z is out of scope.
- `docs/SPACE_AUTHORITY_ADR_2026-08-13.md`: who decides what inside a Space.
- `docs/CROSS_SPACE_AGENT_REGISTRY_ADR_2026-08-19.md`: the agent registry.
- `docs/plans/space-hybrid-build-plan-2026-10-14.md`, `docs/plans/space-w1-report.md`, `docs/plans/space-w2-build-ledger.md`, `docs/plans/space-depth-register-w1.md`: the build plan and status by wave.
- `apps/Model Plane/docs/space-event-stream.md`: the event stream.
- `.plan-work/space-build/w123-gap-ledger.md` (machine copy `w123-gap-ledger.json`): the open-item ledger.
- `.plan-work/space-build/w3-decisions.md`: owner decisions. The latest recorded are D27–D30 (commit `553a3782f`).
- `.plan-work/space-build/depth/`: one note per depth finding.

## 2. Where the branch stands

- `space-w3` holds the Space W1–3 build. It is 464 commits ahead of `main` and 68 behind, and it is not merged. Owner order (2026-11-19): merge only after the Space audit waves 1–3 are complete.
- Commits added on 2026-11-26, newest first:
  - `7730b3528` fix(space): WS06-S4 classify DELETE /api/v1/spaces/status. The status clear route had no row in the surface registry, so `every_space_route_has_a_classified_row` failed. The row is `Config` / `n/a` / `path-rule`, the same class as the reminders cancel row. The golden is re-vendored to the gateway and model-gateway fixtures, and the digest in `scripts/tests/zdr-contract-sync.test.sh` is updated.
  - `a8db13bfd` feat(session-core): WS20-S3 owner summary read of one room browser run, `SpaceBrowserRead.SummarizeRoomBrowserRun`, with regenerated Go and Python bindings. Session-core unit tests and mp-contracts pass. The two SQL queries have never run against a database, and nothing calls the method yet.
  - `ff0658ae9` docs(space): build notes and unit records from the interrupted W1–3 run.
  - `cd5283e9c` feat(gateway): DEPTH-3D-RATELIMIT per-user ceilings on DM open, revise, reactions, pins, search and uploads. Per user, per organization, per minute: DM open 20, uploads 20, edits and deletes 30, pins 30, search 30, reactions 60. These are the builder's numbers, not the plan's. The owner must confirm them.
- Build run `wf_d9077a1b-86c` (245 items) stopped at the weekly usage limit (resets 30 Nov, 02:00 Oslo time). Result: 10 done, 22 partial, 5 blocked, 3 error, 205 build-failed. The 10 "done" items are the builders' own reports and have not been re-checked: F-t1-041, DEPTH-ZDR-REQUEST-HEADER, DEPTH-WS04-S8-LISTING-FEATURE, WS07-S1, B-REC-05, B-REC-03, WS11-S7, WS13-S6, WS13-S7, WS04-S8.
- Gap ledger (last refreshed 2026-11-24): 322 open (178 partial, 144 missing), 55 of them need an owner decision. It does not include the 2026-11-25 run.

## 3. How the Space system is wired

### 3.1 Request path

```
SPA       apps/Frontend Plane/verevonv3/src/features/spaces/        (~110 component files, ~67 lib files)
          src/shared/api/spaces-client/   src/shared/actions/domains/spaces/
   |  HTTP and SSE on /api/v1/spaces/...
   v
Gateway   apps/Frontend Plane/verevonv3/apps/gateway/src/domains/spaces/   (~30 modules, ~160 /api/v1/spaces paths)
   |
   |-- user_core_url -------------> Control: user-core (Go)
   |                                 internal/spaces/*.go (decisions), internal/http/routes_space_*.go
   |                                 e.g. /api/v1/internal/spaces/thread-decision, .../retrieval-decision
   |-- application_convex_url ----> Application: convex-core (convex/space*.ts) via convex-gateway
   |                                 needs APPLICATION_CONVEX_URL and APPLICATION_CONVEX_SERVICE_KEY
   |-- model_gateway_url ---------> Model Plane: model-gateway (Rust)
   |                                 src/space_routes/*.rs (/v1/spaces/...), src/space_sse.rs (event relay)
   |                                   |  gRPC, SESSION_CORE_URL
   |                                   v
   |                                 session-core (Rust), src/space/*.rs
   |                                   gRPC: SpaceAttention, SpaceBrowserRead, SpaceEvents,
   |                                         SpaceHistoryService, SpaceRoomReadService, SpaceRunActions
   |                                   |-- execution-core: space_gate.rs, space_directory_tool.rs,
   |                                   |     space_canvas_tools.rs, space_self_api.rs, browser_events.rs
   |                                   |-- capability-core (Go): capability decisions (migration 0048)
   |                                   |-- quarry-edge (Ingestion Plane): QUARRY_EDGE_URL, browser evidence
   |-- imports_api_url -----------> Ingestion: imports-core, app/space_upload.py (room file uploads)
   |-- org_core_url --------------> Control: org-core (31 references in the Space code)

Data Plane v2: wiki-store-go, internal/canvas/*, internal/handler/canvas*.go (the room canvas)
```

### 3.2 Where each piece is defined

| Piece | Location | Notes |
|---|---|---|
| Surface registry (one row per route, with its ZDR class) | `apps/Model Plane/api/contracts/space-surface.v1.json`, vendored to the gateway and model-gateway `tests/fixtures/` | Checked by gateway `tests/space_routes_zdr_classes.rs`, model-gateway `zdr_posture`, and `scripts/tests/zdr-contract-sync.test.sh` |
| Protos | `apps/Model Plane/proto/model_plane/v1/space_attention.proto`, `space_events.proto`, `space_history.proto`, `space_room.proto`, `space_run.proto` | Rust bindings from `mp-contracts/build.rs`; Go in `apps/Model Plane/go/gen`; Python in `apps/Model Plane/python/mp-events-py` |
| SPA action contracts | `apps/Frontend Plane/verevonv3/src/shared/actions/domains/spaces/*.ts` | Add or update the contract before wiring a UI action (CLAUDE.md) |
| Flags | Gateway `src/config.rs` reads about 62 distinct `SPACE_*` names. The Control flag registry, `apps/Control Plane/flag-registry/flag-registry.json`, lists 14 of them. | Set in `apps/Frontend Plane/verevonv3/docker-compose.yml` and `docker-compose.production.yml`. Several default off in production, for example `SPACE_BROWSER_CARD`. |
| Authority decisions | `apps/Control Plane/user-core/internal/spaces/`, scopes in `internal/http/scopes_space_*.go`, migrations `017_space_authority` to `050_space_member_history_window` | Control owns membership, roles, policy, DMs and history windows |
| Session state, runs, events | `apps/Model Plane/rust/services/session-core/src/space/`, migrations from `0080` (for example `0135_space_attention_outbox_fixes`, `0139_space_membership_fact_role`) | DB-gated tests are `#[ignore]` and need `DATABASE_URL` pointing at a disposable Postgres whose name contains `test` |
| Realtime projections and notifications | `apps/Application Plane/convex-core/convex/space*.ts`, `convex/schema/space*.ts`; notification-core migrations `015_register_space_attention_notifications` and `019_register_space_dm_notification` | Application owns collaborative projections and notifications |
| Room canvas | `apps/Data Plane v2/services/wiki-store-go/internal/canvas/`, `internal/handler/canvas*.go` | |
| Room files | `apps/Ingestion Plane/imports-core/app/space_upload.py` | |
| Compose stacks | Model: `apps/Model Plane/deploy/docker-compose.yml` (model-gateway, session-core, execution-core, capability-core, sandbox-manager, browser-broker, orchestrator-core, inference-core, cost-core). Control: `apps/Control Plane/docker-compose.yml` (user-core, org-core, auth-core, audit-core). Application: `apps/Application Plane/convex-core/docker-compose.yml` (convex-backend, convex-gateway). Frontend: `apps/Frontend Plane/verevonv3/docker-compose.yml` (gateway, frontend, nats). Ingestion: `apps/Ingestion Plane/docker-compose.yml` (quarry-edge, imports-api, and others). | |
| Decisions and rules | `docs/SPACE_AUTHORITY_ADR_2026-08-13.md`, `docs/CROSS_SPACE_AGENT_REGISTRY_ADR_2026-08-19.md`, `apps/Model Plane/docs/space-event-stream.md`, `apps/Model Plane/docs/runbooks/space-self-api-rollout.md` | |

### 3.3 Wiring to check first

- **Two services named `session-core`.** The Control Plane compose has a `session-core` service (container `session-core-service`, NATS-backed). It is not the Model Plane Session Core. Check which one a name resolves to before you assume.
- **Imports service name.** The gateway defaults `IMPORTS_API_URL` to `http://imports-core:3025`, but the Ingestion compose names the service `imports-api`. Confirm which name the running stack uses.
- **Live or dark.** Several Space routes default off in production. Code being present does not mean the route is live.
- **Convex.** Space projections live in Convex. The gateway needs both `APPLICATION_CONVEX_URL` and `APPLICATION_CONVEX_SERVICE_KEY` (`config.rs`, around lines 955–956).

## 4. Open work, in order

1. **Refresh the ledger for the 2026-11-25 run.** Match each result to its ledger `key`; ids are not unique. Check each "done" item against git (its commit exists) and against its tests before marking it done. Commit the ledger update by itself.
2. **Finish WS20-S3 (partial).** Done: the session-core method (`a8db13bfd`). Open:
   - a caller of `SummarizeRoomBrowserRun` that builds the room result turn;
   - steps, hosts, evidence ids, duration and handoffs in the browser run metadata of that turn;
   - making `browser_run` visible in the room (room metadata allowlist or a typed entry, WS20-S5);
   - the org budget ceiling on room grants;
   - binding the gateway-created thread into the new-thread decision.

   Quarry delegation stays in WS20-S6. First confirm which file composes the result turn: the gateway keeps browser-run metadata in `domains/browser.rs`, and an older builder note cites `browser.rs:532-545` without a path.
3. **DEPTH-3D-RATELIMIT (committed in `cd5283e9c`).** The owner confirms the per-minute numbers. Still missing: a route-level test that a refused request makes no upstream call. The current tests cover the limiter only.
4. **Run the DB-gated tests** for `space::browser_read` (`db_tests`) against a disposable Postgres once Docker is running. That is the first real run of the `SummarizeRoomBrowserRun` SQL.
5. **Owner decisions.** The 55 ledger items that need an owner decision go to the owner as one list, with the options and the plan's recommendation. Do not pick for the owner. D27–D30 are already decided.
6. **Second build pass.** After step 1, build the items unblocked by D27–D30 (conditional slices, the first slices of the W3-REC epics, the SSO and SCIM items), plus partial items that can still be built.
7. **Merge.** Merge `space-w3` into `main` only when the owner says the W1–3 waves are complete. `main` has 68 commits the branch lacks, so bring `main` into the branch first.

## 5. Rules that apply

- Planes own their data. Control owns identity, orgs, sessions, audit, quotas, entitlements and Space authority decisions. Application owns Space projections and notifications. Model owns runs, capabilities, sandboxes and browser grants. Data Plane owns the canvas. There is no direct database access across planes. Zero Data Retention must propagate through every boundary that persists content.
- The model may propose browser actions. Quarry executes or rejects them.
- Commits use Conventional Commits. Commits made by the assistant end with `Co-Authored-By: Claude Haiku 5.5 <noreply@anthropic.com>`.
- Subagents run on Haiku 5.5 at max effort for most work. Use Opus only for genuinely hard items (owner, 2026-11-23).
- Run one builder per lock group at a time in one checkout. Parallel builders on the same files collide.
- A run that stopped at the usage limit is not resumed. Recompute the remaining set from the run journals and start a new run.
- Do not run `docker compose up` for the stacks from an agent. If the Docker engine wedges (tests that call localhost:9091 hang), the runbook is `apps/Model Plane/scripts/dev/docker-desktop-recover.ps1`: a dry run first, then `-Apply`. A second `-Apply` may be needed. A wedged engine also makes the gitleaks pre-commit hook report false secrets.
- Check free space on C: before and after Rust builds. Agent cargo targets have filled the drive before.
- On Windows some Model Plane cargo targets fail before any change; use `--lib` for those. Go `-race` needs cgo, which is not available here.
- Protos: `buf` 1.72 and `protoc-gen-go` v1.36.12 are installed locally. Generate Go bindings into a scratch folder and copy only the changed `*.pb.go` files. Run the lint ratchet and `buf breaking` against `baseline.binpb`.

## 6. Checks that passed at this point

Run these from the repository root. The sync script runs under bash (Git Bash on Windows).

```powershell
cargo test --manifest-path "apps/Frontend Plane/verevonv3/apps/gateway/Cargo.toml" --test space_routes_zdr_classes
```

```powershell
cargo test --manifest-path "apps/Model Plane/rust/Cargo.toml" -p model-gateway zdr_posture
```

```powershell
cargo test --manifest-path "apps/Model Plane/rust/Cargo.toml" -p session-core --bin session-core space::browser_read
```

```powershell
bash scripts/tests/zdr-contract-sync.test.sh
```

```powershell
go -C "apps/Control Plane/user-core" test ./internal/http/ -run "SpaceSurface|SurfaceRegistry|Registry" -count=1
```

Expected at `7730b3528`: the gateway route lint passes 5 of 5; model-gateway `zdr_posture` passes; the sync script reports 204 passed, 0 failed; the user-core registry tests pass.

## 7. Not verified

- The wiring in section 3 was read from tracked code on 2026-11-26. It was not checked against running stacks or deployed environment files.
- No Space behaviour was exercised live. The DB-gated session-core tests have not run.
