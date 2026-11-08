# 4. Sequencing and parallelism (Space plan section 4)

Author: Sonnet analyst, 2026-10-20. Inputs: audit sections 9.1 and 9.4; designs WS01-WS20 (Slices, Effort, Feature flags, Conflict hotspots); `haiku/live-tree-delta.md` sections 4-6; `haiku/build-test-gates.md`; analyses `02-decisions.md` (SD ids, gates G0-G6), `05-cross-session.md` (shared-piece owners, reservations 5.1) and `06-verification.md` (M1-M3 scenarios); the two sibling plans and branches. Nothing here changes product code. All effort figures are engineer-weeks (ew) of one engineer who knows the plane, taken from the designs; the roll-up and the schedule simulation in 4.7 were computed by a script from the slice table in 4.2, so the tables agree with each other.

## 4.0 Basis, verified state and conventions

### 4.0.1 What was re-verified on 2026-10-20 (read-only git, `git --no-optional-locks -C C:/dev/CoresSystem`)

| Fact | Evidence | Consequence |
|---|---|---|
| Sibling (b) branch `model-plane` is at `d2ea86a1` (90 commits over `afb83928`, 47 more than the 43 that section 5 counted at `e8f3561c`), local, unpushed | `git rev-list --count afb83928..model-plane` = 90 | Space base must be cut from it (SD-11 option a) and rebased per train |
| Top session-core migration on `model-plane` is still **0048** (`0046_approval_consume_and_mode`, `0047_run_loop_checkpoints`, `0048_tool_action_effect_ledger`); nothing new since section 5 | `git ls-tree model-plane .../session-core/migrations` | The renumbering in 4.6 (Space 0071-0090) stands; the live tree still shows next free 0046, `model-plane` shows 0049 |
| Proto since `e8f3561c` on `model-plane`: `RunAgentResponse.park_reason = 11` (execution.proto), `ConsumeApprovalRequest.omit_call = 7` (orchestration.proto), `SaveLoopCheckpointResponse.park_accepted = 2`, `OrchestrationEvent` oneof 24 `run_parked` and 25 `run_resumed` now BUILT, new RPC `ListToolActions` (+ `ToolActionState`) in sessions.proto, `AgentResume` kind `park` | `git diff e8f3561c..model-plane -- "apps/Model Plane/proto"` | None of these tags is a Space tag; 4.6 adds them to the taken list |
| Sibling (a) branch `quarryv2-next` is at `e7c321d5` (2026-10-20), `quarryv2` at `bed81d16` | `git rev-parse` | no new session-core or user-core migration on either (top session-core on `quarryv2-next` is 0043; user-core 029) |
| The two sibling branches overlap EACH OTHER in 26 files, among them gateway `config.rs`, `src/shared/actions/action-registry.ts`, `src/shared/i18n/errors.ts`, `main.rs`, `upstream.rs`, model-gateway `http_routes.rs`, `state.rs`, `session_flow.rs`, `browser_run.rs`, `.github/workflows/buf.yml`, `contract-goldens.yml` | `comm -12` of both `git diff --name-only afb83928..<branch>` lists | Space inherits two moving bases for the same hot files; the hotspot rules in 4.4 are not optional |
| `model-plane` already edits Space hotspots: gateway `domains/spaces.rs` +10, `action-registry.ts` +64, `sessions.proto` +237, `session-core/grpc.rs` +1,727, `auth.rs` +396, model-gateway `http_routes.rs` +303, `sse.rs` +6,305 | `git diff --stat afb83928..model-plane -- <paths>` | Every Space slice touching these files rebases on (b); `sse.rs` is off limits to Space (4.4 row 13) |
| The live tree is still `main` `afb83928` with 326 uncommitted entries (Frontend 108, Model 78, Ingestion 66, Data 57, Control 2, other 15) | `git status --short \| wc -l` | Disposition table 4.1.2 |
| user-core top migration is 029 on `main`, `model-plane` and `quarryv2-next`; the runner applies pending `*.up.sql` in lexicographic order (`internal/database/database.go:72-74`) | `ls-tree`, source read | Gaps in the user-core sequence are safe; never renumber after merge |
| Paths of the three first PRs (4.8) are unchanged on `model-plane`, `quarryv2`, `quarryv2-next` and clean in the live tree | `git diff --stat afb83928..<branch> -- audit-core user-core features/spaces features/settings domains/audit.rs set-space-effect-policy.sh` empty; live status shows only `SpaceRoomComposer(.test).tsx` | They can start on any base today |

Hot-file sizes in the live tree (lines): gateway `domains/spaces.rs` 9,882; `config.rs` 593; SPA `SpacePage.tsx` 1,447, `SpaceRoomComposer.tsx` 800, `SpaceRoomTimeline.tsx` 486, `action-registry.ts` 3,151, `spaces-client.ts` 1,028; session-core `grpc.rs` 13,341, `auth.rs` 2,095, `main.rs` 248, `sessions.proto` 1,249; model-gateway `http_routes.rs` 11,681, `sse.rs` 13,325; Convex `schema.ts` 1,073, `spaces.ts` 1,555; user-core `http/server.go` 724, `http/service_auth.go` 239, `http/spaces.go` 1,500, `spaces/repository.go` 1,406.

### 4.0.2 Conventions

- **Slice ids**: `WSnn-Sk` as in the designs (`WS18-S4a` is a new split, see 4.2). `M0-*` = baseline slices, `P-*` = prep refactors (new, 4.4.2). `[cond]` = conditional slice (decision-, sibling- or trigger-gated, outline only, or optional): sized, scheduled last, excluded from firm totals.
- **Dependency types**: **H** hard = the dependency's code, data or migration must be merged before this slice can merge (same lane or runtime need). **(c)** contract-first = the slice may start and be tested against the dependency's contract (proto, golden, OpenAPI, fixture, wiremock) once that contract is published, but cannot merge or be enabled before the dependency merges. **soft** = functional nicety or advice only. Orderings that exist only to avoid a merge conflict are not dependencies; they live in 4.4.
- **Milestones** use the numbering of `06-verification.md` (M1 = W1, M2 = W2, M3 = W3) and add M0 (baseline) and M4 (production enablement). Gates G0-G6 are those of `02-decisions.md`.
- **Lanes** `L0..L10` are worktree and file-ownership groups (4.3). **Skills**: MS Model session-core and model-gateway (Rust); MX execution-core, capability-core, sandbox-manager, browser-broker, letta-bridge (Rust and Go); GW gateway BFF (Rust); SP SPA (Solid, TS); CT Control (Go and NestJS); AP Application (Convex TS, notification-core Go); DT Data and Ingestion (Go, Rust).

### 4.0.3 Where this section disagrees with an input, and what is decided

| # | Input says | This section | Reason |
|---|---|---|---|
| D1 | `06` 6.0: M1 = WS01-WS07 whole; T0 due "week 1 of M1" | M1 = WS01-WS07 firm slices **plus WS09-S1 and WS17-S1/S2**; WS04-S8..S12, WS05-S4..S9 and WS06-S6..S10 move to M2; T0 becomes the M0 slice `M0-T0` | WS09-S1 (thread run guard) must be ON before member posting (WS09 design, `02` 2.5), M1-S1 asks the agent in a shared thread; `02` 2.3.5 #7 moves WS17-S1/S2 into W1; WS05-S7/S8 are 8-11 ew of SSO and SCIM that would swamp W1; T0 blocks every live scenario so it cannot wait for M1 |
| D2 | `06` M1-S8 expects each sandbox cap (memory, CPU, file, output, fork bomb) in M1 | M1 verifies only the container limits of WS06-S7; per-spawn `prlimit` caps (WS06-S8) are verified in M2.3 | S8 is in W2 in the WS06 design |
| D3 | `02` SD-29: WS07-S9 creates `SpaceRoomReadService`, WS15-S6 and WS11-S2 consume | the empty service skeleton, the `space/` module and the model-gateway `space_routes/` seam are carved out into `P-SC1` (0.8-1.2 ew, net +0.4 ew because the rest comes out of WS01-S1, WS07-S9 and WS15-S6); WS07-S9 keeps the artifact aggregate | otherwise WS13-S1, WS15-S6 and WS11-S2 wait on a 1.3-1.8 ew L slice for an empty file |
| D4 | WS03 design: WS03-S1 is only "soft" for entitlement slices (WS09-S7, WS10-S2, WS12-S5, WS17-S4, WS19-S2, WS20-S1) | still functionally soft, but an **ordering rule**: WS03-S1 merges before any of them (it turns each entitlement into one catalogue row) | removes six edits of `UpsertEffectPolicy` |
| D5 | WS06 design: WS06-S0 (land the Z-series, 1.5-3 ew) blocks everything | WS06-S0 is dropped from the plan (0 ew): the Z-series is committed on `model-plane`; its replacement is `M0-BASE` (0.3-0.5 ew) | `02` 2.5 and `05` 5.0.1 |
| D6 | WS18 design: S2 needs "S4a" first and S4 needs S2 | S4a (execution-core tolerates the new response fields, 0.4-0.5 ew) is split out of WS18-S4 as its own slice to break the cycle in the dependency graph | graph must be acyclic |
| D7 | Designs' migration numbers 0046-0055, 030-042 | placeholders; final numbers are in 4.6, taken from `05` 5.1 and extended (WS07-S9 gets 0085) | `06` D1 |
| D8 | Audit sizes (S/M/L/XL, no weeks) | designer slice sums adopted everywhere; the audit-equivalent is shown for comparison in 4.7.2 | designers read the code |

## 4.1 Milestones M0..M4

### 4.1.1 Milestone table

Firm effort is the sum of slice low-high ew from 4.2 (conditional slices excluded). Sub-gates M2.x and M3.x exist because M2 and M3 are each 100+ ew; they let a staged release ship before the whole wave is done.

