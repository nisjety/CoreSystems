# Space agent self-API (WS18): rollout and rollback

Owner: Model Plane operator, with the Control, Application and Frontend owners for their switches. For the development stack first, then staging, then production per organisation. The development stack is not production; the switches below are never turned on in production by this runbook alone.

**Status.** Source is built for WS18-S1 to WS18-S5 (registry golden, Control and capability views, Session Core owner path, execution-core adapter, binding switch and gateway routes), with the WS18-S4a decode step, and for WS18-S6 (the SPA switch, registry view and provenance chips). The proof harness is written (WS18-S10: `apps/Model Plane/scripts/tests/space-self-api-proof.sh`). **The live proof has not run.** Nothing here claims the feature is live-proven. The card (WS18-S7, needs WS15-S9) is not built; its capability row is enabled by decision D3 (migration 0047, see the switch list). `space.agent.pause_self` (WS18-S8, conditional) and `space.schedule.follow_up` (WS18-S9, conditional) are not built.

**What is switched.** Run-bound `space.thread.rename` and `space.thread.pin` on the run owner's own thread. The run owner is the delegating human (ADR 5); a room agent never acts with more authority than that person. `space.thread.post_card` (card posting, decision D3) is offered once capability-core migration 0047 enables its admission row, `MODEL_ACTION_ADMISSION=table` is set in each core's `.env`, and Control's allowlist names it. Human titles and pins always win.

## Rules that hold for every switch

- A switch is a rollout control. It is never an authorization input. Authority is the Control decision, checked again at each hop.
- Every new switch defaults off in production. A ZDR org is refused at every layer: no feature, no view, no decision, no receipt.
- Accepted "on" words. Model Plane services (`switch_value_is_on`): `1`, `true`, `on`, `yes`, `enabled`, any case, surrounding space ignored. The gateway (`env_bool`): `1`, `true`, `yes`, `on`. Any other value, or unset, is off in the Model Plane. The gateway is off when unset under `APP_ENV=production`.
- Do not write a token, key, password or DSN into this file, a ticket, a shell history or a compose file. Name the variable; never its value.

## Switch list

