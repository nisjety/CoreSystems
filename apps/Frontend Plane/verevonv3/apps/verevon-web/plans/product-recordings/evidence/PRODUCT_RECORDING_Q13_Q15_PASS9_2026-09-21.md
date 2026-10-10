# Pass 9: preserve the selected ChatGPT Terra subscription

**Follow-up:** [Pass 10](PRODUCT_RECORDING_Q13_Q15_PASS10_2026-09-21.md) verifies the newly connected subscription and records Terra-only live qualification and further fixes. The missing connection described below is resolved; this pass-9 record remains historical. Campaign/project completion and semantic reliability still block recording.

**Status: routing implementation complete; live product qualification blocked by the test account's missing subscription connection. Recording remains blocked.** This pass does not close the project completion timeout, unsupported campaign suitability claims, sales cost-definition issue or broader semantic failures recorded in [pass 8](PRODUCT_RECORDING_Q13_Q15_PASS4_2026-09-20.md#pass-8-computed-numerical-and-conditional-schedule-evidence).

## Standing test constraint

From September 21, 2026, all live inference in these product acceptance tests and source-review evaluations must use **ChatGPT Terra (`gpt-5.6-terra`) through `openai-codex-subscription`**, with an active connection owned by the authenticated test user and organization. This includes tool decisions, authoring, source reviews, repairs, completion, compaction, titles and follow-ups. Do not use Claude, Verevon Balance, an API-paid substitute or another subscription model. An unavailable route stops the test; it does not authorize fallback. Historical reports retain their actual models and must not be relabelled as Terra evidence.

No real inference was invoked in this pass. The authenticated local test account, `local@verevon.dev`, returned zero integration connections. Connect ChatGPT through Verevon Integrations for that account, or configure an authorized already-connected test account before the live replay. Do not copy Codex credentials between accounts or weaken the organization's privacy policy to make a test run.

## Routing defect and implementation

The previous subscription path deliberately substituted Verevon Balance for tool decisions, cleared the subscription connection/provider during an answer handoff, and omitted them from private artifact source-review requests. Auxiliary calls also used separately selected models. Selecting Terra in the UI therefore did not establish Terra-only execution.

- Model Gateway now carries the selected model, provider and connection through decision rounds and artifact checks. Subscription tool failures stop the turn with a specific unavailable-route error. Successful subscription rounds cannot hand the answer to another model. Compaction, source judges, resampling, titles and follow-ups inherit the same route and privacy floor.
- Inference Core converts offered tool definitions into a constrained JSON proposal protocol through the subscription broker. It checks the envelope, offered names, tool choice, call count and object-shaped arguments before returning calls to the gateway. Existing gateway action contracts, authorization, audit and dispatch remain the execution authority. The broker does not execute proposed gateway tools. Invalid output fails closed. Tool proposals use non-streaming inference; ordinary answer streaming remains supported.
- Integration Core forwards an optional, bounded object output schema through the official Codex `turn/start` protocol. The model and provider reported by `thread/start` must match the requested model and OpenAI before starting the turn. Managed ChatGPT authentication remains scoped to the verified user/org connection. Native shell and web-search capabilities are disabled for these broker threads; the existing ephemeral, read-only sandbox and no-approval policy remain.
- Both the shared picker and dashboard/chat composer keep an explicit subscription selection through disconnection instead of resetting it to Balance. The composer blocks submission with a reconnect message and preserves the draft; the subscription resolver also stops an Inbox assist without a connection. Disabled subscription modes display as inactive and the source-scope label agrees with the request. A delayed or failed model catalog cannot make a connected, persisted subscription selection appear revoked.
- The shared product-test setup pins Terra and checks the request's actual wire fields before allowing a chat request. Completion frames and source-review receipts are checked for the same model/provider. The live review harness also pins the route and stops after a route violation. Transport fixtures select Terra in the UI, supply simulated transport responses and block any unhandled chat request from reaching live inference.

The native `outputSchema`, `modelProvider`, reported model/provider and configuration fields were checked against locally generated official Codex app-server schemas. This is protocol and implementation evidence, not a live subscription inference success.

## Validation

Private evidence directory: `C:\Users\ImaFernandesDaCosta\.codex\visualizations\2026\09\19\01a0b94f-be34-7aa3-8714-e90cb61181bc\q13-q15-pass9`.

- Model Gateway library: **1,254 passed, 0 failed, 1 ignored**. Six fewer tests than pass 8 reflect replacing seven assertions about the old substitution/handoff policy with one route-preservation regression; they are not lost product scenarios.
- Inference Core library: **292 passed, 0 failed**, including subscription outage behavior, model mismatch rejection and scoped tool-proposal translation. Tests use mocks; no provider quota is consumed.
- Broker/API Go packages passed, covering schema forwarding/validation, model/provider checks and existing connection-ownership enforcement.
- Frontend: **20 unit tests passed** across selection, composer and Inbox assist, including disconnected draft preservation and catalog-loading independence. Typecheck passed. Scoped ESLint has no errors and one existing reactivity warning in the unrelated dictation callback.
- All **three browser transport-recovery fixtures passed with Terra selected**, covering failed validation, interrupted responses and provider failure without a second invocation (`recovery-terra-verified.json`). These simulate failure responses and do not establish model quality. Earlier attempts, including a Vitest worker startup timeout, a Chromium network-service crash, stale Vite source and the catalog/connection race, are retained. The source-mounted frontend was restarted and its served code verified before the final browser run; the successful unit run uses one thread worker. Local authentication setup had passed earlier and the final run reused its saved session.
- The Terra-only source-review harness compiles. It has not been used to reclassify the 62 live cases in this pass.

All three local services are deployed and healthy. `runtime-final.json` records the gateway image `sha256:5d553dd5fea8f432c3fed7ed113e6b1718d359775909368991683e47f18e14ac`, inference image `sha256:4a91fd8dd9f529313518ec8470ea3424756900d98ed99a7c42ec235578f3f8d3`, and broker image `sha256:4ac588775c24a2f659dbadc2aec2f0a205788327e080c3bc813da9d6939b8d0b`. The deployed catalog exposes Terra's `tools` capability; `subscription-preflight-final.json` still records zero active connections and zero live inference calls. Source fingerprints cover 23 code/test files; all 13 fixture hashes are unchanged, 18 document links/anchors resolve and the scoped diff check passed. No fixture facts or marketing acceptance criteria were weakened. The existing recording gate remains blocked and no marketing media was approved.

## Resume order

1. Verify an active ChatGPT connection for the authenticated test account and the deployed Terra catalog entry. Run a small tool-proposal/source-review canary. Inspect every recorded model/provider before continuing.
2. Replay the complete project, campaign, sales and customer initial-plus-revision journeys on that route. Keep the pass-8 project timeout, unsupported suitability claims and cost-definition wording as regressions; do not count a changed model selection as a fix for those failures.
3. Replay the 62 labelled source-review cases using Terra only. Preserve correct controls as well as rejected failures and compare shared cases. Resolve content and checked-completion failures based on the new evidence.
4. Continue authoritative run/result recovery, five consecutive independently accepted journeys per scenario, the latency cohort, connected-data prerequisites and reviewed recording media. None of those release gates is closed by routing fixes or mock tests.
