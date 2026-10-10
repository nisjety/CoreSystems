# A6: Do mode on the ChatGPT Terra route, live proof (2026-10-01 to 2026-10-02)

Live proof of the agentic run (Do, "Utfør") on `gpt-5.6-terra` through the `openai-codex-subscription` route, run by the Playwright spec `tests/e2e/agentic-subscription.spec.ts` against the local stack. Part of [the gap closure plan](CHAT_GAP_CLOSURE_PLAN_2026-10-01.md), task A6. Evidence lines (ids, counts, codes and timings only, no message content) are in `C:\private\verevon-recordings\a6-proof\evidence.jsonl`.

## Result in one paragraph

Every probe passes on the final build. The agentic loop runs on the subscription route: every inference round of every live run was served by `openai-codex-subscription` / `gpt-5.6-terra`, no other provider appears in the inference-core logs for these runs, and cost-core has no entries for them. `plan.propose`, `knowledge_search`, `web_search` and a three-way wide-research fan-out (with durable child runs and lineage edges) all run for real. Getting there took six fixes that unit tests had not found, listed below. Connector reads (Outlook, Gmail, OneDrive, SharePoint) were not run: they need the owner to reconnect an account.

## Probes (final results)

| Probe | What | Result |
|---|---|---|
| P1 | Switch off: typed refusal, no run, `agentic` not advertised | Pass. One `agentic_subscription_disabled` error frame under HTTP 200, no run created, 0 of 7 subscription models advertise `agentic`. |
| P2 | Switch on, bogus connection id | Pass. The broker answered `subscription_connection_not_found` three times, with no Codex turn and no other provider. (The first try failed in tool parsing, defect 1.) |
| P3 | Do, knowledge and web search | Pass on the second build (22:07Z, 2026-10-01). No refusal hint, 2 source links, 6 durable steps, 3 rounds, 34.6 s. quarry-edge served the model's web searches (8 hits each) and the retrieval engine its knowledge queries. The first run passed too, but both tools had been refused (defect 2): that pass is withdrawn in the evidence file. |
| P4 | Do, a three-step plan | Pass. `plan_proposed` frame with 3 titled steps, a plan row with 3 steps persisted, 5 rounds, 37 s. |
| PK | Do, knowledge search only | Pass. The retrieval engine logged two hybrid queries for the test org, 3 rounds, 18 s. |
| P5 | Wide research, 3-item fan-out | Pass on the third build (23:17Z). 3 `subagent_attached` and 3 `subagent_stopped` frames, 0 failed, 3 durable child runs and 3 lineage edges under the parent, all three cities in the digest. 2 parent rounds and 9 child rounds, all on Terra. 55 s. |

## Defects the proof found (all fixed and deployed)

1. **`publish_social_post` had an invalid parameter schema.** Unescaped quotes inside a description made `parameters_json` invalid JSON. The subscription route turns every offered tool's schema into a prompt and refuses the whole round when one does not parse, so every agentic Terra run would have died on round 1. Test: `every_offered_tool_has_a_parameter_schema_that_is_a_json_object`.
2. **Knowledge and web search were denied by capability policy in every Do run, on any route.** `cap.retrieval.query` and `cap.tool.http` were `unavailable / health_not_attested` (nothing attested them; the ledger's `UNATTESTED_BASELINE`, 2026-08-26). A refused tool returns in about 50 ms and the model says so in plain words; the spec's first tool check counted refused steps and passed. Fix (A7): execution-core attests `cap.retrieval.query` only while the retrieval engine's `/readyz` reports Postgres and Qdrant up and its gRPC endpoint answers an unauthenticated call with `Unauthenticated`; it attests `cap.tool.http` only while quarry-edge `/ready` answers and both Quarry credentials can be minted (it never calls the metered `/v1/search`). The spec now also fails when the final answer reads as a refusal or cites no source.
3. **Execution-core had no Quarry search scopes.** Its principal held only `browser:execute` and `scrape:write` on the Quarry audience, so Auth Core refused the web credential (403). Granted on 2026-10-02 at the owner's request: `search:read` and `extract:read`, Quarry audience only (`apps/Control Plane/config/plane-service-principals.json`, pinned by the policy contract spec; seven mutation checks each caught). The honest probe flipped `cap.tool.http` to available on its first heartbeat after the auth service was recreated.
4. **Every delegated child run failed to register (`PermissionDenied`).** Execution-core started the child run with its service-level terminalize token; session-core's `start_managed_run` requires a user-bound credential. Child runs, lineage edges and the stream's `subagent_attached` and `subagent_stopped` frames had therefore never worked live (0 child runs in the whole database before this fix). Two more links would have failed right behind it: the child's answer record (service token without `session:write`) and the failed-child receipt (`subagent_failed` is not in session-core's failure vocabulary). Fixed in `execution-core/src/runtime_loop/agent.rs`: the child starts under the run's own user bearer, the answer is recorded under it, failures settle as `execution_failed`, and zero-retention runs register nothing. The unit tests missed it because their double accepted the service token; a new test double (`session_core_rules_double.rs`) applies session-core's real credential rules, and reverting any part of the fix fails the contract tests.
5. **Wide-research children could lose their findings.** Two of three children in the first fan-out spent their last round on more tool calls and ended exhausted, with their gathered findings dropped. The last round of a child's budget (budget of 2 or more) now withholds tools and asks it to answer from what it has; the result is labelled "[Partial result: ...]". No global limit was raised.
6. **Weak assertions in the first spec** (found by its reviewer): one chat request per probe is enforced by a route gate, a failed fan-out child fails P5, and P1 and P2 can only ever send a placeholder connection.

