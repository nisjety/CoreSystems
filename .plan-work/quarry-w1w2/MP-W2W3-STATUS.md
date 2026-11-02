# MP W2/W3 status (Model Plane rows of Quarry W2 and W3)

Branch `mp-w2w3` (worktree `mp-w2w3-350537`), cut from `quarryv2-next` at `cc8c43389`. Brief: `side-session-notes/local_6a0cd694-.../mp-w2w3-brief.md`. Specs: `.plan-work/quarry-w1w2/mp-w2w3/spec-A.md` (WS7.L, WS8, WS9, WS10, WS12.S4) and `spec-B.md` (WS11, WS16, INNH-10, W3 rows).

Rules: W2 rows are tested in process (unit, integration with a scratch Postgres where needed), with no live stack. W3 rows are built and compile-checked only, and the user tests them after the merge. Every new behaviour sits behind a flag that defaults off. Commits are local only.

Status key: built / tested / commits / flags / assumed decisions / not verified.

## W2 rows

| Row | Built | Tested | Commits | Flags (default) | Assumed decisions | Not verified |
|---|---|---|---|---|---|---|
| U03 WS8.A ledger row | yes | n/a (docs) | see git log | none | ADR-3 + X-13 taken as ratified (owner order 2026-11-01) | spike measurements (live lane) |
| U61 broker browser_grants + migration 0004 | yes (test fix) | broker `go test ./...` and `-tags integration` on scratch Postgres 16: all green after fixing stale `TestPg_MigrationIsRerunnable`; `cargo test -p model-gateway --lib browser_`: 101 passed, 0 failed (17 of them `browser_grants`), at 7471662b1 before batch 1 | 7471662b1 | n/a | none | none |

U61 live check (read only, 2026-11-01): the live broker image ships `0001`, `0003` and `0004` in `/app/migrations`. The broker's database `browser_broker` on `ingestion-postgres` has the 0004 columns (`policy_mode`, `denied_domains`, `resource_id`), the widened kind, domain, parent, policy and resource checks, and the indexes `browser_grants_attach_idempotency_idx` and `browser_grants_owner_human_active_idx`. D2-140 is closed. Side finding: `session_core` on `model-plane-postgres-1` also holds an old pre-0004 `browser_grants` table, which looks like a leftover from before the broker got its own database. It is recorded in the depth file and not touched.