| Switch | Read by | Set in | Repo default | Dev proof | Production | Off or unset |
|---|---|---|---|---|---|---|
| `MODEL_ACTION_ADMISSION=table` | capability-core, execution-core and model-gateway (each reads it) | each core's own `.env`; **no compose line, by decision D13** (see gap 1 below) | unset | `table` on the proof stack only, after `OWNER_LIST_SPACE_REFUSE=true`, in each core's `.env` (not on a command line) | owner decision; not set by this slice | every v2 token is refused, so no Space tool is offered; capability-core's card flip (migration 0047) is inert without it |
| `OWNER_LIST_SPACE_REFUSE` | model-gateway, execution-core | `deploy/docker-compose.yml` and `docker-compose.production.yml` (both blocks in each) | `true` in every block since W3-FIX-8 (`4fa05ba6e`); only the literal `false` opts out | `true`, and on before `table` | `true` | `false` makes the organisation-wide owner listing reachable from a Space run |
| `CONTROL_SPACE_SELF_API_ACTIONS` | user-core (`Control Plane/docker-compose.yml`, `${...:-}`) | the Control stack environment | empty | `space.thread.rename,space.thread.pin,space.thread.post_card` (D3; add `space.canvas.read,space.canvas.write` only after gap 7 closes) | empty until the owner approves a list per deployment | empty: the v2 view is refused and no Space decision is issued; removing a name stops decisions at effect time |
| `SPACE_SELF_API_SESSION_CORE` | session-core (`deploy/docker-compose.yml`, `${...:-0}`) | Model Plane stack environment | `0` | `1` | `0` | `ApplyRunThreadAction` answers `feature_disabled` |
| `EXECUTION_CORE_SPACE_SELF_API` | execution-core (`${...:-0}`) | Model Plane stack environment | `0` | `1` | `0` | no Space tool is offered and nothing is attested |
| `EXECUTION_CORE_SPACE_SELF_API_RESUMABLE` | execution-core (`${...:-0}`) | Model Plane stack environment | `0` | **`0`** | **`0` until WS10-S9 registers the resumable kind** | an `ask` row is not offered; on before WS10-S9 it would offer rows whose approval nothing resumes |
| `MODEL_GATEWAY_SPACE_SELF_API` | model-gateway (`${...:-0}`, added by WS18-S10) | Model Plane stack environment | `0` | `1` | `0`, per deployment, after the entitlement and the sweep below | the `space_self_api` grant maps to no tool profile |
| `SPACE_AGENT_SELF_API_API` | gateway (`apps/Frontend Plane/verevonv3/docker-compose.yml`, `${...:-false}`, added by WS18-S10) | Frontend stack environment | `false` | `true` | `false` | the self-API routes, the binding toggle, `features.agent_self_api` and the grant on mention turns are all absent |
| `SPACE_AGENT_CANVAS_TOOLS_API` | gateway (`SpaceFeatureFlags.agent_canvas_tools_api`, read through `space_flag`; no compose line, the gateway's own `.env` only) | Frontend stack `.env` | off when unset; on in dev only through `space_flag` | off until gap 7 closes | `off` (the canvas is pilot-only, SD-08 and SD-19) | the canvas toggle, the canvas grant and the canvas row marker are absent; a canvas-only binding gets no self-API marker |
| `MODEL_GATEWAY_SPACE_CANVAS_TOOLS` | model-gateway (`switch_value_is_on`; it honours the canvas marker only inside the `space_self_api` plus `space_canvas_tools` profile, for an agentic Space turn that is neither ZDR nor waived) | Model Plane `.env` (no compose line) | `0` | `0` until gap 7 closes | `0` | the canvas layer is off without touching self-API |
| Control decision key | user-core signs with `CONTROL_SPACE_DECISION_KEY_ID` and `CONTROL_SPACE_DECISION_PRIVATE_KEY_BASE64` (both required). session-core verifies with `CONTROL_SPACE_DECISION_PUBLIC_KEYS_JSON` (a key-id to public-key map) or the singular pair `CONTROL_SPACE_DECISION_KEY_ID` and `CONTROL_SPACE_DECISION_PUBLIC_KEY_BASE64` (passed by the compose at the session-core block) | user-core, session-core | empty in compose | the same key id on both sides | the same | a missing or stale id refuses every decision |
| Capability rows `cap.tool.space.thread.rename`, `.thread.pin`, `.thread.post_card`, `.agent.pause_self`, `.schedule.follow_up` (migration 0045; migration 0047 enables `.thread.post_card` admission, D3) | capability-core data | capability-core database | `unavailable` (`health_not_attested`) | attested only after the session-core probe (below) | same | a row is never offered while unavailable |
| Probe scope `session:run-thread-action:probe` on `service:execution-core` (session-core), and `capability:owner-action:health:write` on `service:execution-core` (capability-core) | principal file `Control Plane/config/plane-service-principals.json` (P-CFG train) | Control config | present in the file; the auth-core parity spec, `scripts/tests/principal-registry-parity.test.sh` and the golden sha re-pin are owed to the P-CFG train | granted in the dev principal set | granted by the same train | until granted the probe answers `permission_denied` and nothing is attested |

Two notes on the table. `tickets.create` is exempt from the allowlist for decisions, but the v2 Space view lists only rows that are named, so `tickets.create` appears in that view only when it is named too. The view is refused when no listed row is named; an unnamed row is skipped, not refused. Session-core verifies only the key ids it is given; the compose passes the singular pair and it passes nothing for `CONTROL_SPACE_DECISION_PUBLIC_KEYS_JSON`, so a second key needs an override.

## Order

1. Deploy execution-core with the decode tolerance (WS18-S4a) before anything that adds response fields, because the old decoder refuses unknown fields.
2. Deploy S1 and S2 with `CONTROL_SPACE_SELF_API_ACTIONS` empty and every capability row `unavailable`. The v2 view is then refused and nothing is offered.
3. Deploy S3 with `SPACE_SELF_API_SESSION_CORE=0`. The RPC answers `feature_disabled`.
4. Dev: set `CONTROL_SPACE_SELF_API_ACTIONS=space.thread.rename,space.thread.pin,space.thread.post_card`, turn Session Core and execution-core on, set `MODEL_ACTION_ADMISSION=table` in the `.env` of capability-core, execution-core and model-gateway (decision D13), wait for the probe and the owner-lane attestation, then turn on the gateway switch in the dev org only.
5. Staging, with a real second member who is a viewer and a removed member (both must be refused).
6. Production, per organisation: the Space effect entitlement (`agent_action_entitled`), then each room's binding toggle (`selfApiEnabled`). The environment switches (`MODEL_GATEWAY_SPACE_SELF_API`, `SPACE_AGENT_SELF_API_API`) are per deployment, so they go on only for a deployment whose organisations hold the entitlement and have passed the sweep.

## Dev proof stack

Run the commands from the repository root. The values shown are the dev-proof values from the table; a token, key or DSN never appears in a command here and comes from the operator's own environment. Setting a switch on the command line turns it on only where a compose line passes it through. `MODEL_ACTION_ADMISSION` has no compose line (decision D13): each core reads it from its own `.env`, so it is not set on these command lines.

```bash
# 1. The decision key the session-core verifies must match the ledger (exit 0 = match; the script prints no key value).
cd "apps/Model Plane" && scripts/check-control-decision-key.sh

# 2. Control: the one allowlist, then rebuild user-core.
cd "apps/Control Plane" && CONTROL_SPACE_SELF_API_ACTIONS=space.thread.rename,space.thread.pin,space.thread.post_card \
  scripts/run-control-plane.sh up -d --build user-core

# 3. Model Plane: switches for session-core, execution-core, model-gateway.
#    MODEL_ACTION_ADMISSION=table is not passed here (decision D13): capability-core, execution-core and model-gateway each read it from their own .env.
cd "apps/Model Plane" && SPACE_SELF_API_SESSION_CORE=1 EXECUTION_CORE_SPACE_SELF_API=1 MODEL_GATEWAY_SPACE_SELF_API=1 OWNER_LIST_SPACE_REFUSE=true \
  ./scripts/compose.sh up -d --build session-core execution-core capability-core model-gateway

# 4. Application Plane: the binding column is in the schema; restart the Convex gateway, then wait for the ready line.
docker restart convex-gateway

# 5. Frontend: the gateway routes and the feature.
cd "apps/Frontend Plane/verevonv3" && SPACE_AGENT_SELF_API_API=true docker compose up -d --build gateway

# 6. Provision the room's entitlement (the PUT replaces the whole policy; the token is the operator's, from the environment).
cd "apps/Control Plane" && CONTROL_SPACE_POLICY_TOKEN=... bash scripts/set-space-effect-policy.sh \
  --org-id <org> ... --thread-create-entitled --agent-action-entitled
```

The compose lines for `MODEL_GATEWAY_SPACE_SELF_API` and `SPACE_AGENT_SELF_API_API` are the defaults above. Setting the variable on the command line is what turns the switch on.

## Before the live proof

1. **Decision key.** Step 1 above passes, and `CONTROL_SPACE_DECISION_KEY_ID` is the same id on user-core and in session-core's key set.
2. **Proto bindings.** The proto PR runs `buf generate` from `apps/Model Plane/proto` (the set in `buf.gen.yaml`) for `sessions.proto` (`ThreadSummary.title_source` = 31, `pinned_source` = 32), `space_run.proto` (service `SpaceRunActions`) and `execution.proto` (`RunAgentRequest.tool_profile` = 28; 25 is `wide_research`). Review the diff: only those messages change. The WS18-S3 and WS18-S4 source commits do not regenerate bindings, so no Go or SPA consumer may read the new fields before this runs. `buf lint` carries pre-existing debt.
3. **Pre-enable sweep of BFF chat paths (a gate).** Before `SPACE_AGENT_SELF_API_API` is true anywhere, list every gateway handler that forwards a chat body to model-gateway (`grep -rn "model_gateway_url" "apps/Frontend Plane/verevonv3/apps/gateway/src" --include=*.rs`). Each forwarder must either run its body through `inject_mentioned_space_agent` (streams and AG-UI) or `inject_personal_thread_context` (JSON handlers), which strip a client-named `space_self_api`, or strip that field itself. A forwarder that does none of these would pass a client-named feature to model-gateway, which honours it on its own switch. Record the list and the result in the evidence. This slice did not perform the sweep.
4. **Principal scopes.** The P-CFG train has granted `session:run-thread-action:probe` to `service:execution-core` and `capability:owner-action:health:write` to `service:execution-core` on capability-core, and the auth-core parity spec and `principal-registry-parity.test.sh` pass with the re-pinned golden sha. Until then the probe answers `permission_denied` and no capability row can be attested.
5. **Probe and attestation.** execution-core attests only after `ProbeRunThreadActions` answers `verifier_configured`, `schema_present` and `flag_on` all true. It then posts to `POST /api/v1/capabilities/owner-actions/health` for `cap.tool.space.thread.rename` and `.thread.pin`. The five rows turn `available` only then. Session-core `/healthz` proves nothing about attestation.
6. **Up-proof on the session-core database.** `\d threads` shows `title_source` and `pinned_source` (both `'owner'` or `'agent'`, default `'owner'`); `\d run_thread_action_receipts` exists with its cascade on the thread (migration 0084).

## Live proof

The proof is `apps/Model Plane/scripts/tests/space-self-api-proof.sh`. It has three layers: source (layer 1), disposable Postgres (layer 2), and live phases (layer 3). Layers 1 and 2 are the same kind of check the tickets proof runs; they need no stack. Layer 3 needs the stack above and these inputs, named by variable only:

- `PROOF_GATEWAY_URL`, `PROOF_SESSION_DB_DSN` (read-only use), `PROOF_EVIDENCE_DIR` (outside the repository)
- `PROOF_ORG_ID`, `PROOF_SPACE_REF`, `PROOF_BINDING_REF`, `PROOF_AGENT_SUBJECT_ID`
- `PROOF_OWNER_TOKEN`, `PROOF_VIEWER_TOKEN` (user tokens, never service tokens)
- `PROOF_SCRIPTED_TITLE` (letters, digits and spaces)

The phases run one at a time, because two of them change the stack in between:

| Phase | Precondition | What it proves |
|---|---|---|
| `flow` (default) | switches on as above; attestation done | the rename and pin are applied as the agent's; the title and pin carry `title_source` and `pinned_source` `agent`; two digest-only receipts; ids-only events; a human title is never overwritten (`title_owned_by_human`); the switch off writes no receipt; a viewer cannot switch it on (403) |
| `tokens` | `PROOF_DECISION_TOKEN_FILE` (one token captured from the flow run, within two minutes, mode 600, outside the repo); `PROOF_SESSION_CORE_GRPC`; `PROOF_THREAD_ID` and `PROOF_RUN_ID` from `flow-ids.txt` | replay answers `REPLAYED`; a forged signature, changed arguments under the token, and another action's arguments are each refused; no receipt is written by a refused call; the claims are decoded into the evidence without the signature segment |
| `allowlist-empty` | the operator emptied `CONTROL_SPACE_SELF_API_ACTIONS` on user-core and recreated it | a mention writes no receipt with the allowlist empty. The operator records the variable's emptiness by name in the evidence; the proof cannot read it |
| `zdr` | `PROOF_ZDR_OWNER_TOKEN`, `PROOF_ZDR_SPACE_REF`, `PROOF_ZDR_BINDING_REF` for a zero-retention org | no receipt and no agent-owned override in that room |

```bash
cd "apps/Model Plane"
PROOF_EVIDENCE_DIR="<outside the repository>" PROOF_PHASES="flow" \
  bash "scripts/tests/space-self-api-proof.sh"
```

Run the source and postgres layers (`bash "scripts/tests/space-self-api-proof.sh" source`) only to check the source. That is not the proof. The proof is complete when layers 1 and 2 pass in one run and all four phases are recorded in one evidence directory. The evidence list and the ledger rows are in `space-self-api-evidence-template.md`.

## Rollback

Stop new effects at the entry first, then the executors. Rollback leaves renamed titles, pins and cards in place: they are ordinary thread data, and there is no data step.

1. Unset `CONTROL_SPACE_SELF_API_ACTIONS` and recreate user-core. Decisions and the v2 view stop at once.
2. `SPACE_AGENT_SELF_API_API=false` on the gateway. The toggle and the grant on mention turns go away.
3. `MODEL_GATEWAY_SPACE_SELF_API=0`, `EXECUTION_CORE_SPACE_SELF_API=0` (no tool is offered), `SPACE_SELF_API_SESSION_CORE=0` (`feature_disabled`).
4. Set the capability rows unavailable if the stack must not admit them.
5. Do not change `MODEL_ACTION_ADMISSION` to undo this feature alone. It also gates `tickets.create` (WS24); changing it is that owner's decision.

## ZDR

A zero-retention org gets no feature: the gateway does not add it, Control refuses the view and the decision, execution-core returns the typed refusal `zdr_durable_mutation_forbidden` before any Control call and offers no tool, and Session Core refuses a ZDR caller (no receipt, no card, no `*_source` write). Receipts, events and logs carry ids, kinds and digests only. The room canvas is refused for a zero-retention org at Control (`ValidateForCanvas`) and at the wiki-store (a token with `zdr=true` is refused on every canvas route, reads included).

## Gaps this runbook does not close

1. **`MODEL_ACTION_ADMISSION` is set in each core's own `.env`, with no compose line (decision D13).** capability-core, execution-core and model-gateway each read it from their own `.env`, and no compose line is added, so a value set on a command line reaches no container. The value is never written into this file or a compose file.
2. **Token negatives need an operator-captured token.** No route or tool in this slice captures a decision token, and minting one needs a service credential, which the proof never reads. The `tokens` phase takes a token the operator captures under the existing path.
3. **Scripted model.** How the dev stack provides a deterministic model (WS18 open question 8) is not decided. The proof assumes one that proposes the two tools on the two prompts.
4. **The sweep** (step 3 above) is not done.
5. **Owner clears a title.** A person who clears a title leaves `title_source` `'owner'`, so the agent may title the thread again. Not decided; not in the proof.
6. **Lock groups.** The P-CFG principal train, the proto regeneration and the production overlay pin for `SPACE_AGENT_SELF_API_API` belong to their own owners.
7. **Canvas tools stay dark.** No canvas row is offered until the WS19-S9 gaps close: `SPACE_CANVAS_AGENT_TOOLS`, `WIKI_STORE_URL`, the wiki-store probe and principal, and the capability attestations, plus `space.canvas.read` and `space.canvas.write` named in `CONTROL_SPACE_SELF_API_ACTIONS` (W3-DEC-CANVAS). The room canvas itself is switched by `WIKISTORE_SPACE_CANVAS` on the Data plane (default off; the purge route is not behind it).