## Timing and cost

- Per inference round on Terra: 5 to 8 s, about 10,200 input tokens per round (the tool list is in every prompt) and no prompt-cache reads.
- **A3 check (done frame after the run finishes):** execution-core finalized at 12:43:33.112 and the done frame arrived at 12:43:33.416 (0.30 s); PK 14:13:31.202 to 14:13:31.471 (0.27 s). The old 25 s idle tail is gone.
- Cost-core: no ledger rows for any run id. Integration Core's broker log shows one successful Codex call per round.
- Free probes before any quota: P1, P2, the registry query and the dependency readiness endpoints cost nothing; P3 to P5 and PK are the only ones that use the subscription.

## Other live checks on the same build

- **I3, redeploy-safe turns:** a model-gateway restart during a Terra answer logged `draining ... active:1` then `drained ... waited_ms:29451`; the client got every chunk and `done`, 5,226 characters were saved and the run completed. An idle restart takes 2.2 s.
- **E3, planner action JSON on Terra:** the plain schema is rejected by Codex (`turn_failed`); the strict schema was accepted with 4 of 4 replies exact and bound to the right snapshot ids. E4 must adopt `ACTION_SCHEMA_STRICT` on the subscription route.
- **J14, answer line:** a Terra Ask answer shows "Behandlet: global, ingen EU- eller ZDR-garanti" with a hover text saying the answer may have been processed outside the EU/EEA and zero retention is not promised (server-stamped values only).

## Findings to follow up (not caused by this work)

- **quarry-edge search is degraded.** While serving the web searches it logged `hybrid: vector retrieval failed; lexical-only (retrieve returned status 403)` and `rerank: model plane failed; keeping original order (Auth Core service-token request returned 403)`. Quarry's own service principals lack scopes for the retrieval engine and the model plane, so search runs lexical-only and unranked. It works, but at lower quality.
- **Zero-retention runs get no child runs.** They run delegation normally but record no lineage, stop event or stored child answer. Registering ZDR children as metadata-only runs is a posture change that needs an owner ruling.
- **Paid paths outside chat:** contextual retrieval (`gpt-4o-mini` on a paid Azure deployment) is on in the Data Plane `.env`; chat uploads do not reach it (attachments are inlined), but SharePoint syncs and Knowledge-page adds do (last fired 2026-09-28). `graph-index` also defaults to `gpt-4o-mini` extraction and has no cost-ledger emission (not confirmed live).
- Child runs' 15-minute deadline is never heartbeated (fine at current budgets); a partial-result label also appears on a last-round answer that would have been complete.

## State left on the stack

All planes were deployed from the working tree on 2026-10-01 and 2026-10-02: Model Plane (several builds, last 23:16Z), Control Plane (auth-service and user-service), Ingestion Plane (integration-api, imports-api, email worker) and Frontend Plane (gateway and frontend). `AGENTIC_SUBSCRIPTION_ENABLED=true` (model-gateway, execution-core, inference-core), `AGENTIC_SUBSCRIPTION_ALLOW_EFFECTS=false`, `AGENTIC_SUBSCRIPTION_MAX_ROUNDS=14`, `WIDE_RESEARCH_FAN_OUT_CONCURRENCY=2` and `EXECUTION_RUN_WALL_CLOCK_SECS=240` were passed in the launcher's environment only. The compose defaults are still off, so the next launcher run without them switches Do on Terra back off. Mail-to-task stays off (its principals and credentials exist; nothing presents them).