| F12 trust fix (WS11.S0 / WS9.E line) | yes | `cargo test -p mp-content-safety` lib 21 + corpus 10 pass | e0ced5d49 | none (always on, tightening) | none | live render of `[source: browser-scraped]` |
| WS11.S0 planner seam, PlannerView, ZDR on InferRequest, hermetic harness | yes | `execution-core --lib -- planner_view llm_planner provenance browser_agent` 139 pass; `--test browser_planner_hermetic` 2 pass | 3d50c894a | none (hygiene always on) | none | live LlmPlanner call to inference-core |
| C0 Quarry client contract + typed errors | yes | `--lib quarry_agent` 23, `--lib browser_agent` 77, hermetic 2 | e5ef4c8a6 | none (optional fields, behaviour unchanged) | none | live Quarry |
| WS11.S1 + U21 (402 BUDGET_EXHAUSTED -> resource_exhausted; 429 backpressure) execution-core half | yes | `browser_agent` 82, `tool_bridge` 19, hermetic 4 | 05b5ee03d | ceilings env `QUARRY_BROWSER_AGENT_*` | caller steps over 100 / runtime over 900 s are now clamped even with no env (plan ceilings); the 200k planner-token budget binds a selected LLM planner | `agent.budget_exhausted` event projection (gateway, DEPTH); `deadline_at` never set in production |
| WS11.S3 recoverable failures, failure budget, planner error policy | part 1 of 2 | `browser_agent` 89, `quarry_agent` 25, `llm_planner` 37, `tool_bridge` 20, hermetic 11, `browser_agent_e2e` 11 | b8b79e18f | n/a | plan defaults | procedure.rs, done payload, Tier A corpus (batch 3) |
| WS16.S1 service lane on POST /v1/ai/chat | yes | model-gateway lib 39 named pass (with WS11 gateway halves); cost-core `go test ./cmd/` ok; golden regenerated; inference-core test: see batch-1 regression run | 85822076e | `MODEL_GATEWAY_AI_CHAT_METERING` default **off** (plan said `service`), `MODEL_GATEWAY_SERVICE_RPM` 120 | metering default off (owner rule beats plan default); `purpose` gated by sub `service:quarry-edge`; route rows keep `service: None` | e2e_invoke_chain tests not written (target pre-broken on Windows); live ledger |
| WS11.S1 + S2 model-gateway halves (planner token ceiling, org planner flag) | yes | in the 39 above | 4dc0bb48b | `MODEL_GATEWAY_BROWSER_PLANNER_QUOTAS` default off | quota key read fails open (tokens) / closed (planner flag), per plan | live org-core read |
| INNH-10 fail-open fix (broker OwnerFilter/Authorize) | yes | broker 308 pass | 4501c9a85 | none (security fix) | unknown principal types refused everywhere | none |
| INNH-10 MP-1 api_key principal (authctx + broker authz) | yes | authctx 53, broker 338, bridge-core 50, capability-core 1462, cost-core 142, letta-bridge 61, orchestrator-core 290, sandbox-manager 272: all pass | 0beba0eb1 | `BROWSER_BROKER_KEY_GRANTS_ENABLED` default off; `authctx.Config.AcceptAPIKey` default off | OD-1, OD-5 at design defaults; a key sees only grants stamped with its key | real Auth Core token |
| INNH-10 MP-2 broker key grants (key_id column + migration 0005, issue path, revoked-key list, proto key_id = 60, api_key_revoked consumer, compose) | yes | broker 385 unit, 479 with the scratch-Postgres integration tests; Rust check of model-gateway, execution-core and session-core clean; contract mjs 30/33 (the 3 failures predate this work) | e52521b81, dedfed20c, e6b18b9a9, 52c09f1b0, c7146e936, review a335f02e4 | `BROWSER_BROKER_KEY_GRANTS_ENABLED=0` (+ TTL, per-key cap, revoke NATS env) | ZDR key grants: key_id kept off the issue log line; the revoked-key list is per process; an unreachable revoke bus disables key grants only | the consumer against real JetStream; the revoked-key list is lost on restart (120 s window, DEPTH) |
| INNH-10 MP-3 model-gateway key-grant routes | NO (batch 2; contract in progress-C.md) | | | | | |
| WS10.S2 envelope contract | yes | Go envelope 4 + gen 3 new pass; mp-contracts proto_wire_parity 5; mp-events 54; session-core event_type test pass; zdr-contract-schema 65 pass; sync script: all 24 run-trace rows pass (10 pre-existing reds on Windows) | 65aae102b, fix bc423459e | none (additive) | D6: `buf breaking` refused shrinking `reserved 14 to 20`, so fields are 21-25 (plan fallback); 160 reserved for WS8.G | Python gen not imported |
| WS10.S5 Model Plane half (stream, principal, consumer grants) | yes | provisioner suite pass incl. 3 new against an in-process broker from the real nats.conf; bootstrap test: 2 new pass (18 pre-existing Windows reds) | d1d63a324 | nothing publishes until Quarry does | X-4 review still gates enablement | smoke script unrun (needs live NATS) |
| WS10.S6 persistence + EventLog + trace route | NO (batch 2) | | | | | |

**Ops note for the merge (WS10.S5):** the Model Plane compose now REQUIRES `MODEL_QUARRY_TRACE_NATS_PASSWORD` for `nats`, using the same `:?` pattern as its siblings. This fails closed on purpose: an empty value would give `quarry-trace-producer` a blank password. Run `scripts/bootstrap_runtime_environment.sh` before the next `model-nats` recreate, or the broker will not start.
**Done 2026-11-01, user's order:** one random 64-hex value was generated into the main checkout's `apps/Model Plane/deploy/.env` (the env ledger, read by dev-mode `compose.sh`) and `apps/Ingestion Plane/.env` (the script's sync target). It was added only where the key was missing, and the value was never printed. A later bootstrap run reads it instead of minting a new one. The key is registered in `.env.example` and the release secret list (25ff8fece). The live `model-nats` container is unchanged until it is next recreated; at that point it picks up the new user from `nats.conf`.

## W3 rows

(Rows added as each batch lands.)

## Assumed owner decisions (defaults taken, owner said "stop asking / just do it")

| Id | Decision | Default taken | Source |
|---|---|---|---|
| ADR-3 / X-13 | shared human + agent browser, human_browse and attach grants, bounds | ratified as designed | brief, plan X-13 |