| Id | Name | Slices (firm) | Firm ew low-high | Exit criteria (all must hold) |
|---|---|---|---|---|
| **M0** | Baseline and seams | `M0-BASE`, `M0-T0`, `M0-CI`, `P-SC1`, `P-GW1`, `P-UC1`, `P-CX1` | 4.1-6.6 | (1) G0 and G1 closed: SD-11 answered (base branch `space` cut from the then-current `model-plane` tip, SHA recorded; integrator named per Model DEC-39; one append-only ledger entry carrying 4.6), SD-01 accepted, SD-15 ratified for schema-bearing slices. (2) Live-tree residue disposed (4.1.2). (3) `M0-T0`: `scripts/space-e2e-seed.sh` seeds users A (owner), B (editor), C (viewer), E (outsider), D (org Y), F (ZDR org Z), `apps/Frontend Plane/verevonv3/tests/e2e/space-fixtures.ts` turns a skipped Space spec into a failure under `E2E_REQUIRE_SPACE=1`, and the baseline failing-test sets of `06` 6.6 are stored under `docs/plans/evidence/baseline-<date>/`. (4) `M0-CI`: `.github/workflows/space-gate.yml` (base-agnostic, path-filtered) and `convex-core.yml` run the Space subset of `06` 6.2; `buf.yml` paths quoted. (5) The four seam PRs are pure moves: test counts unchanged, `git diff -w --stat` shows moved lines only, all suites green against the stored baseline. |
| **M1** | W1 foundations: people can post, the room is live, policy is administrable, security holes closed, memory contained | WS01 S1-S8; WS02 S1-S5; WS03 S1-S7; WS04 S1-S7; WS05 S1-S3; WS06 S1-S5, S7; WS07 S1-S13 (incl. S9a); WS09-S1; WS17 S1, S2; prep `P-SPA1`, `P-SPA2`, `P-CFG` | 46.3-73.5 | (1) `06` scenarios M1-S1..M1-S7 pass on rebuilt images with a non-author running them (protocol 6.4); M1-S8 passes for SCIM 501 and container limits only (D2). (2) G2 closed before WS01-S3 and WS02-S3 merge; G3 (SD-12 ZDR behaviour) closed before any enforcement flag flips. (3) WS09-S1 guard is ON in every environment before `space_human_posts_core` is ON anywhere. (4) WS17-S1 (`MEMORY_SPACE_CONTAINMENT`) is ON everywhere with one week of counters, before sibling (b) enables `GATEWAY_CHAT_PROJECTS_ENABLED`. (5) WS05-S1 deployed (this releases (b)'s mp-WS01.S2 audit replay, O13), WS05-S2 and WS05-S3 deployed. (6) ZDR tour passes (zero rows in `messages`, `space_event_log`, notification tables). (7) Dev has all M1 flags on; staging has WS01-WS04 on with a real second member. (8) Ledger rows for built migration and proto numbers appended. |
| **M2.1** | Agents in a human room | WS08 S1-S8; WS09 S2-S8; WS10 S1, S2, S3, S4, S5a, S5b | 33.5-48.7 | `06` M2-S1 and the owner-path half of M2-S3 (gated write by the owner, decide once, thread grant, revoke, `block` ceiling, ZDR refusal). G4 closed for SD-00, SD-05 (statement), SD-20, SD-21, SD-26. Run visibility text frames stay off in production (SD-20). Member decide and quarantine are not part of this gate. |
| **M2.2** | Attention, DMs, message lifecycle, membership depth | WS04 S8-S12; WS11 S1-S11; WS12 S1-S8 | 33.0-48.4 | `06` M2-S4 and M2-S5. G4 closed for SD-13, SD-14, SD-17, SD-19. `WS04-S4` writer is the only room-membership writer in every org where DMs or the directory are on. |
| **M2.3** | Search, files, timeline, UI baseline, sandbox caps, enterprise identity, member approvals | WS05 S4-S9; WS06 S6, S8, S9, S10; WS10 S6-S9; WS13 S1-S8; WS14 S1-S9; WS15 S1-S9, S11, S12; WS16 S1-S11 | 72.6-109.3 | `06` M2-S6, M2-S7, M2-S8 and the remaining half of M2-S3 (member decide, once-token race, quarantine); M2-S2 (revocation mid-run and deprovision) needs WS05-S8/S9 and WS04-S10. G5 closed (flag-on in an org). Per-spawn caps verified on the production-profile stack (WS06-S10). **M2 exit = M2.1 + M2.2 + M2.3.** |
| **M3.1** | Personal agents, memory labels, self-API, canvas | WS17 S3-S9; WS18 S1-S7, S4a, S10; WS19 S1-S9 | 38.0-56.5 | `06` M3-S1..M3-S5. G6 closed for SD-03, SD-04, SD-22. WS17-S3 labels ON before any personal agent is admitted to a shared room (`enforce` only after two weeks of `observe`). WS19 canvas stays pilot-only until SD-08. |
| **M3.2** | Room browser and the colleague day | WS20 S1-S9 | 19.8-28.7 | `06` M3-S6, M3-S7, M3-S8. Needs sibling (a) deliveries (F-t2-001 grants, F-t2-013 phase 1 room view, takeover enforcement) in the same stack; SD-23 answered. WS20-S2/S3 can finish against a test-double broker; S6, S8, S9 cannot be signed off without (a). |
| **M4** | Production enablement and closeout | none (rollout) | n/a (about 0.2 ew per flag flip) | Per org, in the order of 4.9: flags at target default, kill-switch drill per feature (flag off returns legacy behaviour, no data change), evidence bundles filed under `docs/plans/evidence/`, dated note beside `apps/Frontend Plane/verevonv3/docs/SPACE_*`, `WS04-S12` executed after one release with WS04-S6 on, conditional slices triaged against epics E01-E12 (`03z-epics.md`), stale flags removed one release after their default flips. M4 runs per org in parallel with the later milestones; it is the only milestone that may overlap its successor entirely. |

Conditional and parked slices (not in any exit criterion): WS02-S6, WS02-S7, WS06-S6b, WS06-S11, WS06-S12, WS09-S9, WS13-S9, WS13-S10, WS13-S11, WS15-S10, WS15-S13, WS18-S8, WS18-S9, WS19-S10 (14 slices, 27.0-42.5 ew). WS08-S9..S11 are outline only and not sized.

### 4.1.2 Live-tree residue: what M0 must decide (the base-branch question)

The brief asks that the implementation base either contains the live tree's pending work or carries an explicit decision about it. The base is `model-plane`, which already carries the Model Plane Z-series; the rest is a decision list.

| Residue in `C:/dev/CoresSystem` (uncommitted) | State on `model-plane` | Decision | Blocks |
|---|---|---|---|
| ZDR Z-series in session-core, model-gateway, execution-core, mp-contracts, `zdr-refusals.json`, SPA `zdr-refusals.ts`, retention receipt, migrations 0044/0045 | committed there (`5ae7fe13`..`b67bb7f0`, `c2f5c802`) | Owner discards the duplicate dirty files after `model-plane` merges; never commit them a second time. Space never reads the dirty copy. | nothing once the base is cut |
| `SpaceRoomComposer.tsx` (2 lines, refusal feature key) and `SpaceRoomComposer.test.tsx` | not present | Owner lands it as one small PR on `space` or discards it before `P-SPA2`. Space depends on neither. | `P-SPA2` (same file) |
| `execution-core/src/workspace_promote.rs` (7 lines, typed ZDR refusal) | not present | Owner lands or discards. | nothing (WS06-S9 touches `runtime_loop`, not this file) |
| Data Plane `retrieval-engine-rs` (12+ files: egress attestation Z-41, `authz/service_routes.rs` Space-scope guard, `api/mod.rs`, `config.rs`, `main.rs`, `embed/mod.rs`) | **not present** (0 retrieval-engine paths changed on `model-plane`) | Owner commits as its own reviewed PR on `space` (or main) before WS14-S5 and WS17-S9, which edit the same files | WS14-S5, WS17-S9 |
| SPA `notifications-client.ts` (contract alignment), `AgentRunConsole.tsx`, `ChatMessages.tsx`, `chat-client.ts` ZDR presentation | `chat-client.ts` and `zdr-refusals.*` committed; the other three not present | Owner lands `notifications-client.ts` before WS11-S3; the other two are not Space dependencies | WS11-S3 |
| Quarry/Ingestion (66 entries), Data Plane rerank (about 45 entries), demo and scene work (`VITE_DEMO`) | sibling (a) branches or out of scope | Not Space. Keep out of every Space change set. | none |
| `.github` workflow edits that call uncommitted scripts (`dataplane-v2-ci.yml`, `verevonv3-ci.yml`, `quarry-docs-contract.yml`) | partly | Out of Space scope; `M0-CI` adds its own workflow and does not depend on them | none |

### 4.1.3 Gates map (from `02-decisions.md` 2.4)

| Gate | Meaning | Closes at | Who |
|---|---|---|---|
| G0 | before the first Space code PR that is not a List-A slice | M0 exit | owner (SD-11, SD-01) |
| G1 | before the first merge of a slice that edits `grpc.rs`, `auth.rs`, a `.proto` or adds a migration | M0 exit | integrator (ledger entry), SD-15 |
| G2 | before WS01-S3 (member append) and WS02-S3 (stream frames) | mid-M1 | owner (SD-01, SD-02) |
| G3 | W1 exit and first staging enablement; before any enforcement flag flip | M1 exit | owner (SD-12), mp-WS01 S2 outbox fix deployed for WS02 live enablement |
| G4 | W2 start (WS08-WS15 main slices) | M1 exit, per slice group (SD-00, SD-05, SD-13..SD-21) | owner |
| G5 | W2 flag-on in an org | M2.1..M2.3 exits | owner (SD-18 entitlements) |
| G6 | W3 start | M3 entry (SD-03, SD-04, SD-22, SD-23) | owner |

## 4.2 Slice dependency graph

211 slices: 10 baseline and prep rows, 187 firm design slices, 14 conditional. Legend as in 4.0.2. `Blocks` lists up to seven direct dependents (hard or contract-first); the full list is the inverse of the dependency columns. `M0-BASE` is omitted from the dependency column where another dependency exists. Generic seam dependencies: every GW slice depends on `P-GW1`, every SP slice (except `WS01-S7a`, `WS07-S1`, `WS07-S5`, `WS07-S13`, `WS16-S1`) on `P-SPA1`, every L4 Control slice (except `WS03-S1`, `WS03-S2`, `WS06-S5`) on `P-UC1`, every L6 Convex slice (except `WS04-S1`, `WS04-S2`) on `P-CX1`; `P-SPA2` is a hard dependency of the composer slices; these are in the table.

| Slice | What | Lane | MS | Size | Hard (H) and contract-first (c) dependencies | Soft | Blocks |
|---|---|---|---|---|---|---|---|
| M0-BASE | base branch cut, ledger entry, decisions SD-11/15 | L0 | M0 | S | none | - | M0-T0, M0-CI, P-SC1, P-GW1, P-UC1, P-CX1, P-SPA1, +85 more |
| M0-T0 | T0 seed script, fixtures, baseline capture | L0 | M0 | M | M0-BASE | WS03-S1 | - |
| M0-CI | space-gate.yml, convex-core.yml, buf path fix | L0 | M0 | S | M0-BASE | - | - |
| P-SC1 | session-core space/ module, append_as seam, read-service skeleton, model-gateway space_routes seam | L1 | M0 | S/M | M0-BASE | - | WS01-S1, WS02-S1, WS07-S9, WS13-S1, WS15-S5, WS17-S1 |
| P-GW1 | gateway spaces/ module tree, SpaceFeatureFlags, error_with_details, org_zdr_lookup | L7 | M0 | S | M0-BASE | - | WS01-S5, WS02-S4, WS03-S4, WS03-S5, WS04-S6, WS04-S9, WS07-S3, +14 more |
| P-UC1 | user-core route and scope-table seams | L4 | M0 | S | M0-BASE | - | WS01-S2, WS02-S2, WS03-S3, WS04-S3, WS04-S4, WS04-S7, WS04-S8, +21 more |
| P-CX1 | Convex schema.ts split per feature | L6 | M0 | S | M0-BASE | - | WS04-S5, WS04-S12, WS08-S5, WS11-S3, WS11-S6, WS11-S9, WS11-S10, +8 more |
| P-SPA1 | SPA tab registry, spaces-client and action-registry fragments, errors.ts sections | L8 | M1 | M | WS01-S7a | - | P-SPA2, WS01-S6, WS01-S7b, WS01-S8, WS02-S5, WS03-S6, WS03-S7, +41 more |
| P-SPA2 | composer extension slots and send pipeline seam | L8 | M1 | M | P-SPA1 | - | WS01-S6, WS08-S6, WS09-S2, WS12-S7, WS13-S7, WS14-S8, WS15-S2, +1 more |
| P-CFG | Control PR train for principals, nats.conf, compose (6 trains) | L4 | M1 | S | M0-BASE | - | - |
| WS01-S1 | Session Core schema, post_kind, containment filters | L1 | M1 | M | P-SC1 | - | WS01-S2, WS01-S3, WS01-S8, WS06-S12, WS08-S2, WS12-S4, WS15-S6, +3 more |
| WS01-S2 | Control thread-append v2 and verifier golden | L4 | M1 | S | P-UC1, WS01-S1 (c) | - | WS01-S3, WS01-S5 |
| WS01-S3 | Session Core member append path | L1 | M1 | L | WS01-S1, WS01-S2 (c) | WS09-S1 | WS01-S4, WS10-S6, WS10-S7, WS13-S10, WS13-S11 |
| WS01-S4 | model-gateway post routes, error map, ZDR | L1 | M1 | M | WS01-S3 | - | WS01-S5, WS01-S8, WS08-S3, WS13-S10, WS13-S11 |
| WS01-S5 | BFF post routes, flags, features, action contracts | L7 | M1 | M | P-GW1, WS01-S2 (c), WS01-S4 (c) | - | WS01-S6, WS08-S4, WS12-S3, WS12-S8, WS13-S10, WS15-S10, WS16-S8, +4 more |
| WS01-S6 | SPA post composer, journal, optimistic row | L8 | M1 | M | P-SPA2, P-SPA1, WS01-S5 (c) | - | WS01-S7b, WS08-S6, WS12-S3, WS17-S6 |
| WS01-S7a | SPA AuxPane, read-only thread panel, deep link | L8 | M1 | M | M0-BASE | - | P-SPA1, WS01-S7b, WS12-S7, WS13-S4a, WS17-S6, WS20-S5 |
| WS01-S7b | reply composer in pane, collapse, unread parity | L8 | M1 | S/M | WS01-S6, WS01-S7a, P-SPA1 | WS01-S1, WS01-S4 | WS12-S3, WS12-S7 |
| WS01-S8 | reliable send on the agent path (F-t1-033) | L8 | M1 | S | P-SPA1, WS01-S1 (c), WS01-S4 (c) | - | - |
| WS02-S1 | space event log, triggers, tailer, erasure hooks | L2 | M1 | M | P-SC1 | - | WS02-S3, WS02-S7, WS11-S5 |
| WS02-S2 | Control stream-read audience | L4 | M1 | S | P-UC1 | - | WS02-S3 |
| WS02-S3 | SpaceEvents gRPC, delivery gate, model-gateway SSE | L2 | M1 | L | WS02-S1, WS02-S2 (c) | - | WS02-S4, WS02-S6, WS19-S10 |
| WS02-S4 | BFF stream route, frame validator, limiter | L7 | M1 | M | P-GW1, WS02-S3 (c) | - | WS02-S5 |
| WS02-S5 | SPA stream controller, indicator, presence decoupling | L8 | M1 | M | P-SPA1, WS02-S4 (c) | - | - |
| WS02-S6 [cond] | scale and ops (conditional on a measured trigger) | L2 | M2 | M | WS02-S3 | - | - |
| WS02-S7 [cond] | NATS projection for Application (reserved, owner WS11) | L2 | M2 | M | WS02-S1 | - | - |
| WS03-S1 | catalogue-driven UpsertEffectPolicy, events, outbox audit | L4 | M1 | M | M0-BASE | - | WS03-S3, WS05-S5 |
| WS03-S2 | typed denial vocabulary and issuer retrofit | L4 | M1 | M | M0-BASE | - | WS03-S5, WS03-S7 |
| WS03-S3 | org-admin policy routes, presets, write switch | L4 | M1 | M | WS03-S1, P-UC1 | - | WS03-S4 |
| WS03-S4 | gateway admin routes, flag, dispatchers, registry entries | L7 | M1 | S | P-GW1, WS03-S3 (c) | - | WS03-S6 |
| WS03-S5 | gateway typed denial mapping | L7 | M1 | S | P-GW1, WS03-S2 (c) | - | WS03-S7 |
| WS03-S6 | SPA policy console | L9 | M1 | M | P-SPA1, WS03-S4 (c) | - | - |
| WS03-S7 | SPA denial presenter and copy | L9 | M1 | M | P-SPA1, WS03-S2 (c), WS03-S5 (c) | - | - |
| WS04-S1 | org-room roster reconcile cron and hook | L6 | M1 | S | M0-BASE | - | - |
| WS04-S2 | room name and topic end to end | L6 | M1 | S | M0-BASE | - | - |
| WS04-S3 | people search (user-core SQL, route, picker) | L4 | M1 | M | P-UC1 | - | WS04-S9 |
| WS04-S4 | Control membership writer, ledger, scopes | L4 | M1 | M | P-UC1 | - | WS04-S5, WS04-S6, WS04-S7, WS04-S8, WS04-S10, WS15-S11, WS17-S4 |
| WS04-S5 | recipient audience follows the roster | L6 | M1 | M | P-CX1, WS04-S4 (c) | - | WS04-S6, WS04-S8, WS04-S11 |
| WS04-S6 | Members tab on the Control writer | L7 | M1 | M | P-GW1, WS04-S4 (c), WS04-S5 (c) | WS04-S3 | WS04-S8, WS04-S12 |
| WS04-S7 | archive and restore | L4 | M1 | M | WS04-S4, P-UC1 | - | - |
| WS04-S8 | visibility, directory policy, self-join | L4 | M2.2 | M/L | WS04-S4, P-UC1, WS04-S5 (c), WS04-S6 (c) | - | WS04-S11 |
| WS04-S9 | composed people and agent directory | L7 | M2.2 | M | P-GW1, WS04-S3 (c) | - | - |
| WS04-S10 | durable membership events | L4 | M2.2 | S/M | WS04-S4, P-UC1 | - | WS04-S11, WS05-S8, WS05-S9 |
| WS04-S11 | opt-in history window (since_join) | L4 | M2.2 | L | WS04-S10, P-UC1, WS04-S5 (c), WS04-S8 (c) | - | - |
| WS04-S12 | retire Application member list | L6 | M2.2 | S | WS04-S6, P-CX1 | - | - |
| WS05-S1 | role-gated audit reads | L5 | M1 | S | M0-BASE | - | WS05-S4 |
| WS05-S2 | SCIM stub to 501, redaction migration | L5 | M1 | S | M0-BASE | - | WS05-S8 |
| WS05-S3 | remove SSO attribute-to-admin rule | L5 | M1 | S | M0-BASE | - | WS05-S7 |
| WS05-S4 | audit list v2, admin view, bounded export | L5 | M2.3 | M | WS05-S1 | - | - |
| WS05-S5 | Space decision audit events | L4 | M2.3 | M | WS03-S1, P-UC1 | - | - |
| WS05-S6 | DNS domain verification, identity audit outbox | L5 | M2.3 | M | M0-BASE | - | WS05-S7, WS05-S8 |
| WS05-S7 | SSO policy, break-glass, enforcement hook | L5 | M2.3 | L | WS05-S3, WS05-S6 | - | WS05-S8 |
| WS05-S8 | SCIM Users in org-core, deprovision | L5 | M2.3 | L | WS05-S2, WS05-S6, WS05-S7, WS04-S10 (c) | - | WS05-S9 |
| WS05-S9 | deprovision propagation probe, runbook | L5 | M2.3 | S | WS05-S8, WS04-S10 (c) | - | - |
| WS06-S1 | type the 11 Session Core refusals, inventory test | L2 | M1 | S | M0-BASE | - | - |
| WS06-S2 | gate Space decision ZDR flag (observe/enforce) | L2 | M1 | S | M0-BASE | - | - |
| WS06-S3 | metric-label guard | L3 | M1 | S | M0-BASE | - | - |
| WS06-S4 | route-class registry (model-gateway, BFF table) | L2 | M1 | S | M0-BASE | - | - |
| WS06-S5 | Control capability decision: egress vocabulary | L4 | M1 | S | M0-BASE | - | WS06-S6, WS06-S9 |
| WS06-S6 | sandbox-manager ZDR gate interceptor | L3 | M2.3 | M | WS06-S5 (c) | - | WS06-S6b |
| WS06-S6b [cond] | scratch-only leases under ZDR (decision WS06-D2) | L3 | M2.3 | M | WS06-S6 | - | - |
| WS06-S7 | container limits, bounded output | L3 | M1 | S | M0-BASE | - | WS06-S8, WS06-S10 |
| WS06-S8 | per-spawn prlimit, limit classification | L3 | M2.3 | M | WS06-S7 | - | WS06-S10 |
| WS06-S9 | SpawnAdmission, shell lease, cleared env | L3 | M2.3 | M | WS06-S5 (c) | - | WS06-S10, WS06-S11 |
| WS06-S10 | production-profile verification and calibration | L3 | M2.3 | M | WS06-S7, WS06-S8, WS06-S9 | - | - |
| WS06-S11 [cond] | sandbox egress through Quarry tunnel (parked, XL) | L3 | M3 | XL | WS06-S9 | - | - |
| WS06-S12 [cond] | thread retention receipt (optional) | L2 | M3 | M | WS01-S1 | - | - |
| WS07-S1 | RegionBoundary, FailureCard, classifier | L9 | M1 | M | M0-BASE | - | WS07-S2 |
| WS07-S2 | build id, version.json, nginx snippets, chunk recovery | L9 | M1 | M | WS07-S1, P-SPA1 | - | WS07-S3, WS07-S6 |
| WS07-S3 | gateway client-config, contract constants | L7 | M1 | M | WS07-S2, P-GW1 | - | WS07-S6 |
| WS07-S4 | restricted state for shared_read | L8 | M1 | S | P-SPA1 | - | WS07-S9 |
| WS07-S5 | link safety and sandbox wrapper fix | L9 | M1 | S | M0-BASE | - | WS07-S10, WS15-S4, WS15-S10 |
| WS07-S6 | CSP report-only and csp-report route | L7 | M1 | M | WS07-S2, WS07-S3, P-GW1 | - | - |
| WS07-S7 | avatar read path and AgentAvatar | L9 | M1 | S | P-SPA1 | - | WS07-S8 |
| WS07-S8 | identity hover card | L9 | M1 | M | WS07-S7, P-SPA1 | - | - |
| WS07-S9 | Session Core SpaceRoomReadService, artifact aggregate | L1 | M1 | L | P-SC1, WS07-S4 (c) | - | WS07-S12 |
| WS07-S9a | room metadata allowlist in model-gateway | L2 | M1 | S | M0-BASE | - | - |
| WS07-S10 | viewer registry and surfaces | L9 | M1 | M | WS07-S5, P-SPA1 | - | WS07-S11 |
| WS07-S11 | turn mapper, artifact chips, per-thread sheet | L8 | M1 | M | WS07-S10, P-SPA1 | WS07-S9a | WS07-S12 |
| WS07-S12 | Artifacts tab on the aggregate | L8 | M1 | M | WS07-S11, P-SPA1, WS07-S9 (c) | - | - |
| WS07-S13 | artifact-first instruction snippet | L9 | M1 | S | M0-BASE | - | - |
| WS08-S1 | Control model.run.wake decision | L4 | M2.1 | M | P-UC1 | - | WS08-S2, WS08-S4 |
| WS08-S2 | Session Core mentions, wake verification, run columns | L1 | M2.1 | L | WS01-S1, WS09-S1, WS08-S1 (c) | - | WS08-S3, WS08-S7, WS10-S7, WS18-S8 |
| WS08-S3 | model-gateway wake mode, skip-append, history splice | L1 | M2.1 | L | WS08-S2, WS01-S4 | - | WS08-S4, WS08-S7 |
| WS08-S4 | BFF plan_mentions, policy split, wake injector | L7 | M2.1 | L | P-GW1, WS08-S1 (c), WS08-S3 (c), WS01-S5 (c) | - | WS08-S6 |
| WS08-S5 | binding wake policy (Convex, PATCH route, retire group) | L6 | M2.1 | M | P-CX1 | - | WS08-S8 |
| WS08-S6 | SPA structured mentions, picker, sequential wakes | L8 | M2.1 | L | WS01-S6, P-SPA2, P-SPA1, WS08-S4 (c) | - | WS08-S8 |
| WS08-S7 | mention notifications producer | L2 | M2.1 | M | WS08-S2, WS08-S3 | WS11-S9 | - |
| WS08-S8 | Agent tab policy editor, why-no-answer copy | L8 | M2.1 | S | WS08-S6, P-SPA1, WS08-S5 (c) | WS10-S5b | - |
| WS09-S1 | honest run phase, per-thread run guard | L2 | M1 | M | M0-BASE | - | WS08-S2, WS20-S3 |
| WS09-S2 | owner mid-run steer in room composer | L8 | M2.1 | S | P-SPA2, P-SPA1 | WS09-S1 | - |
| WS09-S3 | room-safe projector, run pointer, work summary | L2 | M2.1 | M | M0-BASE | - | WS09-S4 |
| WS09-S4 | member live attach (Session Core, SSE, gateway) | L2 | M2.1 | L | WS09-S3 | - | WS09-S5, WS09-S8, WS09-S9, WS20-S4 |
| WS09-S5 | run card, work disclosure, Work and Activity adapter | L8 | M2.1 | M | P-SPA1, WS09-S4 (c) | WS09-S3 | WS09-S6 |
| WS09-S6 | activity strip, sidebar working state | L8 | M2.1 | S | WS09-S5, P-SPA1 | WS01-S7a | - |
| WS09-S7 | Control model.run.control and entitlement bit | L4 | M2.1 | M | P-UC1 | WS03-S1 | WS09-S8, WS20-S9 |
| WS09-S8 | manager Stop end to end | L2 | M2.1 | M | WS09-S4, WS09-S7 (c) | - | WS20-S9 |
| WS09-S9 [cond] | member steer by policy (outline) | L2 | M3 | L | WS09-S4 | - | - |
| WS10-S1 | honest approval card, typed context, derived continuation | L2 | M2.1 | M | M0-BASE | - | WS10-S3, WS10-S4, WS10-S6, WS10-S8, WS10-S9 |
| WS10-S2 | Control grants, approval policy, gate/use endpoints | L4 | M2.1 | M | P-UC1 | WS03-S1 | WS10-S3, WS10-S4, WS10-S5a |
| WS10-S3 | execution-core gate, once-token, grant pass | L3 | M2.1 | L | WS10-S1, WS10-S2 (c) | - | WS10-S4, WS10-S6, WS10-S8, WS10-S9 |
| WS10-S4 | Space decide route, scope picker, standing list | L7 | M2.1 | M | P-GW1, WS10-S1 (c), WS10-S2 (c), WS10-S3 (c) | - | - |
| WS10-S5a | tool policy store (org floor, Space ceiling, agent) | L4 | M2.1 | M | WS10-S2, P-UC1 | - | WS10-S5b |
| WS10-S5b | editor routes, Convex projection, Agent-tab editor | L8 | M2.1 | M | P-SPA1, WS10-S5a (c) | - | - |
| WS10-S6 | addressee routing, member decide in Session Core | L2 | M2.3 | L | WS10-S1, WS10-S3 (c), WS01-S3 (c) | - | WS10-S7, WS10-S8 |
| WS10-S7 | turn_origin rules | L2 | M2.3 | S | WS10-S6, WS01-S3 (c), WS08-S2 (c) | - | - |
| WS10-S8 | quarantine approval kind, hold and release | L3 | M2.3 | L | WS10-S1, WS10-S3, WS10-S6 (c) | - | - |
| WS10-S9 | resumable-kind registry and parity test | L3 | M2.3 | M | WS10-S1, WS10-S3 | - | WS18-S9 |
| WS11-S1 | Control bulk read decisions | L4 | M2.2 | S | P-UC1 | - | WS11-S2, WS11-S3 |
| WS11-S2 | Session Core SpaceAttention RPC | L2 | M2.2 | L | WS15-S5, WS11-S1 (c) | - | WS11-S3, WS11-S7 |
| WS11-S3 | Convex read-state extensions, BFF aggregate | L6 | M2.2 | L | P-CX1, WS11-S1 (c), WS11-S2 (c) | - | WS11-S4, WS11-S6, WS11-S10, WS16-S10 |
| WS11-S4 | SPA store, sidebar counts, mark all read | L9 | M2.2 | M | P-SPA1, WS11-S3 (c) | - | WS16-S10 |
| WS11-S5 | Session Core attention outbox, NATS stream | L2 | M2.2 | M | WS02-S1 | - | WS11-S6 |
| WS11-S6 | Convex ingest, rules, items, follows | L6 | M2.2 | L | WS11-S3, P-CX1, WS11-S5 (c) | WS08-S2 | WS11-S7, WS11-S8, WS11-S9, WS11-S10, WS11-S11, WS19-S10 |
| WS11-S7 | items API, /attention page, follow control | L9 | M2.2 | L | P-SPA1, WS11-S2 (c), WS11-S6 (c) | - | WS11-S11 |
| WS11-S8 | invite signal from gateway add-member | L7 | M2.2 | S | P-GW1, WS11-S6 (c) | - | - |
| WS11-S9 | notification-core principal, allowlist, courier | L6 | M2.2 | M | WS11-S6, P-CX1 | - | - |
| WS11-S10 | org status text and agent availability chips | L6 | M2.2 | M | WS11-S3, WS11-S6, P-CX1 | - | - |
| WS11-S11 | reminders | L6 | M2.2 | M | WS11-S6, P-CX1, WS11-S7 (c) | - | - |
| WS12-S1 | Control DM kinds, participant_hash, eligibility | L4 | M2.2 | M | P-UC1 | - | WS12-S2, WS12-S3 |
| WS12-S2 | Application DM open, hide, legacy guards | L6 | M2.2 | M | P-CX1, WS12-S1 (c) | - | WS12-S3 |
| WS12-S3 | gateway DM routes, SPA sidebar group, start dialog | L7 | M2.2 | L | P-GW1, WS12-S1 (c), WS12-S2 (c), WS01-S5 (c), WS01-S6 (c), WS01-S7b (c) | WS04-S3, WS04-S9 | - |
| WS12-S4 | Session Core revisions, fold, ReviseMessage | L1 | M2.2 | L | WS01-S1 | - | WS12-S5, WS12-S6, WS12-S8, WS13-S11 |
| WS12-S5 | Control revise decision, message policies | L4 | M2.2 | M | P-UC1, WS12-S4 (c) | WS03-S1 | WS12-S6, WS12-S7 |
| WS12-S6 | model-gateway PATCH/DELETE/target routes | L1 | M2.2 | M | WS12-S4, WS12-S5 (c) | - | WS12-S7 |
| WS12-S7 | gateway lifecycle routes, SPA toolbar, edit, delete | L8 | M2.2 | L | WS01-S7a, P-SPA2, P-SPA1, WS12-S5 (c), WS12-S6 (c), WS01-S7b (c) | - | WS12-S8 |
| WS12-S8 | reactions (Convex, routes, chips) | L6 | M2.2 | M | P-CX1, WS12-S7 (c), WS12-S4 (c), WS01-S5 (c) | - | - |
| WS13-S1 | Session Core SpaceHistoryService, FTS index | L1 | M2.3 | L | P-SC1 | WS04-S11 | WS13-S2, WS13-S9, WS13-S10 |
| WS13-S2 | model-gateway search routes and client | L1 | M2.3 | S/M | WS13-S1 | - | WS13-S3, WS13-S9 |
| WS13-S3 | BFF search routes, entries, features | L7 | M2.3 | M | P-GW1, WS13-S2 (c) | WS01-S5 | WS13-S4b, WS13-S5, WS13-S9 |
| WS13-S4a | SPA entry links, entity router, highlight | L8 | M2.3 | S | WS01-S7a, P-SPA1 | - | WS13-S4b, WS13-S5 |
| WS13-S4b | /m/:messageId permalink route | L8 | M2.3 | S | WS13-S4a, P-SPA1, WS13-S3 (c) | - | - |
| WS13-S5 | in-room search UI | L8 | M2.3 | M | WS13-S4a, P-SPA1, WS13-S3 (c) | - | - |
| WS13-S6 | command layer: registry, palette, switcher | L9 | M2.3 | M | P-SPA1 | - | WS13-S7, WS13-S9 |
| WS13-S7 | platform slash commands in composer | L8 | M2.3 | S | WS13-S6, P-SPA2, P-SPA1 | WS13-S5 | - |
| WS13-S8 | room pins (Convex, routes) | L6 | M2.3 | M | P-CX1 | WS04-S4 | - |
| WS13-S9 [cond] | global search scope (decision D-5) | L4 | M3 | M | WS13-S1, WS13-S2, WS13-S3, WS13-S6, P-UC1 | - | - |
| WS13-S10 [cond] | fork with visible-prefix copy (needs mp ForkThread) | L1 | M3 | L | WS13-S1, WS01-S3, WS01-S4, WS01-S5 (c) | - | - |
| WS13-S11 [cond] | quote with server-resolved tombstone | L1 | M3 | M | WS01-S3, WS01-S4, WS12-S4 | - | - |
| WS14-S1 | Control shared import decision | L4 | M2.3 | M | P-UC1 | - | WS14-S3, WS14-S6, WS14-S7 |
| WS14-S2 | Control grants: owner-asserted, Space subject | L4 | M2.3 | L | P-UC1 | - | WS14-S3, WS14-S4, WS14-S5 |
| WS14-S3 | documents-api space upload | L10 | M2.3 | L | WS14-S1 (c), WS14-S2 (c) | - | - |
| WS14-S4 | sharing un-fence (documents-api, gateway) | L10 | M2.3 | M | WS14-S2 (c) | - | - |
| WS14-S5 | retrieval-engine listing provenance, grants union | L10 | M2.3 | M | WS14-S2 (c) | - | WS14-S7 |
| WS14-S6 | imports-core space-upload | L10 | M2.3 | M | WS14-S1 (c) | - | WS14-S7 |
| WS14-S7 | gateway space_files.rs R1-R6 | L7 | M2.3 | M | P-GW1, WS14-S1 (c), WS14-S5 (c), WS14-S6 (c) | - | WS14-S9 |
| WS14-S8 | SPA per-turn attachments, chips, typed ZDR copy | L8 | M2.3 | M | P-SPA2, P-SPA1 | - | - |
| WS14-S9 | SPA Files tab, preview, delete | L8 | M2.3 | L | P-SPA1, WS14-S7 (c) | - | - |
| WS15-S1 | client timeline: truncation fix, anchoring, jump pill | L8 | M2.3 | S | P-SPA1 | - | WS15-S8 |
| WS15-S2 | per-room per-thread drafts | L8 | M2.3 | S | P-SPA2, P-SPA1 | - | - |
| WS15-S3 | markdown toolbar, preview, counter | L8 | M2.3 | M | P-SPA2, P-SPA1 | WS01-S6 | - |
| WS15-S4 | human posts rendered as markdown | L8 | M2.3 | S | WS07-S5, P-SPA1 | - | - |
| WS15-S5 | Session Core thread activity summary | L1 | M2.3 | M | P-SC1 | - | WS11-S2, WS15-S6 |
| WS15-S6 | ListSpaceTimeline, tail transcript | L1 | M2.3 | L | WS15-S5, WS01-S1 (c) | - | WS15-S7, WS15-S9 |
| WS15-S7 | BFF timeline routes, legacy fallback | L7 | M2.3 | M | P-GW1, WS15-S6 (c) | - | WS15-S8 |
| WS15-S8 | SPA timeline store, paged UI, lazy transcripts | L8 | M2.3 | L | WS15-S1, P-SPA1, WS15-S7 (c) | - | WS15-S11, WS15-S13 |
| WS15-S9 | typed entry plumbing and registry | L1 | M2.3 | M | WS01-S1, WS15-S6 | - | WS15-S10, WS15-S11, WS18-S7 |
| WS15-S10 [cond] | link previews (needs Quarry preview mode) | L7 | M3 | M | WS07-S5, P-GW1, WS15-S9 (c), WS01-S5 (c) | - | - |
| WS15-S11 | room system rows from Control ledger | L7 | M2.3 | M | P-GW1, WS15-S8 (c), WS15-S9 (c), WS04-S4 (c) | - | WS15-S12 |
| WS15-S12 | Application room notices | L6 | M2.3 | M | P-CX1, WS15-S11 (c) | WS04-S2 | - |
| WS15-S13 [cond] | windowed list if budget fails (conditional) | L8 | M3 | M | WS15-S8, P-SPA1 | - | - |
| WS16-S1 | locale-correct Space copy and dates | L9 | M2.3 | M | M0-BASE | - | WS16-S3, WS16-S4 |
| WS16-S2 | Control preference contract v2 | L4 | M2.3 | S | P-UC1 | - | - |
| WS16-S3 | preferences bootstrap, locale, density | L9 | M2.3 | M | WS16-S1, P-SPA1 | WS16-S2 | WS16-S5 |
| WS16-S4 | shared announcer, Space narration | L9 | M2.3 | M | WS16-S1, P-SPA1 | - | - |
| WS16-S5 | forced-colors, contrast, density CSS | L9 | M2.3 | S | WS16-S3, P-SPA1 | - | - |
| WS16-S6 | viewer locale hint on Space turns | L7 | M2.3 | S | P-GW1 | - | - |
| WS16-S7 | Convex spaceSidebarPreferences | L6 | M2.3 | M | P-CX1 | - | WS16-S8, WS16-S11 |
| WS16-S8 | BFF sidebar routes, ZDR split | L7 | M2.3 | M | P-GW1, WS16-S7 (c), WS01-S5 (c) | - | WS16-S9 |
| WS16-S9 | organiser core, grouped panel, stars, drag | L9 | M2.3 | L | P-SPA1, WS16-S8 (c) | - | WS16-S10 |
| WS16-S10 | computed sections, mute entry, more-unread pill | L9 | M2.3 | M | WS16-S9, P-SPA1, WS11-S3 (c), WS11-S4 (c) | - | - |
| WS16-S11 | orphan sweep cron | L6 | M2.3 | S | WS16-S7, P-CX1 | WS04-S1 | - |
| WS17-S1 | memory containment (space_kind claim, memory_scope.rs) | L1 | M1 | M | P-SC1 | - | WS17-S2, WS17-S3, WS17-S9 |
| WS17-S2 | withheld legacy rows visible to owner, panel badge | L1 | M1 | S | WS17-S1 | - | - |
| WS17-S3 | audience labels, resolver parity, letta-bridge | L1 | M3.1 | L | WS17-S1 | - | WS17-S7, WS17-S8 |
| WS17-S4 | Control agent registry, access list, admission, cascade | L4 | M3.1 | L | P-UC1, WS04-S4 (c) | WS03-S2 | WS17-S5 |
| WS17-S5 | personal agents in product (Convex, routes, SPA) | L6 | M3.1 | L | P-CX1, WS17-S4 (c), WS01-S5 (c) | - | WS17-S6 |
| WS17-S6 | private panel, reviewed share-to-room | L8 | M3.1 | M | WS17-S5, WS01-S6, WS01-S7a, P-SPA1, WS01-S5 (c) | - | - |
| WS17-S7 | declassify decision, DeclassifyMemory, review UI | L1 | M3.1 | L | WS17-S3, WS01-S5 (c) | WS03-S1 | WS17-S8 |
| WS17-S8 | per-turn personal-memory opt-in | L1 | M3.1 | S | WS17-S3, WS17-S7 | - | - |
| WS17-S9 | retrieval guard and audience claim | L4 | M3.1 | M | WS17-S1, P-UC1 | WS14-S5 | - |
| WS18-S1 | registry golden, sync test, Control registry lookup | L3 | M3.1 | M | M0-BASE | - | WS18-S4a, WS18-S2, WS18-S3, WS19-S9 |
| WS18-S4a | execution-core decode tolerance for new response fields (first step of S4) | L3 | M3.1 | S | WS18-S1 | - | WS18-S2, WS18-S4 |
| WS18-S2 | Control view v2, Capability Core v2 verifier | L3 | M3.1 | M | WS18-S1, WS18-S4a (c) | - | WS18-S4 |
| WS18-S3 | Session Core run-bound owner path, receipts | L2 | M3.1 | L | WS18-S1 | - | WS18-S4, WS18-S7 |
| WS18-S4 | execution-core adapter, tool_profile, attestation | L3 | M3.1 | L | WS18-S4a, WS18-S2, WS18-S3 | - | WS18-S5, WS18-S8, WS18-S9, WS18-S10 |
| WS18-S5 | binding switch, BFF routes, profile mapping | L7 | M3.1 | M | P-GW1, WS18-S4 (c) | - | WS18-S6, WS18-S10 |
| WS18-S6 | SPA registry view, toggle, provenance chips | L8 | M3.1 | S/M | P-SPA1, WS18-S5 (c) | - | WS18-S7 |
| WS18-S7 | space.thread.post_card | L2 | M3.1 | M | WS18-S3, WS01-S1, WS18-S6 (c), WS15-S9 (c) | - | - |
| WS18-S8 [cond] | space.agent.pause_self (conditional) | L4 | M3 | M/L | WS18-S4, P-UC1, WS08-S2 (c) | - | - |
| WS18-S9 [cond] | space.schedule.follow_up (epic until deps land) | L4 | M3 | L | WS18-S4, WS10-S9, P-UC1 | - | - |
| WS18-S10 | proof script, evidence, ledger | L3 | M3.1 | S | WS18-S4, WS18-S5 (c) | - | - |
| WS19-S1 | Data repo: canvas migration, CAS save, restore, purge | L10 | M3.1 | M | M0-BASE | - | WS19-S3, WS19-S8 |
| WS19-S2 | Control canvas decision, entitlement | L4 | M3.1 | M | P-UC1 | WS03-S1 | WS19-S3 |
| WS19-S3 | wiki-store HTTP: verifier, routes, ZDR guards | L10 | M3.1 | M | WS19-S1, WS19-S2 (c) | - | WS19-S4, WS19-S8, WS19-S9, WS19-S10 |
| WS19-S4 | gateway canvas module and routes | L7 | M3.1 | M | P-GW1, WS19-S3 (c) | - | WS19-S5, WS19-S9 |
| WS19-S5 | SPA Canvas tab, read-only view | L8 | M3.1 | S | P-SPA1, WS19-S4 (c) | - | WS19-S6 |
| WS19-S6 | SPA create, edit, save, conflict banner | L8 | M3.1 | M | WS19-S5, P-SPA1 | - | WS19-S7 |
| WS19-S7 | SPA history, diff, restore | L8 | M3.1 | M | WS19-S6, P-SPA1 | - | - |
| WS19-S8 | canvas purge and erasure legs | L10 | M3.1 | S | WS19-S1, WS19-S3 | - | - |
| WS19-S9 | agent canvas tools | L3 | M3.1 | L | WS19-S3 (c), WS19-S4 (c), WS18-S1 (c) | WS10-S5a, WS17-S1 | - |
| WS19-S10 [cond] | content-free pointer (deferred) | L10 | M3 | M | WS19-S3, WS02-S3 (c), WS11-S6 (c) | - | - |
| WS20-S1 | Control browser-run decision, entitlement | L4 | M3.2 | S | P-UC1 | WS03-S1 | WS20-S2 |
| WS20-S2 | broker Space binding, ephemeral profile, caps | L3 | M3.2 | M | WS20-S1 (c) | - | WS20-S3 |
| WS20-S3 | server-resolved Space browser path | L3 | M3.2 | L | WS20-S2, WS01-S1, WS09-S1 | - | WS20-S4, WS20-S5, WS20-S6, WS20-S7 |
| WS20-S4 | room-safe read: Session Core RPCs, routes | L2 | M3.2 | L | WS20-S3, WS09-S4 | - | WS20-S5, WS20-S7 |
| WS20-S5 | start panel, browser card, evidence view | L8 | M3.2 | L | WS01-S7a, P-SPA1, WS20-S3 (c), WS20-S4 (c), WS01-S5 (c) | - | WS20-S8 |
| WS20-S6 | Quarry delegation, room-view relay, control lease | L3 | M3.2 | L | WS20-S3 | - | WS20-S7, WS20-S8, WS20-S9 |
| WS20-S7 | challenge to hand-off card (HUMAN_ASSIST) | L2 | M3.2 | M | WS20-S3, WS20-S4, WS20-S6 | WS10-S1 | - |
| WS20-S8 | interactive view, take control, give back | L8 | M3.2 | M | WS20-S5, WS20-S6, P-SPA1 | - | WS20-S9 |
| WS20-S9 | manager takeover and stop, teardown | L4 | M3.2 | L | WS20-S6, WS20-S8, WS09-S7, WS09-S8, P-UC1 | - | - |

### 4.2.1 External gates (not slices; nothing in this plan builds them)

| Id | Gate | Owner | Needed by | State on 2026-10-20 |
|---|---|---|---|---|
| E1 | `model-plane` base: Z-series, retention receipt, mp-WS03 S1-S8A | (b) | `M0-BASE` | built, local, unmerged |
| E2 | mp-WS04.S6 send receipts: `AppendMessageRequest.client_message_id = 18`, `AppendMessageResponse.message_id = 2` and `deduplicated = 3`, migration slot `message_client_id` (0052 by (b)'s plan order, integrator may renumber) | (b) | WS01-S1, WS01-S8 | NOT built (no `client_message_id` in `sessions.proto` on `model-plane`). If (b) has not landed it when WS01-S1 is ready, Space lands only the session-core half (migration, dedupe in `append_message_in_tx`, proto) under (b)'s name and number; the unique key includes the author (05 X02). WS01-S1's 1.0-1.5 ew already contains a dedupe, so effort is unchanged. |
| E3 | mp-WS01.S2 outbox fix deployed | (b) | WS02 live enablement, WS11-S5 (G3) | build and test dark until it deploys |
| E4 | `ForkThread` RPC | (b) mp-WS17.S6 | WS13-S10 [cond] | not built |
| E5 | Lift of the Space-scope refusal on `/continue` (commit `78a3d169`, DEC-06) | (b) | in-place resume in rooms | no Space slice waits; WS10 stays action-only |
| E6 | Broker grants F-t2-001 (qb-WS1.A2 needs Control registry row X-1), takeover enforcement default | (a) | WS20-S2 real store, WS20-S3 live proof | A0/A1/B built; A2 not |
| E7 | Quarry room-view routes and `delegation` (F-t2-013 phase 1) and the masked-or-raw decision (SD-23) | (a), nobody owns it today (05 X16) | WS20-S6 sign-off, S8, S9 | not built |
| E8 | Quarry `/v1/preview` and `scan_upload` | (a), unscheduled | WS15-S10 [cond], WS14 scan hook | not scheduled |
| E9 | One Control PR train for principals, `nats.conf`, compose (05 X29) | integrator | `P-CFG`, WS02-S3, WS04, WS09-S7, WS10, WS11, WS18, WS20 | open |
| E10 | Operator facts SD-27: second dev member, deterministic dev model, Convex re-push, Novu workflows (WS11-S9) | ops | every live proof | `M0-T0` covers the seed; the rest open |

### 4.2.2 Critical path

Computed on the firm slices with H and (c) edges counted as serial (strict) and with (c) edges overlapping by half (overlap):

| Basis | Length (ew) | Path |
|---|---|---|
| strict, low / mid / high | 15.6 / 19.1 / 22.5 | M0-BASE > P-UC1 > WS20-S1 > WS20-S2 > WS20-S3 > WS20-S4 > WS20-S5 > WS20-S8 > WS20-S9 |
| overlap, low / mid / high | 13.9 / 17.1 / 20.1 | same path |

At 0.8 productivity this is about 24 calendar weeks at mid with unlimited engineers (4.7.3). WS20 is the longest chain by a margin and is also the most externally gated (E6, E7); WS20-S2..S3 may run against a test-double broker, S6/S8/S9 cannot be signed off without (a).

Near-critical chains (mid, strict), each ending in a different workstream:

| ew | Chain | Reading |
|---|---|---|
| 16.4 | M0-BASE > P-SC1 > WS01-S1 > S2 > S3 > S4 > WS08-S3 > S4 > S6 > S8 | addressed wake rides the member-post spine |
| 15.2 | spine > WS01-S5 > S6 > S7b > WS12-S7 > S8 | DMs and lifecycle UI wait for the post composer |
| 14.2 | M0-BASE > WS05-S6 > S7 > S8 > S9 | SSO and SCIM; independent lane (L5), 16.5 ew high |
| 14.2 | P-SC1 > WS15-S5 > WS11-S2 > S3 > S6 > S7 > S11 | attention waits for the thread activity summary (decision 9 of `02` 2.3.5) |
| 13.8 | P-UC1 > WS10-S2 > S3 > S6 > S8 | approvals: grants, gate, member decide, quarantine |
| 13.7 | spine > WS01-S5 > WS17-S5 > S6 | personal agents UI |
| 13.2 | spine > WS01-S5 > WS16-S8 > S9 > S10 | sidebar organiser |
| 11.7 | M0-BASE > WS18-S1 > S3 > S4 > S5 > S6 > S7 | self-API |
| 11.4 | P-SC1 > WS01-S1 > WS15-S6 > S7 > S8 > S11 > S12 | scalable timeline and system rows |
| 10.6 | P-UC1 > WS14-S2 > S5 > S7 > S9 | files |

Reading of the graph:

1. **The plan is capacity-bound, not path-bound.** Total firm effort is 309.5 ew at mid against a 19.1 ew critical path (ratio about 16), so no realistic team (3 to 8) is limited by the path; staffing, skill mix and lane order set the calendar (4.7.3).
2. **The member-post spine WS01-S1 > S2 > S3 > S4 > S5 (about 8 ew strict, 6.5 with overlap, after P-SC1) sits on five of the ten chains** and gates WS08, WS12, WS13-S10/S11, WS16-S8, WS17-S5/S6 and WS20-S5. Shorten it first: publish the `sessions.proto` tags and the thread-append v2 golden in the first two days of WS01-S1 so that WS01-S2 and the gateway and SPA halves start against contracts, and start WS01-S7a on day one (it has no dependency).
3. **Independent chains that can start in week 1 with no coordination**: WS05 (S1, S2, S3, then S6), WS03-S1/S2, WS07-S1/S4/S5/S7/S13, WS16-S1/S2, WS19-S1, WS14-S1, WS14-S8, WS13-S6, WS18-S1, WS06-S3/S5/S7.
4. **Prep slices are on the path only by their own length**: `P-UC1` (0.5-0.8 ew) heads the WS20 chain and `P-SC1` (0.8-1.2 ew) heads the spine; both are 1 to 2 days of pure moves.

## 4.3 Parallel lanes

A lane is one git worktree on a branch `space/<lane>` cut from `space`, owned by one engineer (or one pair), with a file-ownership rule. Lanes follow planes because the hotspots are plane files; features cross lanes through published contracts. The slice order inside each lane is the recommended priority order (it matches the schedule simulation of 4.7.3, adjusted by hand for the M1 gate). Effort is firm ew low-high.

### 4.3.1 Lane table

| Lane | Worktree (suggested) and owner plane | Owns (edits allowed without a seam) | Slices in order | Firm ew | Cross-lane waits |
|---|---|---|---|---|---|
| **L0 PLAT** | `space-platform`, integrator (repo) | `docs/plans/evidence/**`, `.github/workflows/space-gate.yml`, `convex-core.yml`, `scripts/space-e2e-seed.sh`, ledger entry in `apps/Model Plane/docs/decisions/ledger.md`, regenerated `go/gen/**`, Convex `_generated/**` | M0-BASE > M0-T0 > M0-CI; then per train: ledger append, `buf generate`, Convex restart | 1.8-3.0 | none |
| **L1 SCT** | `space-sc-transcript`, Model Plane (session-core messages, threads, memory; model-gateway message routes) | `session-core/src/space/{append,history,room_read,timeline,revisions,memory_scope}.rs`, migrations 0071, 0073, 0075-0078, 0081, 0083, 0085, model-gateway `space_routes/{posts,search,timeline,revisions,artifacts,memory}.rs` | P-SC1 > WS01-S1 > WS17-S1 > WS01-S3 > WS01-S4 > WS07-S9 > WS17-S2 > WS08-S2 > WS08-S3 > WS12-S4 > WS12-S6 > WS15-S5 > WS13-S1 > WS13-S2 > WS15-S6 > WS15-S9 > WS17-S3 > WS17-S7 > WS17-S8 [cond: WS13-S10, WS13-S11] | 29.1-42.6 | WS01-S3 waits for WS01-S2 contract (L4); WS08-S2 waits WS08-S1 vectors (L4) and WS09-S1 (L2); WS12-S6 waits WS12-S5 (L4); WS15-S6 waits WS01-S1 fields; WS17-S7 waits WS17-S3 |
| **L2 SCR** | `space-sc-runs`, Model Plane (session-core runs, approvals, events, attention; model-gateway run and stream routes) | `session-core/src/space/{events,run_attach,run_control,approvals,attention,self_api,browser_read}.rs`, migrations 0072, 0074, 0079, 0080, 0082, 0084, model-gateway `space_routes/{runs,approvals,attention,browser}.rs` and `space_sse.rs` | WS09-S1 > WS06-S1 > WS06-S2 > WS06-S4 > WS07-S9a > WS02-S1 > WS02-S3 > WS09-S3 > WS09-S4 > WS10-S1 > WS09-S8 > WS08-S7 > WS11-S5 > WS11-S2 > WS10-S6 > WS10-S7 > WS18-S3 > WS18-S7 > WS20-S4 > WS20-S7 [cond: WS02-S6, WS02-S7, WS06-S12, WS09-S9] | 29.3-44.0 | WS02-S3 waits WS02-S2 (L4); WS09-S8 waits WS09-S7 (L4); WS11-S2 waits WS15-S5 (L1) and WS11-S1 (L4); WS10-S6 waits WS10-S3 (L3) and WS01-S3 (L1); WS20-S4 waits WS20-S3 (L3) |
| **L3 RUN** | `space-agent-runtime`, Model Plane (execution-core, capability-core, sandbox-manager, browser-broker, letta-bridge, mp-contracts registry) | `execution-core/src/space_{gate,self_api,canvas_tools,browser}.rs`, `capability-core` rows, `sandbox-manager` interceptors, broker Space binding, sandbox-manager 0008, capability-core 0027 | WS06-S3 > WS06-S7 > WS18-S1 > WS10-S3 > WS06-S8 > WS06-S9 > WS06-S6 > WS06-S10 > WS10-S9 > WS10-S8 > WS18-S4a > WS18-S2 > WS18-S4 > WS18-S10 > WS19-S9 > WS20-S2 > WS20-S3 > WS20-S6 [cond: WS06-S6b, WS06-S11] | 27.7-43.7 | WS10-S3 waits WS10-S1 (L2) and WS10-S2 (L4); WS06-S6/S9 wait WS06-S5 (L4); WS18-S2 waits WS18-S4a; WS18-S4 waits WS18-S3 (L2); WS19-S9 waits WS19-S3/S4 and WS18-S1; WS20-S3 waits WS01-S1 (L1), WS09-S1 (L2); WS20-S6 waits E7 |
| **L4 CTL** | `space-control-spaces`, Control (user-core; split L4a decisions/entitlements and L4b membership/registry when two engineers) | `user-core/internal/spaces/*_decision.go`, `entitlement_catalog.go`, migrations 033-048, route files per feature behind `P-UC1` | P-UC1 > WS03-S1 > WS03-S2 > WS03-S3 > WS04-S4 > WS01-S2 > WS02-S2 > WS06-S5 > WS04-S3 > WS04-S7 > P-CFG > WS08-S1 > WS09-S7 > WS10-S2 > WS10-S5a > WS11-S1 > WS04-S10 > WS12-S1 > WS04-S8 > WS04-S11 > WS12-S5 > WS14-S1 > WS14-S2 > WS05-S5 > WS16-S2 > WS17-S4 > WS19-S2 > WS17-S9 > WS20-S1 > WS20-S9 [cond: WS13-S9, WS18-S8, WS18-S9]. L4a: 03, 01-S2, 02-S2, 06-S5, 08-S1, 09-S7, 10-S2, 10-S5a, 11-S1, 12-S5, 14-S1, 16-S2, 19-S2, 20-S1, 20-S9, 17-S9. L4b: 04-S3/S4/S7/S8/S10/S11, 12-S1, 14-S2, 17-S4, 05-S5 | 36.5-54.8 | WS01-S2 needs the WS01-S1 proto; WS04-S8 waits contracts of WS04-S5/S6 (L6, L7); WS12-S5 waits the WS12-S4 proto (L1); WS20-S9 waits WS20-S6 and WS20-S8 |
| **L5 IDN** | `space-identity`, Control (org-core, audit-core, auth-core, integration-corev2) | `audit-core/**`, `org-core/**`, `auth-core/src/auth/**` (new files), `integration-corev2/internal/{scim,auth}`, audit-core 011, org-core 028-029, auth-core 029, integration-corev2 0014 | WS05-S1 > WS05-S3 > WS05-S2 > WS05-S6 > WS05-S4 > WS05-S7 > WS05-S8 > WS05-S9 | 14.1-21.7 | WS05-S2 lands after the integration-corev2 owner commits its dirty hunks; WS05-S8/S9 wait WS04-S10 (L4) |
| **L6 APP** | `space-application`, Application (convex-core, notification-core) | `convex/schema/<feature>.ts`, new `convex/<feature>.ts` modules, `crons.ts` (WS04-S1 first), notification-core 020 | P-CX1 > WS04-S1 > WS04-S2 > WS04-S5 > WS08-S5 > WS16-S7 > WS12-S2 > WS11-S3 > WS11-S6 > WS11-S9 > WS11-S10 > WS12-S8 > WS11-S11 > WS04-S12 > WS13-S8 > WS16-S11 > WS15-S12 > WS17-S5 | 21.6-30.7 | WS04-S5 waits the WS04-S4 audience route (L4); WS11-S6 waits WS11-S5 (L2); WS12-S8 waits WS12-S7 (L8); WS17-S5 waits WS17-S4 (L4) |
| **L7 GW** | `space-gateway`, Frontend Plane (Axum gateway/BFF) | `gateway/src/domains/spaces/<feature>.rs` (one file per feature), `config.rs` flag fields, `domains/actions/{handlers,dispatchers}.rs` (append-only) | P-GW1 > WS03-S4 > WS03-S5 > WS01-S5 > WS02-S4 > WS04-S6 > WS07-S3 > WS07-S6 > WS08-S4 > WS10-S4 > WS04-S9 > WS11-S8 > WS12-S3 > WS13-S3 > WS14-S7 > WS15-S7 > WS16-S6 > WS16-S8 > WS15-S11 > WS19-S4 > WS18-S5 [cond: WS15-S10] | 21.1-32.9 | every slice builds against the contract of its Control or Model provider (c) | 
| **L8 SPA-R** | `space-spa-room`, Frontend Plane (SPA room surfaces; split L8a composer and shell, L8b timeline and panes when two engineers) | `features/spaces/components/{SpacePage,SpaceRoomComposer,SpaceRoomTimeline,SpaceCockpit}.tsx`, `features/spaces/components/aux/**`, `features/spaces/lib/**` | WS01-S7a > P-SPA1 > P-SPA2 > WS07-S4 > WS01-S6 > WS01-S8 > WS07-S11 > WS07-S12 > WS01-S7b > WS02-S5 > WS09-S2 > WS08-S6 > WS09-S5 > WS09-S6 > WS08-S8 > WS10-S5b > WS12-S7 > WS13-S4a > WS15-S1 > WS14-S8 > WS15-S2 > WS15-S3 > WS15-S4 > WS13-S5 > WS13-S4b > WS13-S7 > WS15-S8 > WS14-S9 > WS19-S5 > WS19-S6 > WS19-S7 > WS18-S6 > WS17-S6 > WS20-S5 > WS20-S8 [cond: WS15-S13]. L8a: 01-S6/S8/S7b, 08-S6/S8, 09-S2, 14-S8, 15-S2/S3, 13-S7, 10-S5b. L8b: 07-S4/S11/S12, 02-S5, 09-S5/S6, 12-S7, 13-S4a/S4b/S5, 15-S1/S4/S8, 14-S9, 19-S5..S7, 18-S6, 17-S6, 20-S5/S8 | 39.3-58.4 | each UI slice waits the gateway route of its feature (c); WS01-S6 waits P-SPA2 and the owner's disposition of the 2-line composer change |
| **L9 SPA-S** | `space-spa-shell`, Frontend Plane (settings, sidebar, palette, Home, shared libs) | `features/settings/**` (own component per WS), `features/core/components/sidebar/**`, `shared/artifacts/**`, `shared/i18n/**`, `shared/lib/**`, `nginx.conf`, `Dockerfile` | WS07-S1 > WS07-S5 > WS07-S13 > WS16-S1 > WS07-S2 > WS07-S10 > WS07-S7 > WS07-S8 > WS03-S7 > WS03-S6 > WS11-S4 > WS11-S7 > WS13-S6 > WS16-S3 > WS16-S4 > WS16-S5 > WS16-S9 > WS16-S10 | 16.9-25.5 | WS03-S6/S7 wait WS03-S4/S5 (L7) and the WS03-S2 golden (L4); WS11-S4/S7 wait WS11-S3/S6 (L6); WS16-S10 waits WS11-S3/S4 |
| **L10 DAT** | `space-data`, Data Plane and imports-core (wiki-store, documents-api, retrieval-engine-rs, imports-core) | `wiki-store-go/**`, `documents-api-go/**`, `retrieval-engine-rs/src/space_*`, `imports-core/**`, Data migration timestamps, imports-core 005 | WS19-S1 > WS14-S6 > WS14-S5 > WS14-S3 > WS14-S4 > WS19-S3 > WS19-S8 [cond: WS19-S10] | 10.0-14.5 | WS14-S3..S6 wait the WS14-S1/S2 contracts (L4); WS14-S5 waits the retrieval-engine-rs residue (4.1.2); WS19-S3 waits the WS19-S2 golden (L4) |

### 4.3.2 Lane rules

1. A lane edits only the paths it owns. Any other path is touched through the seam of 4.4 (one registration line) or through a contract PR to the owning lane first.
2. A slice that spans planes (WS04-S2, WS04-S3, WS04-S6, WS09-S4, WS09-S8, WS10-S1, WS10-S4, WS14-S7, WS20-S4, WS20-S5 and others the designs call "spans planes") is delivered as one PR per lane, ids suffixed `-a`, `-b`, merged contract-first; effort splits as the design's file list does. The designs keep them as one slice only because the flow must be proven end to end; the DoD is run once all legs are merged.
3. Contracts are published in the first PR of a chain, before the implementation: the artifact table below.
4. A lane never merges a PR that turns a flag on; flag changes are L0 rollout PRs (4.9).
5. A lane rebases daily on `space`, and on any train the integrator merges from `model-plane` or `quarryv2-next` (4.5).

### 4.3.3 Contract artifacts that unlock a lane

| Artifact | Published by | Unlocks | Form |
|---|---|---|---|
| `sessions.proto` additions with regenerated Go bindings | WS01-S1 (first), then each proto slice | all lanes' stubs | merged proto + `go/gen` in one PR from the integrator |
| thread-append v2 golden digest (Go and Rust) | WS01-S2 | WS01-S3, WS12-S5, WS08-S1 vectors | JSON golden under `apps/Control Plane/user-core/internal/spaces/testdata/` and the Rust copy, sha-pinned |
| model-gateway post route contract | WS01-S4 | WS01-S5 | wiremock fixtures in `tests/` |
| `space_context.features` and `limits` TS type | WS01-S5 | every SPA slice | `src/shared/api/spaces-client/` types |
| stream frame golden | WS02-S3 | WS02-S4, WS02-S5 | JSON golden plus `ROOM_EVENT_POLICY` table |
| denial codes golden `space-denial-codes.golden.json` | WS03-S2 | WS03-S5, WS03-S7 and every later issuer | sha-pinned golden |
| `model.run.wake` decision vectors | WS08-S1 | WS08-S2, WS08-S4 | golden digest |
| `ApprovalScope` and grant records (ADR-0011) | SD-15 ledger entry, then WS10-S2 | WS10-S3, WS10-S4, WS10-S6 | ADR text plus Control JSON schema |
| audience-candidates route | WS04-S4 | WS04-S5, WS04-S6 | OpenAPI fragment |
| import decision and grant routes | WS14-S1, WS14-S2 | WS14-S3..S7 | OpenAPI fragments plus golden |
| attention RPC | WS11-S2 | WS11-S3 | proto |
| registry golden | WS18-S1 | WS18-S2..S4, WS19-S9 | shared crate and JSON |
| canvas decision golden | WS19-S2 | WS19-S3 | sha-pinned golden |
| browser-run decision golden | WS20-S1 | WS20-S2, WS20-S3 | sha-pinned golden |

## 4.4 Conflict matrix and prep refactors

### 4.4.1 Hotspot files, who edits them, and the rule that prevents conflicts

"Slices" are the editors named by the designs' Conflict-hotspot and File-level sections. "Siblings" = changes already on `model-plane` (b) or `quarryv2-next` (a) against `afb83928`, measured on 2026-10-20. A rule is binding on every lane; the seam it relies on is a prep slice in 4.4.2.

| # | Hotspot (lines in live tree) | Space slices that edit it | Siblings | Rule |
|---|---|---|---|---|
| 1 | gateway `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/spaces.rs` (9,882) | WS01-S5, WS02-S4, WS03-S5, WS04-S2/S3/S6/S7/S8/S9, WS08-S4, WS09-S4, WS10-S4, WS11-S3/S8, WS12-S3/S7, WS13-S3, WS14-S7, WS15-S7, WS16-S8, WS17-S5, WS18-S5, WS19-S4, WS20-S4 (about 28) | (b) +10 | `P-GW1` renames it to `domains/spaces/mod.rs` (`git mv`, no content change) and exposes merge points. Each feature adds `spaces/<feature>.rs` with `pub fn routes()`, one `mod` line and one `.merge()` line. Legacy handlers (six membership URL sites, `public_space`) are edited only by WS03-S5 (denial mapping) then WS04-S6, S7, S8 in that order. Merge conflicts on the `mod`/`.merge` lines are mechanical: keep both. |
| 2 | gateway `config.rs` (593) with five test `AppState` literals | every GW slice that adds a flag | (a) +2, (b) touches (both) | `P-GW1` adds `SpaceFeatureFlags` (`Default`) and `AppState::for_tests()`, so a new field never edits the literals. Flags read once into `AppState` with `env_bool(key, dev_flags_allowed(APP_ENV))`; never `std::env::var` in a handler (a handler read raced under parallel tests, memory note). Fields are appended alphabetically. |
| 3 | gateway `envelope.rs` (`error_with_details`), `zdr.rs` (`org_zdr_lookup`) | WS01-S5 (designed first lander), WS03-S4, WS04, WS19-S4, WS20 | none | `P-GW1` creates both with the final signatures; consumers only call them. |
| 4 | SPA `src/shared/actions/action-registry.ts` (3,151); gateway `domains/actions/{handlers,dispatchers}.rs` | about 25 slices (every slice with a meaningful operation) | (b) +64, (a) +92 | `P-SPA1` moves Space entries into `src/shared/actions/domains/spaces/<ws>.ts` fragments joined by one import line; gateway dispatchers into `dispatchers_spaces/<ws>.rs`. The action contract lands before the UI wiring (repo rule) and `node --test apps/gateway/scripts/action-surface-contract.test.mjs` runs in every PR. On rebase keep both sides, sorted. |
| 5 | SPA `SpacePage.tsx` (1,447) | WS01-S6/S7a/S7b, WS02-S5, WS04-S6/S7/S8, WS07-S11/S12, WS08-S8, WS09-S6, WS10-S5b, WS12-S7, WS13-S4a/S5, WS14-S9, WS15-S8, WS17-S6, WS19-S5, WS20-S5 | none | WS01-S7a lands first and creates the pane host; `P-SPA1` adds the tab and pane registry. After that a slice adds one registration line plus its own component files. Lane L8 only. |
| 6 | SPA `SpaceRoomComposer.tsx` (800; live tree dirty +2 lines) and its test | WS01-S6, WS08-S6, WS09-S2, WS13-S7, WS14-S8, WS15-S2/S3, WS04-S7 (read-only prop), WS17-S6 | none | Owner disposes of the 2-line change (4.1.2); `P-SPA2` extracts mode registry, chip slot, toolbar slot and the send pipeline hook; then strictly serial in L8a: P-SPA2 > WS01-S6 > WS15-S2 > WS08-S6 > WS09-S2 > WS14-S8 > WS15-S3 > WS13-S7. |
| 7 | SPA `SpaceRoomTimeline.tsx` (486) | WS01-S6/S7a/S7b, WS07-S4/S11, WS09-S5, WS12-S7, WS15-S1/S4/S8, WS20-S5 | none | WS01-S7a extracts `SpaceTurnList`; WS15-S9 creates `entry-registry.ts` for typed rows; between them rows plug into `SpaceTurnList` slots. Serial in L8b: WS07-S4 > WS07-S11 > WS09-S5 > WS12-S7 > WS15-S1 > WS15-S4 > WS15-S8 > WS20-S5. |
| 8 | SPA `SpaceCockpit.tsx` tabs | WS07-S12 (Artifacts), WS14-S9 (Files), WS19-S5 (Canvas), WS04 (Members exists) | none | `P-SPA1` tab registry; a tab is one registration line. |
| 9 | SPA `spaces-client.ts` (1,028), `shared/i18n/errors.ts` (199), `shared/api/http.ts`, `chat-client.ts` | almost every SP and GW slice | (a) and (b) both edit `errors.ts` and `http.ts`; (b) owns `chat-client.ts` | `P-SPA1` splits `spaces-client/<feature>.ts` behind a barrel; `errors.ts` entries go under a `// WSnn` banner with sorted keys. WS01-S8 and WS16-S6 touch `chat-client.ts` only for the idempotency, `session_key` and locale lines, after (b)'s mp-WS04.S6 lands or in agreement with (b). |
| 10 | SPA `features/settings/components/WorkspaceSettingsPage.tsx`, `settings-sections.ts` | WS03-S6, WS05-S1/S4/S6/S7/S8, WS16-S3 | none | each in its own component file; the page gets one registration line (L9 owns the page). |
| 11 | SPA `CoreSidebarSpacesPanel.tsx`, `sidebar-navigation.ts`, `CoreSidebar.tsx` | WS01-S7b (link builder, one line), WS16-S9/S10, WS11-S4, WS12-S3 | none | order WS01-S7b > WS16-S9 > WS11-S4 > WS12-S3 > WS16-S10; L9 owns. |
| 12 | session-core `src/grpc.rs` (13,341) | WS01-S1/S3, WS02-S1 (hooks), WS04-S11, WS06-S1/S2, WS07-S9 (registration), WS08-S2, WS09-S1/S4/S8, WS10-S1/S6, WS11-S2/S5, WS12-S4, WS13-S1, WS15-S5/S6/S9, WS17-S1/S3/S7, WS18-S3, WS20-S4 | (b) +1,727 (ConsumeApproval, loop checkpoints, run context) | `P-SC1` creates `src/space/`; new logic goes in `src/space/<feature>.rs`. Edits to `grpc.rs` are limited to (a) `append_message_in_tx` (order: WS01-S1 > WS01-S3 > WS12-S4 > WS15-S9 > WS08-S2), (b) service registration lines, (c) ZDR refusal typing (WS06-S1/S2 first). L1 and L2 never edit the same function (L1 owns message and thread tables, L2 owns runs, approvals, events). |
| 13 | session-core `src/auth.rs` (2,095), `src/main.rs`, `mp-contracts/build.rs` | WS01-S3 (`authorize_append`), WS06-S1/S2, WS09-S4, WS10-S6, WS12-S4, WS13-S1, WS20-S4 | (b) +396 on `auth.rs` | Space authorization functions live in `src/space/authz.rs` (`P-SC1`); `auth.rs` only gets WS06-S1/S2 refusal typing (first) and the WS01-S3 seam. `main.rs` and `build.rs` get one line per service or proto. |
| 14 | `sessions.proto` (1,249), `runs.proto`, `orchestration.proto`, `execution.proto` | WS01, WS02, WS07, WS08, WS09, WS10, WS11, WS12, WS13, WS15, WS17, WS18 | (b) +237 `sessions.proto`, +18 `execution.proto`, +31 `orchestration.proto` | Tags exactly as 4.6.C. New services go in new files `space_events.proto`, `space_history.proto`, `space_room.proto`, `space_attention.proto` (integrator to confirm; avoids interleaving with (b)'s train). A proto-bearing PR carries the regenerated Go bindings (`go/gen/model_plane/v1`, Docker `buf generate`, L0 only); proto PRs merge strictly one at a time. |
| 15 | session-core, user-core, org-core, audit-core, notification-core, capability-core, sandbox-manager migration directories | all migration slices | (b) 0046-0066, (a) 0067-0070 on session-core | Numbers only from 4.6; one migration per PR; a migration merges before the code that reads its table, and its feature flag flips later; never renumber after merge; `revert/*.down.sql` in the same PR; migration up/down gate X6 of `06`. |
| 16 | model-gateway `http_routes.rs` (11,681), `sse.rs` (13,325), `session_flow.rs`, `state.rs`, `zdr_posture.rs`, `zdr_precheck.rs` | WS01-S4, WS06-S4, WS07-S9/S9a, WS08-S3, WS09-S3/S4/S8, WS10-S1, WS11-S2, WS12-S6, WS13-S2, WS15-S6, WS16-S6, WS18-S5/S7, WS20-S3/S4 | (b) `http_routes.rs` +303, `sse.rs` **+6,305**; (a) edits `http_routes.rs`, `state.rs`, `session_flow.rs`, `browser_run.rs`, `zdr_precheck.rs` | `P-SC1` adds `space_routes/` (one file per feature, one `.merge()` line) and `space_sse.rs`. **Space edits `sse.rs` and `sse_events.rs` never**; Space adds no `ChatEvent` variant (room projection is default-deny through `ROOM_EVENT_POLICY`), so `sse-taxonomy.yml` is untouched. `DURABLE_MUTATIONS` in `zdr_posture.rs` is append-only. |
| 17 | execution-core `runtime_loop/{mod,agent,closers,effect,fold,park,resume}.rs`, `grpc.rs`, `executor.rs`, `sandbox.rs` | WS06-S9, WS10-S3/S8/S9, WS18-S4, WS19-S9, WS20-S3 | (b) the whole loop (mp-WS03 S1-S8A) | Space code in new files (`space_gate.rs`, `space_self_api.rs`, `space_canvas_tools.rs`, `space_browser.rs`); edits to `mod.rs` and `agent.rs` are registration lines only, after rebasing on (b)'s latest train. One lane (L3). Space adds no resumable kind to the cold worker (SD-21). |
| 18 | user-core `internal/http/server.go` (route block `:343-381`), `service_auth.go` (scope switch `:92-163`), `internal/spaces/repository.go` (`UpsertEffectPolicy`), `internal/http/spaces.go`, `internal/spaces/authority.go` | about 30 L4 slices | none | `P-UC1` splits route registration into `routes_<feature>.go` and the scope table into `scopes_spaces.go`. WS03-S1 lands first (catalogue-driven `UpsertEffectPolicy`); each entitlement is then one catalogue row plus its migration. Decisions live in new `internal/spaces/<name>_decision.go`. `authority.go` order: WS03-S2 > WS04-S4 > WS04-S7 > WS17-S4. L4 is serial by construction (L4a and L4b split by file, not by function). |
| 19 | Control `docker-compose.yml:426` (credential JSON), `config/plane-service-principals.json`, `cross-plane-token-requests.json` (+ sha256), `apps/Model Plane/deploy/nats.conf`, `.env.example`s | WS02-S3, WS04-S4, WS09-S7, WS10-S2, WS11-S5/S9, WS18-S2, WS20-S2/S6 | (a) and (b) both | Control PR train `P-CFG`: one weekly batch, rows added alphabetically, never reordered; each PR names its scope in the PR text and the integrator batches; `scripts/tests/principal-registry-parity.test.sh` and the golden sha re-pin run in the train PR (05 X29, O16). |
| 20 | Convex `schema.ts` (1,073), `spaces.ts` (1,555), `crons.ts`, `http.ts`, `_generated/*`, `nats-subscriber.js` | WS04-S1/S2/S5, WS07-S7, WS08-S5, WS11-S3/S6/S10/S11, WS12-S2/S8, WS13-S8, WS15-S12, WS16-S7/S11, WS17-S5, WS18-S8, WS19-S10 | none | `P-CX1` moves tables into `convex/schema/<feature>.ts` composed by `schema.ts`; new functions go in new `convex/<feature>.ts` modules; `crons.ts` gets WS04-S1 first (WS16-S11 needs it); `_generated/*` is never hand-merged: take either side and regenerate (L0). After a merge: `docker restart convex-gateway`, wait for "Convex functions ready!" (never `pnpm run deploy`, which targets production). |
| 21 | `scripts/tests/zdr-contract-sync.test.sh`, `.github/workflows/{contract-goldens,buf,quarry-v2-ci}.yml`, `docs/compliance/evidence/*` | WS06, WS02, every new durable RPC row (O18) | (a) and (b) both edit all four | append rows only; `M0-CI` quotes the paths in `buf.yml` once. |
| 22 | frontend `nginx.conf`, `Dockerfile`, compose | WS07-S2 (first), WS02 (SSE no-buffering), WS14 (download headers), WS07-S6 (CSP) | none | order WS07-S2 > WS02-S4 > WS14-S7 > WS07-S6; nginx snippets, not the main file. |
| 23 | `tests/e2e/space-fixtures.ts`, `playwright.config.ts`, `local-auth-b.setup.ts` | `M0-T0` creates; every WS adds its own spec | none | each WS adds `tests/e2e/space-<feature>.spec.ts` and never edits another spec; `E2E_REQUIRE_SPACE=1` at milestone gates. |
| 24 | `apps/Model Plane/docs/decisions/ledger.md` | `M0-BASE`, then one row per built number | (b) owns the file | one append-only entry per train by L0; no slice edits it. |

### 4.4.2 Prep refactors (new slices)

All are pure moves or seams: behaviour and test counts must not change, `git diff -w --stat` shows moved lines, and the suites named in 4.5.4 are green against the stored baseline. They reduce 24 hotspots to one-line registration conflicts.

| Id | What | Size | ew | Lane | Needed before | Acceptance |
|---|---|---|---|---|---|---|
| `P-SC1` | session-core `src/space/` (`mod.rs`, `authz.rs`), `append_message_in_tx_as(tx, req, author, AppendAs{Owner,Member})` with the old function as an `Owner` wrapper (05 X01), empty `SpaceRoomReadService` registration (D3), model-gateway `space_routes/mod.rs` and `space_sse.rs` stubs, service registration lines in `main.rs` | S/M | 0.8-1.2 | L1 | WS01-S1, WS02-S1, WS07-S9, WS13-S1, WS15-S5, WS17-S1 | `cargo test -p session-core -p model-gateway` unchanged; SQL of `append_message_in_tx` byte-identical; DB tests (`--ignored`) green |
| `P-GW1` | gateway `domains/spaces/` tree (`git mv`), `SpaceFeatureFlags` + `AppState::for_tests()`, `envelope::error_with_details`, `zdr::org_zdr_lookup`, router merge seam | S | 0.6-1.0 | L7 | every GW slice | `cargo test --manifest-path apps/gateway/Cargo.toml --all-targets`, clippy `-D warnings`, the 5 literals compile unchanged |
| `P-UC1` | user-core `routes_<feature>.go` registration and `scopes_spaces.go` scope table | S | 0.5-0.8 | L4 | all L4 slices except WS03-S1, WS03-S2, WS06-S5 | `go test ./...` in user-core; route table golden unchanged |
| `P-CX1` | Convex `convex/schema/<feature>.ts` split composed by `schema.ts` (Convex codegen accepts imported table definitions: UNVERIFIED, spike first) | S | 0.4-0.6 | L6 | all L6 slices except WS04-S1, WS04-S2 | `pnpm test` in convex-core; `docker restart convex-gateway` shows "Convex functions ready!" |
| `P-SPA1` | SPA tab and pane registry, `spaces-client/` split, action-registry fragments, `errors.ts` banners; lands after WS01-S7a | M | 1.0-1.5 | L8 | all SP slices except WS01-S7a, WS07-S1, S5, S13, WS16-S1 | `pnpm typecheck && pnpm lint && pnpm test` against baseline; `action-surface-contract.test.mjs` |
| `P-SPA2` | composer extension slots and send pipeline hook (after the owner disposes of the live 2-line change) | M | 1.0-1.5 | L8 | WS01-S6, WS08-S6, WS09-S2, WS13-S7, WS14-S8, WS15-S2/S3, WS12-S7 | `SpaceRoomComposer.test.tsx` unchanged and green |
| `P-CFG` | Control PR train mechanism (6 batches over M1-M3) | S | 1.2-1.8 | L4 | any new principal, scope, ACL | parity test, golden re-pin |
| `M0-CI` | `space-gate.yml`, `convex-core.yml`, quoted `buf.yml` paths (not a seam but a gate) | S | 0.5-1.0 | L0 | merge gates of 4.5.4 | the workflows go red on a seeded failure |

Prep total 5.5-8.4 ew (about 2 percent of firm effort), of which 0.4 ew is net new (P-SC1 carves 0.4 ew out of WS01-S1, WS07-S9, WS15-S6; the same holds for the flag-struct work moved from WS01-S5 into `P-GW1`).

## 4.5 Merge order, rebase rules and CI gates

### 4.5.1 Branches

- `space` = long-lived integration branch, cut in `M0-BASE` from the then-current `model-plane` tip (it must contain `22090c3b` and the Z-series; record the SHA in the ledger entry). Never force-pushed. Space PRs target `space`. If SD-11 prefers waiting for `model-plane` to reach `main`, the same flow runs on a branch cut from `main` after that merge; nothing below changes.
- Lane worktrees `space/<lane>` branch from `space`; slice branches follow the repo's Conventional Commit types (`feat/ws01-s7a-thread-pane`, `fix/ws05-s1-audit-read-role-gate`).
- Upstream sync: the integrator merges `model-plane` into `space` weekly (merge commit, never a rebase of merged history), and `quarryv2-next` every second week or when a Space slice needs a file (a) changed (`browser_run.rs`, gateway `browser.rs` for WS20). When either sibling reaches `main`, `space` merges `main`. The 26 files both siblings changed will conflict at that point regardless of Space; the rules of 4.4 keep Space's share to one-line registrations.
- Stacked PRs: do not delete a merged base branch before retargeting its child (deleting a base auto-closes the child PR; memory note). Use `gh pr edit --base`.

### 4.5.2 Merge trains (coarse order; inside a train lanes merge in parallel unless a row says otherwise)

| Train | Content | Entry condition |
|---|---|---|
| T0 | `M0-BASE` ledger; `P-SC1`, `P-GW1`, `P-UC1`, `P-CX1` (parallel pure moves); `M0-CI`, `M0-T0` | SD-11, SD-01 answered |
| T1 | independent and unblocking: WS05-S1, S3, S2; WS03-S1, S2; WS01-S7a then `P-SPA1`, `P-SPA2`; WS07-S1, S5, S7, S13, WS16-S1; WS06-S1, S2, S3, S4, S7; WS09-S1 (0 migrations); WS01-S1 (0071 + proto, alone in the proto slot) | T0 merged (the first three PRs of 4.8 may merge before) |
| T2 | G2 train: WS01-S2, WS17-S1 (0073, proto claim), WS02-S1 (0072), WS02-S2, WS03-S3, WS04-S4, WS06-S5, then WS01-S3 and WS02-S3 after G2, WS01-S4, WS07-S2, WS07-S9a | G2 closed |
| T3 | WS01-S5, WS02-S4, WS03-S4/S5, WS04-S1/S2/S3/S5, WS07-S3, WS07-S9 (0085), then SPA: WS01-S6, S8, WS02-S5, WS03-S6/S7, WS04-S6, S7, WS07-S4, S6, S8, S10-S12, WS01-S7b, WS17-S2; `P-CFG` batches 1-2 | M1 exit = T1..T3 + 06 M1 scenarios |
| T4 (M2.1) | WS08-S1, S5; WS09-S3, S7; WS10-S1, S2; then WS08-S2 (0075), WS10-S3 (0074), WS09-S4, WS08-S3, WS10-S4, S5a; WS08-S4, WS09-S5; SPA: WS09-S2, WS08-S6, WS09-S6, WS08-S8, WS10-S5b; WS09-S8, WS08-S7 | G4 for SD-00, SD-05, SD-20, SD-21, SD-26 |
| T5 (M2.2) | WS11-S1; WS15-S5 (0078, pulled forward because WS11-S2 depends on it); WS11-S2; WS12-S1, WS12-S4 (0076); WS04-S10, S8, S11 (034-037 already in order); WS12-S2, S5, S6; WS11-S3, S5 (0080); WS11-S6, S9, S10, S11; WS12-S3, S7, S8 | G4 for SD-13, SD-14, SD-17, SD-19; E3 for WS11-S5 live |
| T6 (M2.3) | WS05-S4..S9 (own lane, any time after T3); WS06-S8, S9, S6, S10; WS10-S6, S7, S9, S8 (0082); WS13-S1 (0077), S2, S3, S4a, S4b, S5, S6, S7, S8; WS14-S1..S9; WS15-S1..S9, S11, S12; WS16-S1..S11 | G5 |
| T7 (M3.1) | WS17-S3 (0081), S4, S5, S6, S7 (0083), S8, S9; WS18-S1..S7, S4a, S10 (0084, capability-core 0027); WS19-S1..S9 | G6 |
| T8 (M3.2) | WS20-S1..S9 (user-core 047-048) | E6, E7 for S6, S8, S9 |
| Rolling | conditional slices, `WS04-S12`, flag flips (4.9) | per decision |

### 4.5.3 Ordering rules that hold in every train

| # | Rule |
|---|---|
| R1 | Provider before consumer along every edge of 4.2; a consumer's flag stays dark until its provider is deployed. |
| R2 | One proto-bearing PR at a time, in tag order of 4.6.C, with regenerated bindings; one migration per PR; a migration merges before the code that reads its table. |
| R3 | `P-*` seams merge before any slice touching their files; WS03-S1 merges before any entitlement-column migration (user-core 038 onward). |
| R4 | WS09-S1 is ON in every environment before `SPACE_HUMAN_POSTS_CORE` is ON anywhere; WS17-S1 is ON before (b) enables `GATEWAY_CHAT_PROJECTS_ENABLED`; WS05-S1 deploys before (b) replays audit rows (mp-WS01.S2, O13). |
| R5 | WS04-S4 deploys dark before WS04-S6's flag is on anywhere; WS04-S5 is on before any member added by WS04-S6, WS04-S8 or a DM can read or post through a shared decision; WS04-S3 or S9 before the DM picker. |
| R6 | WS14-S4: re-point the actions first, then flip `CONTROL_OWNER_ASSERTED_GRANTS`, never the reverse. |
| R7 | WS15-S5 before WS11-S2; WS11-S2 before WS11-S3; WS02-S1 before WS11-S5; WS11-S5 before WS11-S6. |
| R8 | WS10-S1 before S3; S3 before S6; S6 before S8. WS10-S5a and WS17-S1 are on before WS19-S9 is default-on. |
| R9 | Deploy order inside a feature: Control decision, then Session Core or Data, then model-gateway, then gateway, then SPA; every flag off at each step; Control images through `scripts/run-control-plane.sh` (bare compose build exits 0 and builds nothing). |
| R10 | Control principals, `nats.conf` and compose edits only through the weekly `P-CFG` train (E9). |
| R11 | `WS05-S2` merges only after the integration-corev2 owner has committed the dirty `internal/auth/middleware.go` hunks, or rebases on them. |

### 4.5.4 Rebase rules

1. Rebase the slice branch on `space` at least daily and immediately before merge; the lane branch likewise. No merge commits inside slice branches.
2. Generated files are never hand-merged: `go/gen/**`, `*.pb.*`, Convex `_generated/**`, `Cargo.lock`, `pnpm-lock.yaml`. Take either side, then regenerate (L0 for proto and Convex; the author for lockfiles).
3. Append-only registries (`action-registry.ts` fragments, `errors.ts`, `zdr-contract-sync.test.sh` rows, `DURABLE_MUTATIONS`, principal JSON, `nats.conf`): keep both sides, sort, re-run the registry's parity test.
4. A migration or proto-tag collision found at rebase is resolved by taking the next number from the spare range of 4.6 and updating the ledger row in the same PR; a number that has merged or deployed is never changed.
5. After a `model-plane` sync, re-run the session-core and model-gateway suites before the next Space merge; a failing sibling test is reported to its owner, not patched in Space.
6. Re-check every line number a PR cites; the designs cite live-tree lines that drift.

### 4.5.5 CI gates a merge must pass (and what only a flag flip needs)

Origin has only `dev` and `fix/ui-premium-restore`, and the Control, gateway and SPA workflows trigger only for PRs based on `main`/`develop` (`build-test-gates.md` findings 1 and 2), so `M0-CI` adds a base-agnostic, path-filtered `space-gate.yml` that runs the rows below. Pre-existing failures are compared as sets against the stored baseline (new failure = fail); they are not fixed in Space PRs.

| Touched area | Gate (run from repo root) |
|---|---|
| user-core, audit-core, org-core | `cd "apps/Control Plane/<svc>" && go vet ./... && go test ./...` (user-core live tests need Postgres 16); principal or token change: `bash scripts/tests/principal-registry-parity.test.sh` |
| auth-core | `cd "apps/Control Plane/auth-core" && pnpm install --frozen-lockfile && REQUIRE_CONTRACT=1 pnpm test`; the full suite has 15 pre-existing failures in 3 suites: diff the failing set against HEAD |
| Model Plane Rust | `cd "apps/Model Plane/rust" && cargo fmt --all -- --check && cargo check --workspace --all-targets && cargo test --workspace --no-fail-fast`; DB tests: `cargo test -p session-core -- --ignored` on a scratch Postgres whose name contains "test"; two cargo targets fail pre-existing on Windows (use `--lib` locally) |
| Model Plane Go | `cd "apps/Model Plane/go/services/<svc>" && go test ./...` (new dependency: `GOWORK=off`); no CI today, `space-gate.yml` adds one job per touched service |
| Proto and contracts | `buf lint` and `buf breaking` in `apps/Model Plane/proto` against `baseline.binpb`; `REQUIRE_CONTRACT=1 bash scripts/tests/zdr-contract-sync.test.sh`; every new durable RPC adds a row to the ZDR inventory and posture matrix in the same PR (O18) |
| Gateway | `cd "apps/Frontend Plane/verevonv3" && cargo fmt --manifest-path apps/gateway/Cargo.toml -- --check && cargo clippy --manifest-path apps/gateway/Cargo.toml --all-targets -- -D warnings && cargo test --manifest-path apps/gateway/Cargo.toml --all-targets && node --test apps/gateway/scripts/action-surface-contract.test.mjs` |
| SPA | `cd "apps/Frontend Plane/verevonv3" && pnpm lint && pnpm typecheck && pnpm test && pnpm build`, failing sets against baseline; Playwright at milestone gates only: `pnpm exec playwright test tests/e2e/<spec>.spec.ts` on a second Vite server (not `:5173`) with `E2E_REQUIRE_SPACE=1` |
| Convex | `cd "apps/Application Plane/convex-core" && pnpm test` (no CI today; `convex-core.yml` adds it) |
| notification-core, imports-core, Data services | `go test ./...` per service; Data Postgres contracts `bash "apps/Data Plane v2/tests/scripts/run-postgres-contract-tests.sh"` with `REQUIRE_CONTRACT=1`; retrieval-engine-rs `cargo test` |
| Always | `secret-scan` (gitleaks 8.24.3; `.gitleaks.toml` uses the singular `[allowlist]`), Conventional Commit title, evidence of "no new failing test" |
| Flag flip (not a merge gate) | the slice's DoD flow on images rebuilt from the commit under test, flag on then off, evidence bundle per `06` 6.4; a person who did not write the slice runs it at each milestone gate |

## 4.6 Migration, proto and flag reservation table

This table is the Space input to the single ledger entry (E9, `05` question 1). It takes every number the siblings built or reserved as taken, and extends `05` 5.1 with the facts re-verified on 2026-10-20. Gaps are safe (sqlx applies unapplied versions; user-core applies pending files in lexicographic order); numbers are identities, not an order: merge order follows 4.5.

### 4.6.A session-core migrations (`apps/Model Plane/rust/services/session-core/migrations`)

| Number | Owner | Name | Slice | Status |
|---|---|---|---|---|
| 0044, 0045 | (b) | `run_retention_receipt`, `zdr_waived_runs_org_index` | n/a | built on `model-plane`; untracked in the live tree |
| 0046, 0047, 0048 | (b) | `approval_consume_and_mode`, `run_loop_checkpoints`, `tool_action_effect_ledger` | n/a | built (verified at `d2ea86a1`; no migration since) |
| 0049-0066 (+ optional `thread_memory_mode`) | (b) | in plan order: 0049 `thread_activity_cursor`, 0050 `thread_autonomy_grant`, 0051 `user_memory_state`, 0052 `message_client_id`, 0053 `attention_state`, 0054 `run_questions`, 0055 `message_search_index`, 0056 `cron_fire_run_link`, 0057 `org_learning_policy_mirror`, 0058 `agent_skills_proposals`, 0059 `thread_fork_lineage`, 0060 `thread_star_unread`, 0061 `message_versions`, 0062 `approval_scope_grants`, 0063 `run_question_consume`, 0064 `skill_revisions`, 0065 `cron_schedule_contract`, 0066 `drop_hook_configs` | n/a | reserved, renumberable by the integrator until its train; the per-name numbers are derived from (b)'s plan order and are UNVERIFIED against its ledger |
| 0067-0070 | (a) | `run_trace_events`, `run_external_content`, epic E4.6, epic E4.2 | n/a | proposed re-take (their R-4 numbers 0046-0049 collide with (b)); (a) has built none |
| **0071** | Space | `message_post_kind` | WS01-S1 | reserved; `message_send_dedupe` and `client_message_key` dropped, consume (b)'s `client_message_id` (E2) |
| **0072** | Space | `space_event_log` | WS02-S1 | reserved |
| **0073** | Space | `thread_space_kind` | WS17-S1 | reserved; first, because (b)'s `GATEWAY_CHAT_PROJECTS_ENABLED` waits on it |
| **0074** | Space | `approval_gate_context` | WS10-S3 | reserved; columns must not repeat built `consumed_at`, `continuation_mode` |
| **0075** | Space | `space_mentions_and_wake_origin` | WS08-S2 | reserved |
| **0076** | Space | `message_revisions` | WS12-S4 | reserved |
| **0077** | Space | `space_message_search_index` (`-- no-transaction`, partial index `space_id IS NOT NULL`) | WS13-S1 | reserved |
| **0078** | Space | `thread_activity_summary` | WS15-S5 | reserved; pulled early (R7) |
| **0079** | Space | `messages_space_activity_index` | WS11-S2 | **conditional**: dropped if WS15-S5's summary serves (`02` 2.3.5 #9) |
| **0080** | Space | `space_attention_outbox` (widens the outbox subject CHECK; one editor, X31) | WS11-S5 | reserved |
| **0081** | Space | `agent_memory_audience_labels` (carries BOTH `(scope_kind, scope_id)` and `(origin_kind, space_ref, audience_hash, audience_revision)`, C12) | WS17-S3 | reserved |
| **0082** | Space | `quarantined_outputs` | WS10-S8 | reserved |
| **0083** | Space | `memory_declassifications` | WS17-S7 | reserved |
| **0084** | Space | `run_thread_actions` | WS18-S3 | reserved |
| **0085** | Space | `messages_space_artifact_index` (plain `CREATE INDEX`, quiet window) | WS07-S9 | **new in this section** (`05` 5.1.A left it unnumbered) |
| 0086-0090 | Space | spare | n/a | free |
| dropped | Space | WS13 `thread_fork_lineage` (consume (b)'s) | WS13-S10 | not built |

Next free after every reservation: **0091**.

### 4.6.B Other services

| Service | Highest today | Taken by siblings | Space final |
|---|---|---|---|
| user-core `apps/Control Plane/user-core/migrations` | 029 | (b) 030 `retire_ai_memory_enabled`; (b)'s 031 `space_effect_policy_read_actions` and 032 `space_browser_entitlement` are DROPPED (N8, X34); (a) none | **033** WS03-S1 `space_effect_policy_events`; **034** WS04-S4 `space_membership_source_and_events`; **035** WS04-S7 `space_archived_state`; **036** WS04-S8 `space_visibility_directory`; **037** WS04-S11 `space_history_mode`; **038** WS09-S7 `run_control_entitled`; **039** WS10-S2 `approval_grants`; **040** WS10-S5a `space_tool_policies`; **041** WS12-S1 `dm_kinds`; **042** WS12-S5 `message_lifecycle`; **043** WS14-S2 `resource_grants_space_subject`; **044** WS17-S4 `agent_subjects_and_admissions`; **045** WS17-S7 `space_memory_entitlements`; **046** WS19-S2 `canvas_effect_policy`; **047** WS20-S1 `browser_effect_policy`; **048** WS20-S9 `browser_control_effect_policy`; 049-052 spare. Next free **053**. |
| org-core | 020 | (b) 021-023 (`org_model_entitlements`, `siem_destinations`, `org_browser_sites`); (a) moves to 024-027 | **028** WS05-S6 `org_identity_domains`; **029** WS05-S8 `org_scim` |
| audit-core `internal/store/migrations` | 006 | (b) 007-010 | **011** WS05-S4 `audit_read_indexes` |
| notification-core | 014 | (b) 015-017; (a) 018-019 | **020** WS11-S9 |
| capability-core | 0015 | (b) 0016-0026 | **0027** WS18-S2 `space_self_api_capabilities` |
| sandbox-manager | 0004 | (b) 0005-0007 | **0008** WS06-S6b `lease_retention_class` [cond] |
| auth-core | 028 | none | **029** WS05-S7 `sso_enforcement` |
| integration-corev2 | 0013 | none | **0014** WS05-S2 |
| imports-core | 004 | none | **005** WS14-S6 `space_upload_idempotency` |
| Data Plane v2 (14-digit timestamps, top `20261003150000`) | n/a | none | WS19-S1 wiki-store and WS14-S3 documents-api: take `date +%Y%m%d%H%M%S` at merge (the designs' `20261020100000` and `20261020120000` are placeholders) |
| cost-core, billing-core, conversation-core | n/a | (b) cost-core 0011 | none (Space has no cost slice, X17) |
| Convex | schema only | none | no numbering; hot file rules in 4.4 row 20 |

### 4.6.C Proto field numbers (add-only; never renumber; no `reserved` ranges)

| Message.field | Taken (do not use) | Space final | Slice |
|---|---|---|---|
| `AppendMessageRequest` | built 1-17; (b) `client_message_id = 18` (E2) | `post_kind = 19`, `mentions = 20`, `entry = 21`; `client_message_key` dropped | WS01-S1, WS08-S2, WS15-S9 |
| `AppendMessageResponse` | built `sequence = 1`; (b) `message_id = 2`, `deduplicated = 3` | `mentions = 4` | WS08-S2 |
| `SessionMessage` | built 1-6; (b) 7-10 | `post_kind = 11`, `sequence = 12` (drop if message-id paging suffices), `mentions = 13`, `entry = 14`, `revision_state = 15`, `revised_at = 16`, `revised_by_subject_id = 17`, `revision_no = 18`, `agent_followed_up = 19` | WS01-S1, WS15-S6, WS08-S2, WS15-S9, WS12-S4 |
| `ListConversationRequest` | built 1-5; (b) `limit = 6`, `before_message_id = 7`, `include_inactive = 8`; response `has_more = 2` | `exclude_human_posts = 9`, `post_context_cap = 10`; response `omitted_before = 3` | WS01-S1, WS08-S2, WS15-S6 |
| `ThreadSummary` | built 1-14; (b) 15 `archived_at`, 16-18 attention, 19 `forked_from_thread_id`, 20-22 `starred`, `unread`, `last_read_at` | `reply_count = 23`, `last_author_subject_id = 24`, `last_message_seq = 25`, `recent_author_subject_ids = 26`, `last_role = 27`, `turn_count = 28` | WS01-S1, WS15-S5/S6 |
| `StartManagedRunRequest` | built 1-9 | `wake = 10` | WS08-S2 |
| `RunDetail` (runs.proto) | built through `park_reason = 18` | `owner_subject_id = 19` | WS09-S4 |
| `RunAgentRequest` (execution.proto) | built `run_context = 18`, `resume = 19`; (b) 20-24 | `tool_profile = 25` | WS18-S4 |
| `RunAgentResponse` | built through `park_reason = 11` (new since `05`) | none | n/a |
| `Approval` | built 1-15; (b) 16 `cause`, 17 decision record | `tool_key = 18`, `action_label = 19`, `addressee = 20`, `requester_subject_id = 21`, `grant_ref = 22`, `turn_origin = 23`, `continuation` projection `= 24` | WS10-S1, S3, S6 |
| `CreateApprovalRequest` | built 1-12; (b) `cause = 13` | `context_json = 14`, `addressee = 15`, `tool_key = 16`, `action_digest = 17`, `turn_origin = 18`, `grant_use_token = 19` | WS10-S1, S3, S6 |
| `DecideApprovalRequest` | built 1-5; (b) 6-7 | `space_id = 8`, `space_decision_ref = 9`, `space_decision_token = 10`, `grant_use_token = 11` | WS10-S6 |
| `ConsumeApprovalRequest` | built through `omit_call = 7` (new since `05`) | none | n/a |
| `ApprovalKind` | built 1-5 | `HUMAN_ASSIST = 6`, the one coordinated kind with (b) mp-WS21.S9 | WS20-S7 |
| `OrchestrationEvent` oneof | live 10-23; (b) 24-29, of which 24 `run_parked` and 25 `run_resumed` are now built | `BrowserHandoffRequested = 30`, `BrowserHandoffResolved = 31` | WS20-S7 |
| `events.proto` `Event` and `EventType` | (a) fields 14-18, types 150-160; (b) fields 21+, types 161+ | EventType block 180-189 (WS20 uses 180, 181); no `Event` field; `reserved 200 to 255` untouched | WS20-S7 |
| `browser.proto` | built and (a)/(b) reservations | none (no Space field in any broker proto, C1, X15) | n/a |
| `memory.proto` | built `MemoryEntry` 1-8, `SearchMemoryRequest` 1-7; (b) `IndexMemoryRequest.origin = 7` etc. | `MemoryEntry` 9, 10; `SearchMemoryRequest` 8, 9 (free) | WS17-S3 |
| `sandboxes.proto` | (b) mp-WS22 reservations UNVERIFIED | `AcquireLeaseRequest` field 7 per the WS06 design [cond] | WS06-S6b |
| New RPCs in `sessions.proto`, `runs.proto` | (b) appended after `CountWaivedRuns`, now also `ListToolActions` | new Space services in `space_*.proto` files (4.4 row 14); RPCs appended in merge order | WS02, WS07, WS09, WS11, WS12, WS13, WS15, WS18 |

### 4.6.D Subjects, flags, decisions, ADR ids

| Namespace | Taken | Space |
|---|---|---|
| NATS subjects | (b) `mp.v1.runctl.*`, `mp.v1.security.*`; (a) `mp.v1.trace.run.*`; existing `mp.v1.run.*.event` | `mp.v1.space.<space>.event` only (WS02-S7, WS11-S5); `deploy/nats.conf` through `P-CFG` |
| Flags | the (a) and (b) lists of `05` 5.1.D (all default off) | every Space flag is `SPACE_*`, `space_*`, `MEMORY_*` or plane-prefixed, no collision; a Space slice never uses a sibling flag as its own kill switch; sibling flags that must be on first are listed in 4.9 |
| Decision ids | (b) DEC-01..40; (a) ADR-1..10, X-1..20, R-1..32 | `SD-00..SD-29`, `D-WSnn-x`, `PD-WSnn`, no reuse |
| ADR files | (b) ADR-0005..0013 placeholders | Space ADR-0014..0023 (`SP-ADR-1..10`); ADR-0011 (approval scope) and ADR-0012 (run origin) are joint and must exist before any Space proto or migration that depends on them |
| Control decision actions | Space `model.space.browser_run`, `data.canvas.read`, `data.canvas.write`, `model.run.wake`, `thread:append`, `schedule_create`, `run-action-v1`; (b) `model.thread.sandbox_workspace` (deferred) | no collision; Control owns every name |
| Tables | (b) `run_loop_checkpoints`, `tool_action_effect_ledger`, `run_questions`, `approval_scope_grants`, `message_versions`; (a) `run_trace_events` | no collision; Space keeps `quarantined_outputs` |

## 4.7 Effort roll-up

### 4.7.1 Assumptions

1. **ew** = one engineer-week of focused work by an engineer who knows the plane, as the designers estimated it (tests, live proof and review included). Low-high are the designers' ranges summed, not a statistical interval; mid = (low + high) / 2.
2. **Firm scope** = 197 slices: 187 design slices plus 10 baseline and prep rows. Conditional slices (14: 27.0-42.5 ew) are listed in 4.1.1 and excluded. Not sized and not included: WS08-S9..S11, WS11-S12, WS14 E1 and E2, the epics E01-E12, audit items outside WS01-WS20, the Support Plane. `WS06-S0` (1.5-3 ew in its design) is dropped (D5).
3. **Planning figure** = mid plus 15 percent for integration, rebases on two moving sibling branches and live-proof environment contention: 309.5 x 1.15 = **356 ew**. The high column (371.7 ew) is the pessimistic bound, not a probability.
4. **Calendar model** (script, list scheduling): one slice per engineer at a time; priority = milestone, then longest remaining path; a (c) edge lets the consumer start when its provider is half done and finish no earlier than the provider; duration in weeks = ew / 0.8 (review, rebase, queueing for live proof, sibling coordination); no holidays; decisions answered at their gates; external gates E1-E10 assumed available except where 4.1.1 says otherwise (WS20-S6/S8/S9 are not delayed for (a) in the numbers, so M3.2 is optimistic). A multi-plane slice is assigned wholly to its primary skill, which flatters the simulation slightly.
5. **Team shapes**: 3 = Model (session-core, model-gateway, execution, Data) / Control + Application + gateway / SPA. 5 = two Model engineers (session-core and runtime) / Control + Application + Data / gateway + SPA flexible / SPA. 8 = two session-core, one runtime + Data, one Control, one Control + Data, one Application + SPA, one gateway + SPA, one SPA. Anyone outside these skill sets cannot take a slice (no cross-training assumed).

### 4.7.2 By workstream, with the audit's size for comparison

Audit sizes carry no weeks; the S/M/L/XL to weeks conversion below is the one the designs use (S 0.5-1, M 1-2, L 2-4, XL 4-8). Designer sums are adopted in every case (D8); the audit's "effort counts the work to close the stated gap" and left out the cross-plane work the designers found.

| WS | Audit-equivalent ew | Designer firm ew | Designer mid | Ratio mid/mid | Conditional ew |
|---|---|---|---|---|---|
| WS01 | 3-6 | 9.3-13.8 | 11.6 | 2.6x | 0 |
| WS02 | 4-8 | 5.8-10.6 | 8.2 | 1.4x | 2.0-4.0 |
| WS03 | 2-4 (phase 1) | 6.5-9.8 | 8.1 | 2.7x | 0 |
| WS04 | 5-10 | 11.8-18.2 | 15.0 | 2.0x | 0 |
| WS05 | 5-10 | 15.6-24.2 | 19.9 | 2.7x | 0 |
| WS06 | 1.5-3 | 7.9-14.5 | 11.2 | 5.0x | 8.5-14.5 |
| WS07 | 3.5-7 | 7.1-11.1 | 9.1 | 1.7x | 0 |
| WS08 | 3-6 | 13.5-19.5 | 16.5 | 3.7x | 0 |
| WS09 | 6-12 | 11.0-14.7 | 12.8 | 1.4x | 2.0-3.0 |
| WS10 | 2-4 | 17.5-27.0 | 22.2 | 7.4x | 0 |
| WS11 | 8-16 | 15.8-22.7 | 19.2 | 1.6x | 0 |
| WS12 | 4-8 | 12.0-17.5 | 14.8 | 2.5x | 0 |
| WS13 | 5-10 | 10.1-14.9 | 12.5 | 1.7x | 5.5-7.5 |
| WS14 | 4-8 | 15.5-21.5 | 18.5 | 3.1x | 0 |
| WS15 | 6-12 | 11.0-16.9 | 13.9 | 1.5x | 2.5-4.5 |
| WS16 | 2-4 | 9.5-14.5 | 12.0 | 4.0x | 0 |
| WS17 | 6-12 | 17.8-25.7 | 21.8 | 2.4x | 0 |
| WS18 | 2-4 | 11.5-17.5 | 14.5 | 4.8x | 5.0-7.0 |
| WS19 | 2-4 | 11.0-17.0 | 14.0 | 4.7x | 1.5-2.0 |
| WS20 | 2-4 | 19.8-28.7 | 24.2 | 8.1x | 0 |
| **WS01-WS20** | **76-152** | **240.1-360.3** | **300.2** | **2.6x** | 27.0-42.5 |
| M0 baseline | not in the audit | 1.8-3.0 | 2.4 | n/a | 0 |
| Prep seams `P-*` | not in the audit | 5.5-8.4 | 7.0 | n/a | 0 |
| **Plan total (firm)** | | **247.4-371.7** | **309.5** | | |

Where designer and audit differ most, and why the designer figure is adopted:

| WS | Gap | Reason found in the code (designs) |
|---|---|---|
| WS20 (8.1x) and WS10 (7.4x) | audit L; designs 19.8-28.7 and 17.5-27.0 | WS20: a Control decision, a broker binding, three Session Core RPCs, a Quarry delegation, a hand-off kind, takeover. WS10: grants, gate, once-token, addressee, quarantine, tool-policy store; the card never said so |
| WS06 (5.0x) | audit S plus M; design 7.9-14.5 firm | F-t1-005 is S only for S1-S3; S5-S10 are F-t1-021 (M); S11 egress (XL) is parked outside the firm total |
| WS18 (4.8x), WS19 (4.7x) | audit L; designs 11.5-17.5 and 11.0-17.0 | WS18: registry golden, Capability Core v2, Session Core owner lane, execution-core adapter. WS19: Data repo, Control decision, wiki-store HTTP, gateway, three SPA slices |
| WS16 (4.0x), WS08 (3.7x), WS14 (3.1x) | audit M+M, L+M, L+L | WS16 needed a Control preference contract and three-store reconciliation; WS08 missed run-start authority, the reply slot and the history splice; WS14 had an owner bug and a dual fence |
| WS02 (1.4x), WS09 (1.4x) | closest to the audit | the audit's XL already counted the hardest parts |
| Designer vs designer | WS05's design calls the audit's estimate "holds" while its own slices sum to 15.6-24.2 ew against an audit-equivalent 5-10; WS02's design reads XL as 4-8 ew while WS05's reads XL as 16-24 ew | the units differ; this section sums slice figures and applies one conversion |

By milestone (firm; cumulative low-high at the end of each row):

| Milestone | Firm slices | ew low-high | mid | Cumulative mid | Conditional ew |
|---|---|---|---|---|---|
| M0 | 7 | 4.1-6.6 | 5.3 | 5.3 | 0 |
| M1 | 57 | 46.3-73.5 | 59.9 | 65.2 | 0 |
| M2.1 | 21 | 33.5-48.7 | 41.1 | 106.3 | 0 |
| M2.2 | 24 | 33.0-48.4 | 40.7 | 147.0 | 0 |
| M2.3 | 54 | 72.6-109.3 | 90.9 | 237.9 | see note |
| M3.1 | 25 | 38.0-56.5 | 47.2 | 285.1 | see note |
| M3.2 | 9 | 19.8-28.7 | 24.2 | 309.3 | 0 |
| **Total** | **197** | **247.3-371.7** | **309.5** | | **27.0-42.5** |

Conditional slices by milestone: M2 (WS02-S6, WS02-S7, WS06-S6b) 3.5-6.5 ew; M3 (WS06-S11, WS06-S12, WS09-S9, WS13-S9, S10, S11, WS15-S10, S13, WS18-S8, S9, WS19-S10) 23.5-36.0 ew. The 0.2 ew difference in the cumulative column is rounding of the per-slice mids. M2 as a whole is 139.1-206.4 ew (mid 172.8), 56 percent of the plan.

By lane and by skill (firm ew low-high, mid):

| Lane | Slices | ew | mid | | Skill | Slices | ew | mid |
|---|---|---|---|---|---|---|---|---|
| L0 PLAT | 3 | 1.8-3.0 | 2.4 | | MS (Model session-core, model-gateway) | 39 | 58.4-86.6 | 72.5 |
| L1 SCT | 19 | 29.1-42.6 | 35.9 | | MX (execution, capability, sandbox, broker) | 18 | 27.7-43.7 | 35.7 |
| L2 SCR | 20 | 29.3-44.0 | 36.6 | | GW (gateway BFF) | 21 | 21.1-32.9 | 27.0 |
| L3 RUN | 18 | 27.7-43.7 | 35.7 | | SP (SPA) | 53 | 56.2-83.8 | 70.0 |
| L4 CTL | 30 | 36.5-54.8 | 45.6 | | CT (Control) | 40 | 52.1-78.9 | 65.5 |
| L5 IDN | 8 | 14.1-21.7 | 17.9 | | AP (Application) | 18 | 21.6-30.7 | 26.1 |
| L6 APP | 18 | 21.6-30.7 | 26.1 | | DT (Data, Ingestion) | 8 | 10.3-15.1 | 12.7 |
| L7 GW | 21 | 21.1-32.9 | 27.0 | | | | | |
| L8 SPA-R | 35 | 39.3-58.4 | 48.8 | | | | | |
| L9 SPA-S | 18 | 16.9-25.5 | 21.2 | | | | | |
| L10 DAT | 7 | 10.0-14.5 | 12.2 | | | | | |

The largest lanes are L8 (48.8), L4 (45.6) and L2 (36.6); they are the first to be split between two engineers (4.3.1).

### 4.7.3 Critical path and calendar time

- **Critical path**: 15.6 / 19.1 / 22.5 ew (low / mid / high, strict); 13.9 / 17.1 / 20.1 with contract-first overlap. At 0.8 productivity that is about 24 calendar weeks at mid with unlimited engineers (21 with overlap). It is the WS20 chain of 4.2.2.
- **Capacity floor** (total mid / (N x 0.8)): 3 engineers 129 weeks, 5 engineers 77, 8 engineers 48. The simulated times are higher because skills are not interchangeable and each milestone gate leaves some skills idle.
- **M1 is the only path-bound milestone**: its own critical path is about 11 ew strict (spine WS01-S1..S5, then S6 and S7b), about 14 weeks at 0.8; more than 8 engineers do not shorten M1 below roughly 13 weeks.

Simulated completion times in calendar weeks, milestone-ordered (the plan's gating), for low / mid / high ew. "M1" includes M0. Months at mid use 4.345 weeks per month.

| Team | M1 done | M2.1 done | M2.2 done | M2 done (all of M2) | M3.1 done | All firm done |
|---|---|---|---|---|---|---|
| 3 engineers | 25.9 / **33.6** / 40.6 | 42.5 / **53.8** / 64.7 | 65.1 / **82.7** / 99.5 | 101.2 / **127.8** / 153.2 | 116.9 / **147.1** / 175.9 | 133.9 / **167.9** / 200.7 |
| months at mid | 7.7 | 12.4 | 19.0 | 29.4 | 33.9 | 38.6 |
| 5 engineers | 16.5 / **21.2** / 25.5 | 29.6 / **39.0** / 46.9 | 40.9 / **50.4** / 61.4 | 72.3 / **89.2** / 108.3 | 84.6 / **104.4** / 126.3 | 100.2 / **123.5** / 148.9 |
| months at mid | 4.9 | 9.0 | 11.6 | 20.5 | 24.0 | 28.4 |
| 8 engineers | 11.7 / **16.8** / 17.4 | 23.8 / **28.5** / 36.5 | 36.4 / **43.6** / 54.0 | 37.6 / **45.7** / 56.6 | 43.2 / **52.6** / 64.7 | 54.3 / **67.0** / 80.3 |
| months at mid | 3.9 | 6.6 | 10.0 | 10.5 | 12.1 | 15.4 |

Sensitivity at mid (calendar weeks to finish all firm slices; the first column is the table above):

| Team | milestone-ordered, eff 0.8 | critical-path-first, eff 0.8 (ignores milestone gates) | eff 0.7 | eff 0.9 |
|---|---|---|---|---|
| 3 | 167.9 | 152.7 | 191.6 | 149.4 |
| 5 | 123.5 | 98.6 | 141.0 | 108.5 |
| 8 | 67.0 | 54.0 | 77.2 | 58.8 |

Milestone gating therefore costs about 10 percent (3 engineers) to 25 percent (5 and 8 engineers) of total calendar time; it buys shippable gates. The honest reading: **a 3-engineer team needs about 3 years for the whole plan, so it should stop at a release line**.

Recommended release lines (weeks at mid, milestone-ordered):

| Release | Content | 3 eng | 5 eng | 8 eng |
|---|---|---|---|---|
| R1 | M0 + M1: people post, live room, governance console, room admin, security fixes, contained memory | 34 | 21 | 17 |
| R2 | + M2.1: agents in a human room (addressed wake, live run view, approvals owner path) | 54 | 39 | 29 |
| R3 | + M2.2: attention, DMs, edit and delete, membership depth | 83 | 50 | 44 |
| R4 | + M2.3: search, files, timeline, UI baseline, SSO and SCIM, member approvals (M2 complete) | 128 | 89 | 46 |
| R5 | + M3.1: personal agents, labels, self-API, canvas | 147 | 104 | 53 |
| R6 | + M3.2: room browser | 168 | 124 | 67 |

If the owner wants the priority items WS17-WS20 earlier, the cheapest cut is to start WS17-S3, WS19-S1..S4 and WS18-S1 in the idle lanes of M1 and M2 (L3 RUN has only 1.2-2.2 ew of M1 work and L10 DAT has none), which the simulation already does when a skill is idle.

## 4.8 The first three PRs

Chosen because each is independent of the others and of every open decision, high in value, low in risk, unblocks other lanes, and can start the day the plan is approved, in parallel with M0. Their paths were checked: unchanged on `model-plane`, `quarryv2` and `quarryv2-next` against `afb83928`, and clean in the live tree (4.0.1), so they can start on any base and be cherry-picked onto `space`. None carries a proto tag; one carries a migration (user-core 033). Commit trailers follow the repo's attribution rule.

| | PR-1 | PR-2 | PR-3 |
|---|---|---|---|
| **Title** | `fix(audit): role-gate org audit and usage reads (WS05-S1)` | `feat(control): catalogue-driven space effect policy writes with audit events (WS03-S1)` | `feat(spaces): thread side pane with ?thread_id deep link, read-only (WS01-S7a)` |
| **Branch** | `fix/space-audit-read-role-gate` | `feat/space-effect-policy-catalogue` | `feat/space-thread-pane-readonly` |
| **Lane, size** | L5 (with L9 for one SPA file), S, 0.5-1 ew | L4, M, 1.0-1.5 ew | L8, M, 1.0-1.5 ew |
| **Closes / unblocks** | the audit read path that lets any member read org audit rows (a security bug); releases (b)'s mp-WS01.S2 audit replay of 5,000 rows (O13, `05` X21) | the "dev policy admin": `UpsertEffectPolicy` becomes catalogue-driven, so WS09-S7, WS10-S2, WS12-S5, WS17-S4, WS19-S2, WS20-S1 each add one catalogue row instead of editing SQL (rule R3); gives `M0-T0` and every live proof a safe way to set policy rows | the right-hand pane and the `?thread_id=` reader that Activity already emits and nothing reads; unblocks `P-SPA1`, WS13-S4a, WS12-S7, WS09-S6 click-through, WS17-S6, WS20-S5 |

### PR-1 `fix(audit): role-gate org audit and usage reads (WS05-S1)`

- **Scope**: audit-core reads (`listAudit` `api.go:197`, `listUsage` `:224`, `summariseUsage` `:250`, line numbers re-verified) return the org's rows only to owner or admin (role from the signed v3 delegation, canonical check); any other member gets their own rows, with a redacted `tool_action` projection so the Trust Center still aggregates; typed `audit_scope_denied` 403 for a forged or absent role; gateway rewrites a member's query to self scope and clamps `limit`; SPA gates `RecentSecurityEvents` (`WorkspaceSettingsPage.tsx:1256`, render `:1557`) and adds `listOwnAuditEvents`. Switch `AUDIT_READ_ROLE_ENFORCEMENT` (`enforce` default, `report`, `off`).
- **Files**: `apps/Control Plane/audit-core/internal/api/api.go`, `service_auth.go` (`IsOrgAdmin()`), `api_test.go`, `service_auth_test.go`, `apps/Control Plane/audit-core/.env.example`, `apps/Control Plane/docker-compose.yml` (about `:829`), `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/audit.rs` (105 lines, no tests today), `.../src/features/settings/components/WorkspaceSettingsPage.tsx`, `TrustCenterSection.tsx` (test only), `.../src/shared/api/audit-client.ts` and `audit-client.test.ts`.
- **Tests**: `cd "apps/Control Plane/audit-core" && go vet ./... && go test ./...` (role matrix with the signed delegation helper of `service_auth_test.go:61-136`: owner, admin, member, viewer, forged header, missing claim, plan flag); `cd "apps/Frontend Plane/verevonv3" && cargo test --manifest-path apps/gateway/Cargo.toml --all-targets` (new `audit.rs` tests: admin detection, self-scope rewrite, 403, clamp); `pnpm exec vitest run src/shared/api/audit-client.test.ts src/features/settings && pnpm typecheck && pnpm lint`.
- **Definition of done**: (1) the role matrix above passes; (2) rebuilt images: `cd "apps/Control Plane" && scripts/run-control-plane.sh up -d --build audit-core` and `cd "apps/Frontend Plane/verevonv3" && docker compose up -d --build gateway`, SPA on a second Vite server (not `:5173`); (3) live flow: org owner `GET /api/v1/audit` returns org rows; a plain member returns only their own rows with `meta.scope` set; a request with a forged role header gets 403 `audit_scope_denied`; the Trust Center page still renders its aggregate for a member; (4) `AUDIT_READ_ROLE_ENFORCEMENT=report` for one release in dev and staging to find unknown readers, then `enforce` in production; (5) evidence bundle with the commands, HARs and test summaries; a note to sibling (b) that O13 is released. If a second org member cannot be seeded before `M0-T0`, the member path is proven by the Go test and the live member proof is recorded as pending, not skipped.
- **Risk**: an unknown consumer of the unscoped read breaks under `enforce`; mitigated by `report` mode and the redacted Trust Center view.

### PR-2 `feat(control): catalogue-driven space effect policy writes with audit events (WS03-S1)`

- **Scope**: `UpsertEffectPolicy` (`repository.go:1229`) takes an actor and a catalogue of entitlement columns; an entitlement omitted from the request keeps its stored value (today the PUT replaces the whole policy, `06` T0); every write inserts an immutable event and a `user_audit_outbox` row in one transaction; `fenceActiveSpaces` stays; `set-space-effect-policy.sh` gains the two missing flags and sends only the flags passed.
- **Files**: `apps/Control Plane/user-core/migrations/033_space_effect_policy_events.up.sql` (the design's 032 renumbered by 4.6.B; events table, indexes, immutability trigger), `.../internal/spaces/entitlement_catalog.go` (+ `_test.go`: catalogue, drift and ZDR-binding tests), `.../internal/spaces/policy_audit.go`, `.../internal/spaces/repository.go`, `.../internal/http/spaces.go` (`upsertSpaceEffectPolicy` `:1359`, pointer entitlements, actor), `.../internal/http/thread_read_decision_live_test.go` (call site `:121`), `.../internal/spaces/effect_policy_audit_live_test.go` (new), `apps/Control Plane/scripts/set-space-effect-policy.sh`.
- **Tests**: `cd "apps/Control Plane/user-core" && go vet ./... && go test ./internal/spaces/... ./internal/http/...` (live tests need Postgres 16; they are the same ones `user-service.yml` runs); a drift test fails when a column exists without a catalogue row or the reverse.
- **Definition of done**: (1) tests above green; (2) rebuilt image: `cd "apps/Control Plane" && scripts/run-control-plane.sh up -d --build user-core` (a bare compose build exits 0 and builds nothing); (3) `psql` shows `schema_migrations` row 033 and `\d space_effect_policy_events`; an `UPDATE` and a `DELETE` on the events table are rejected; (4) live flow on the dev org: run the script with one entitlement flag, then again with a different one, and the first stays set; each write leaves one event row and one outbox row whose `published_at` fills; health `wget --spider http://127.0.0.1:3012/health` (database round trip); (5) evidence bundle; (6) ledger row for user-core 033 appended by L0.
- **Risk**: wire addition breaks a strict decoder in a service caller; WS03-S2 audits callers; S1 keeps statuses and error strings byte-stable (test). A raw `PUT /effect-policy` keeps working meanwhile.

### PR-3 `feat(spaces): thread side pane with ?thread_id deep link, read-only (WS01-S7a)`

- **Scope**: a generalised right pane (`SpaceAuxPane`) bound to `?thread_id=`; read-only thread panel over today's transcript route; honest restricted state (the swallowed 403 at `SpaceRoomTimeline.tsx:201` becomes a named state); `SpaceTurnList` extracted from the timeline so rows render in both places; the link builder in `activity-grammar.ts` (`:200` and `:326`, both re-verified, already emit `?thread_id=`) and the sidebar thread link in `CoreSidebarSpacesPanel.tsx` (`:298`, which links to the Space without `thread_id` today) stay inside `features/spaces` (a test forbids the literal `/chat?thread_id=` elsewhere). `reply_count` is optional (absent until WS01-S1). The BFF flag object `features.thread_pane` arrives with WS01-S5; until then the pane is enabled by a build-time constant that defaults on in dev and can be reverted as one line (the slice owner may instead add the single `thread_pane` field through `P-GW1`).
- **Files** (all `apps/Frontend Plane/verevonv3/src/...`): new `features/spaces/components/aux/{SpaceAuxPane,SpaceThreadPanel,SpaceRestrictedNotice,SpaceTurnList}.tsx`, `aux/space-aux-pane.css`, `features/spaces/lib/space-thread-deep-link.ts`; modified `features/spaces/components/SpacePage.tsx` (`useSearchParams`, pane host), `SpaceRoomTimeline.tsx`, `features/spaces/lib/activity-grammar.ts`, `features/core/components/sidebar/CoreSidebarSpacesPanel.tsx`; tests `SpaceAuxPane.test.tsx`, `SpaceThreadPanel.test.tsx`, `activity-grammar.test.ts`, `src/features/chat/lib/chat-route-ownership.test.ts` (must stay green), e2e `tests/e2e/space-thread-pane.spec.ts` (new). It does not touch `SpaceRoomComposer.tsx`, so the live tree's 2-line change does not block it.
- **Tests**: `cd "apps/Frontend Plane/verevonv3" && pnpm exec vitest run src/features/spaces src/features/chat/lib/chat-route-ownership.test.ts src/shared/actions && pnpm typecheck && pnpm lint`, failing sets compared with the stored baseline; `pnpm exec playwright test tests/e2e/space-thread-pane.spec.ts` on a second Vite server with `E2E_REQUIRE_SPACE=1` (the spec needs `E2E_SPACE_REF`; a second member needs `M0-T0`, until then the owner-only path and the restricted state with a viewer storage state are recorded pending).
- **Definition of done**: (1) tests green; (2) the SPA verified on a second Vite dev server (not `:5173`, the main checkout's container over a Windows bind mount that never sees worktree edits), restarted, with a hard reload because the browser caches `global.css` across restarts; (3) flow: open `/spaces/<ref>?thread_id=<id>#chat` from the Activity link: the pane opens with the transcript read-only, focus moves in and returns on Escape, keyboard resize works, width persists; a thread the viewer cannot read shows the restricted state, never "log in again"; (4) screenshots and a Playwright trace in the evidence bundle; (5) no change to any gateway, Control or Model Plane file.
- **Risk**: first PR to edit `SpacePage.tsx` and the timeline; later SPA slices wait for `P-SPA1` behind it (4.4 rows 5 and 7).

### Also in the first week, no PR slot needed

`M0-BASE` (base branch cut, ledger entry, decisions SD-11 and SD-15), `M0-T0`, `M0-CI`, then `P-SC1`, `P-GW1`, `P-UC1`, `P-CX1`; small independent fixes WS05-S3 (`fix(auth-core): remove SSO attribute-to-admin rule`, S) and WS05-S2 (SCIM stub to 501, after the integration-corev2 owner commits its dirty hunks); List-A slices WS07-S1, S5, S7, S13, WS16-S1, WS16-S2, WS06-S3, S5, S7, WS14-S1, WS13-S6, WS19-S1, WS18-S1 for lanes with spare capacity.

## 4.9 Rollout order of flags across dev, staging and production

Principles: (1) flags are rollout controls, never authorization inputs (Control decisions and Session Core gates decide); (2) a flag turns on first where the next plane's contract is deployed (R9); (3) ZDR orgs are refused by design at every step, with typed refusals; (4) each flip is gated by the slice's DoD flow with the flag on and then off (kill-switch drill, `06` 6.4); (5) entitlement bits in `space_effect_policies` default FALSE and are set per org through the audited WS03 path (SD-18), so production exposure is per org: internal org, pilot orgs, then all; (6) observe or shadow precedes enforce wherever a flag has the mode. **Dev** = local stack with the dev compose override (core flags `"1"`; gateway flags on when `APP_ENV` is not production via `dev_flags_allowed`). **Staging** = the pre-production stack with a real second member, a ZDR org Z and a fresh policy row; whether the repo has such a stack beyond local rebuilt images is UNVERIFIED (SD-27). **Production** = `APP_ENV: production`: gateway flags off, core flags off unless stated.

| Wave | Flags in this order | Dev | Staging | Production | Preconditions (gates, siblings) | Kill switch |
|---|---|---|---|---|---|---|
| 1 Always-on safety (M1) | `space_run_thread_guard` (WS09-S1); `MEMORY_SPACE_CONTAINMENT` (WS17-S1); `AUDIT_READ_ROLE_ENFORCEMENT` (WS05-S1); `EXECUTION_SANDBOX_LIMITS` (WS06-S7); modes `SESSION_CORE_SPACE_ZDR`, `SPACE_AGENT_ADMISSION`, `EXECUTION_CORE_SPACE_SHELL_LEASE`, `CLIENT_VERSION_POLICY` | on; role gate `report` one release | on | on everywhere on merge (guard before wave 3); role gate `enforce` after the report release; the four modes start at `observe` or `banner` | WS17-S1 counters one week before (b) sets `GATEWAY_CHAT_PROJECTS_ENABLED`; WS09-S1 before wave 3 | `0` per flag; `report`/`off` for the role gate |
| 2 Governance and room admin (M1) | `SPACE_POLICY_ADMIN_WRITES`, `space_policy_admin_api`, `space_denials_typed` (WS03); `space_org_room_reconcile_sync` and `space_audience_reconcile_sync` (WS04-S1, S5: the first two production flags); `space_room_details_ui`, `space_people_search_api`; `space_membership_writer_api` (after WS04-S4 deployed dark and S5 on); `space_room_archive_api` | on | on | admin writes off until the attestation answer of SD-18; reconcile flags on first; others per org | G2 for membership writer; SD-13 for production writer enablement | gateway flip or Convex env `0` plus `docker restart convex-gateway` |
| 3 Human posts and live room (M1) | `space_human_posts_core` then `space_human_posts_api`; `space_thread_pane_ui`, `space_send_idempotency_api`, `space_connectivity_banner_ui`; `space_realtime_emit` then `space_realtime_stream` | on | on with a real second member; emit first, wait one retention-sweeper cycle, then stream | per org after staging; emit before stream; stream for one org first | G2, G3; WS09-S1 on; (b) mp-WS01.S2 outbox fix deployed (E3) before live emit; E2 for send keys | env `0` + restart; SPA falls back to the 6 s poll |
| 4 Artifacts and client hardening (M1) | `SPACE_ROOM_ARTIFACTS_API`; `CLIENT_VERSION_POLICY` `banner` then `enforce`; `CSP_REPORT_LOG_HOSTS` | on | on; CSP report staging only | artifacts: one org then all; `enforce` only after WS01-S6 and WS15 composers are registered; CSP report never in production | WS07-S9 deployed Session Core first, then model-gateway, then gateway | gateway flip; `off` |
| 5 Agents in the room (M2.1) | `space_mention_records_core/api`, `space_agent_wake_policy_api` (WS08-S5 can start in pilot orgs first), `space_wake_core/api`, `space_addressed_wake_api`; `space_wake_room_context` stays off until its decision; `space_run_work_summary`, `space_run_live_view` (no text), `space_run_live_text` after SD-20, `space_run_owner_steer`, `space_run_manager_stop` with `run_control_entitled`; `space_approval_grants`, `EXECUTION_CORE_SPACE_APPROVAL_GATE` `off` > `shadow` > `enforce`, `space_tool_policy_editor`; `space_mention_notify` last | on, gate `enforce` | gate `shadow` for two weeks comparing would-be levels, then `enforce` | per org by entitlement; text frames off; gate `shadow` first | G4 (SD-00, SD-05, SD-20, SD-21, SD-26); notification-core redeployed with the new key before `space_mention_notify` | `off` returns today's behaviour (tighten-only) |
| 6 Attention, DMs, lifecycle (M2.2) | `space_attention_summary` > `space_attention_api` > `space_attention_ui`; `space_attention_emit` (needs `space_realtime_emit`) > `SPACE_ATTENTION_INGEST` > `SPACE_ATTENTION_OUT_OF_APP` last; `SPACE_DM_REGISTRATION` > `SPACE_DM_OPEN` > `SPACE_DM_API` with `dm_create_entitled`; `SPACE_MESSAGE_REVISE_CORE` > `SPACE_MESSAGE_REVISE_API`, `SPACE_REACTIONS(_API)`; `space_membership_events_sweeper`, `space_directory_resolver_api`, `space_room_directory_api` (per org row), `space_history_window_read` (opt-in per room) | on | on | per org; DMs only where WS04-S5 is on; directory off until an org row exists | G4 (SD-13, SD-14, SD-17, SD-19); Novu workflows before out-of-app delivery | env `0`; org row false for the directory |
| 7 Search, files, timeline, baseline, identity, sandbox (M2.3) | `space_message_search_core` > `_api` > `_ui`, `space_entry_links_ui`, `space_command_palette_ui`, `space_room_pins_api`; `CONTROL_SPACE_SHARED_IMPORT`, `CONTROL_SPACE_SUBJECT_GRANTS`, `DOCUMENTS_SPACE_FILES`, `RETRIEVAL_SPACE_GRANTS`, `IMPORTS_SPACE_UPLOAD`, `space_room_files_api`, `space_turn_attachments_ui`, `space_room_files_ui`, then (actions re-pointed first) `CONTROL_OWNER_ASSERTED_GRANTS`, `DOCUMENTS_SHARE_WRITES`; `SPACE_TIMELINE_CORE` > `SPACE_TIMELINE_API` > `space_timeline_api` > `features.timeline_windows`, `system_entries`; `space_sidebar_api`, `chat_locale_hint` (after the Model image carrying the field is deployed); `AUDIT_EXPORT_ENABLED`, `SPACE_AUDIT_DECISION_EVENTS`, `ORG_DOMAIN_VERIFICATION_ENABLED` > `ORG_IDENTITY_ADMIN_API`, `SSO_ENFORCEMENT` (`report` everywhere; per-org `enforce` after a clean week), `ORG_SCIM_ENABLED` + `SCIM_EDGE_ENABLED` (pilot org with a real IdP only); `SANDBOX_MANAGER_REQUIRE_RETENTION_CLAIM` (after the real-mint replay test), `EXECUTION_CORE_SPACE_SHELL_LEASE` and `SESSION_CORE_SPACE_ZDR` `observe` > `enforce` after one release; `space_approval_addressee`, `EXECUTION_CORE_QUARANTINE_MODE` `shadow` > `enforce` with Control `quarantine_mode` | on | on; search migration 0077 applied in a quiet window with the plan-shape test first | per org by entitlement; SCIM and SSO enforce pilot only; ZDR `enforce` per stack after the counter shows real exposure | G5, SD-12 before any `enforce`; SD-24 for WS05-S5..S8; retrieval-engine-rs residue committed (4.1.2) before WS14-S5; (b) `AGENT_CONTINUE_AFTER_APPROVAL_ENABLED` is not a precondition (action-only continuation) | `off` per flag; SSO ceiling `off` platform-wide |
| 8 Personal agents, self-API, canvas (M3.1) | `MEMORY_AUDIENCE_LABELS` (after the WS17-S3 backfill is verified), `SPACE_AGENT_ADMISSION` `observe` two weeks > `enforce`, `personal_agent_admit_entitled`, `space_personal_agents_api/ui`, `space_private_panel_ui`, `memory_declassify_entitled` + `space_memory_declassify_api`, `space_personal_memory_turn_api`; `CONTROL_SPACE_SELF_API_ACTIONS` (rename and pin only first), `SPACE_SELF_API_SESSION_CORE`, `EXECUTION_CORE_SPACE_SELF_API`, `MODEL_GATEWAY_SPACE_SELF_API`, `SPACE_AGENT_SELF_API_API`; `canvas_entitled` (one pilot org), `WIKISTORE_SPACE_CANVAS`, `SPACE_CANVAS_API`, `SPACE_CANVAS_UI`, `SPACE_CANVAS_AGENT_TOOLS` | on | on | per org; personal agents in shared rooms only per org opt-in (SD-22); canvas pilot only until SD-08; agent canvas tools default-on only after WS10-S5a and WS17-S1 | G6 (SD-03, SD-04, SD-22); (b) `GATEWAY_CHAT_PROJECTS_ENABLED` waits on WS17-S1 and WS17-S3 | `0`; entitlement false takes effect within one decision life (2 minutes) |
| 9 Room browser (M3.2) | `BROWSER_BROKER_SPACE_GRANTS`, `SPACE_BROWSER_RUN_API`, `browser_entitled`, `SPACE_BROWSER_VIEW_RELAY` with Quarry `QUARRY_ROOM_VIEW_ENABLED`, `SPACE_BROWSER_HANDOFF`, `space_browser_manager_control` | on with a test-double broker | on once (a) grants and room view are in the same stack | per org by entitlement after SD-23 | (a) `BROWSER_GRANT_ENFORCEMENT`, takeover enforcement, F-t2-013 phase 1 (E6, E7) | `0`; the card keeps working from events |

M4 flips production defaults wave by wave in this order, one org cohort at a time, and removes a flag one release after its default has been on everywhere.

## 4.10 What this section needs from the owner, and what was not verified

Needed (ranked; each is also in `02` 2.6 or `05` 5.4 where marked):

| # | Question | Recommendation | If it waits |
|---|---|---|---|
| 1 | SD-11 (`02`): base branch `space` from the `model-plane` tip, one named integrator for all three plans, one ledger entry carrying 4.6 (`05` question 1) | yes | only List-A slices and the first three PRs proceed |
| 2 | Approve the milestone amendments D1 and D2 of 4.0.3 (WS09-S1, WS17-S1/S2 into M1; WS04-S8..S12, WS05-S4..S9, WS06-S6..S10 into M2; T0 into M0) | yes | `06`'s scenario sets and this plan disagree on M1 |
| 3 | Approve the prep seams of 4.4.2 and the new `space_*.proto` files (4.4 row 14) | yes (about 2 percent of effort) | hotspot conflicts grow with every lane |
| 4 | Name the team size and the release line (4.7.3): at 3 engineers stop at R1 or R2 | state R1-R6 target and headcount | the calendar column of 4.7.3 cannot be committed |
| 5 | Who lands or discards the live-tree residue of 4.1.2 (retrieval-engine-rs, `notifications-client.ts`, composer 2 lines) and when | owner, before WS14-S5, WS11-S3 and `P-SPA2` | those three slices are blocked |
| 6 | May Space land the session-core half of mp-WS04.S6 (`client_message_id`, author in the unique key) under (b)'s name and number if WS01-S1 is ready first (`05` question 6) | yes | WS01-S1 and WS01-S8 wait for (b) |
| 7 | One weekly Control PR train for principals, `nats.conf` and compose across all three sessions (`05` question 19, E9) | yes | each lane edits the same JSON and ACL by hand |

Not verified (UNVERIFIED) and where it bites:

1. Per-name numbers inside (b)'s 0049-0066 range (derived from the order of its plan table; its ledger may differ).
2. Whether Convex codegen accepts table definitions imported from `convex/schema/<feature>.ts` (spike inside `P-CX1`).
3. `sandboxes.proto` field 7 for WS06-S6b against (b) mp-WS22 reservations (the slice is conditional).
4. Whether a staging stack exists beyond local rebuilt images (4.9).
5. Whether the user-core runner needs numbers in strict sequence beyond lexicographic order (read: it applies pending `*.up.sql` in lexicographic order, `database.go:72-74`; gaps assumed safe).
6. The audit-core health port and the exact member seeding path before `M0-T0` (PR-1 DoD).
7. The skill-set mapping of multi-plane slices in the simulation (4.7.1 item 4).

Re-verified in this pass: branch tips and counts, session-core and user-core migration tops, proto diffs since `e8f3561c`, sibling overlap of 26 files, hot-file sizes, the live-tree status counts and Space-relevant dirty files, the line numbers cited in 4.8 (`UpsertEffectPolicy` `repository.go:1229`, `upsertSpaceEffectPolicy` `spaces.go:1359`, call site `thread_read_decision_live_test.go:121`, `listAudit` `api.go:197`, `listUsage` `:224`, `summariseUsage` `:250`, `RecentSecurityEvents` `WorkspaceSettingsPage.tsx:1256` and `:1557`), and that the first three PRs' paths are unchanged on every sibling branch.

<!-- COMPLETE -->

