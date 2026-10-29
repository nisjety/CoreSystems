# Q05–Q07 execution record

Date: 2026-09-19. Work in progress; this is not a recording/release sign-off.

Current status: Q06's implemented source-isolation, edit/delete/recall and background-review checks pass, including a full extraction-cycle observation. Q05 has passing sales/project journeys but still needs campaign sign-off and five consecutive passes per scenario. Q07's scoped empty-inbox and quote-provenance contracts pass; Visma, populated knowledge/customer evidence and FedEx authorization remain unverified. No marketing media was recorded or published.

## Implemented changes

- Maintained browser coverage for sales, campaign and project fixtures, their exact REGI follow-ups, artifact versions and server reload. Sales expectations are calculated independently from the CSV. Checks include per-piece word limits, address style, prohibited claims, unchanged campaign pieces and tentative project dates.
- A conversation-only source selection is carried from dashboard/chat to Model Gateway and persisted by Session Core. It excludes workspace context assembly, personal memory, direct retrieval, learned skills and external tools. Only this conversation's artifacts/history and networkless calculation are offered. The tool loop rejects calls outside its offered set. Session Core rejects widening and relabelling existing history. Memory extraction and skill review skip isolated conversations. Source scope does not change retention.
- Fixed background review's missing JSON response contract, forced schema-bearing tool response, bounded validation, refusal/truncation diagnostics and delayed retries (30 seconds, 2 minutes, 10 minutes, 30 minutes). Exhausted records retain their original JetStream sequence for targeted recovery.
- Disabled duplicate semantic extraction in agent-memory-server for memories already extracted by Session Core. The bridge writes `discrete_memory_extracted: "t"` so canonical IDs remain editable/deletable.
- Deterministic memory extraction now excludes attachment blocks, matching the model-driven extractor's input rule. Memory editing fails visibly if full content cannot be loaded, rather than saving a truncated preview.
- Each shipping quote and recommendation now carries configured environment, mock status, quote timestamp and package count; Execution Core preserves these fields when rendering model context. Quote/recommendation APIs reject unsupported extra fields instead of silently pricing one parcel from a multi-parcel request. Recommendations cannot name a service that was not quoted.
- Queued messages inherit the active run's source scope for durable append. Listing an empty artifact collection returns an empty result; reading a specific missing artifact remains an error. The model is offered artifact read/update only once this conversation has a deliverable, recomputed each tool round; the first create immediately enables them. This prevents prompting the model to guess an artifact ID before creation.
- A delayed memory check found a new record referencing the deleted fixture after the immediate browser pass. Migration 0039 adds a content-free per-user extraction cutoff and forgotten-ID records. Manual deletion (also used by correction) takes precedence over pending/in-flight extraction; late semantic mirrors are suppressed on list, search and chat-context reads and cleaned up after indexing. GDPR erasure includes these control rows and mirror IDs. This deliberately discards unprocessed pre-edit extraction windows while preserving existing unrelated memories and allowing new explicit learning. The final live test passed after observing absence for another 330 seconds, spanning the 15:04:29 UTC Dreaming cycle.

## Attempts retained

| Attempt | Outcome |
| --- | --- |
| scenarios-01 | Three initial journeys run. Sales assertion incorrectly required two-decimal margins although the answer rounded correctly to one decimal; corrected the oracle. Campaign/project passed the initial basic checks but manual review found unsupported stability/comparison claims, singular address, a 91-word revision, an unestimated repair deadline and a target pilot date marked as decided. Strengthened checks and response guidance; these are not accepted passes. |
| acceptance-02 | Failed before meaningful scenario verification: a new Session Core query incorrectly assumed threads had a metadata column; replaced with migration 0038 and a dedicated typed source_scope column. The running Vite process also served its earlier cached composer, so the source selector was absent. Stopped the remaining run after confirming both causes. No model result from this attempt is a quality pass. |
| memory-03 | Full-content editor was visible, but the test's text-filtered row no longer matched once the value moved into a textarea. Corrected the locator. |
| acceptance-04 | Source isolation and project journey passed. Memory edit/new-thread recall worked, but the test selected the old forget-button label after confirmation opened; fixed the selector. Sales arithmetic passed; its concise-note count incorrectly included report metadata. Campaign genuinely added unsupported stability claims. |
| memory-connected-05 | Immediate memory edit/delete/new-thread recall passed in 1.1 minutes. Q07 failed during browser-context setup, before a business request. A separate check at 14:43:36 UTC found one new record referencing the deleted memory fixture, so the immediate pass is not a durable-deletion sign-off. Retained `memory-cleanup-before-guard.json` from the tool output; removed only identified QA fixture records and three previously confirmed Q04-derived memories. |
| acceptance-06 | Sales and Q07 read/quote contract checks passed. Campaign listed an empty artifact collection and received an erroneous tool failure; manual review also found singular address mixed into the requested plural copy. Project used a clearly proposed repair buffer and warned no time was allocated; the oracle accepted too few equivalent phrasings and was corrected. No repeatability sign-off. |
| memory-delayed-07 | Passed edit → reload → new-thread corrected recall → delete → new-thread absence → 330 seconds of continued absence. Total browser check 6.4 minutes. Logs confirm the background cycle ran during observation and skipped isolated campaign/project sources. |
| scenarios-07 | Project brief/follow-up/reload passed again. Campaign false failure counted each post's separate internal source note as customer-facing text. Corrected the parser and ran it against the saved artifact: post counts 77/74/69, email 102; all mechanical campaign checks passed offline. This does not retroactively count as a full browser journey; the original run stopped before its revision. Manual review still notes incorrect self-reported word counts and a misleading “Mottaker: Fjordform” label. |
| campaign-08 | Failed after the model guessed an artifact ID before creating its first deliverable. This was a real invalid read, not the previously fixed empty-list case. Changed per-round tool availability to follow actual artifact state and strengthened guidance to reuse exactly validated prose. |
| campaign-09 | Per-round artifact availability removed the invalid-read failure. The real answer still mixed singular “du” into the required “dere” address. Manual review also found unsupported claims that the lamp suits most desks and a lower light level does not disturb colleagues. The test correctly failed before the revision; no campaign sign-off. This is a remaining content-quality problem, not a reason to relax the oracle or accept a selected lucky run. |

Evidence directory: `C:/Users/ImaFernandesDaCosta/.codex/visualizations/2026/09/19/01a0b94f-be34-7aa3-8714-e90cb61181bc/q05-q07`.

## Connected-data baseline

Authenticated Verevon workspace reads returned no registered MCP servers, one inbox with zero conversations, and zero indexed documents. This does not establish Visma, customer-message or knowledge-quality readiness. Bring and UPS reported production configuration; DHL reported sandbox; FedEx reported sandbox with a failed latest quote. These are carrier-list metadata, not a verified quote for order I1/12475.

The hybrid fixture describes one representative 66 kg parcel out of five. The current shipping contract prices one parcel. It cannot establish the total shipment price, service eligibility, handling requirements or delivery commitment for that order. Do not multiply a single quote into a supposedly verified total.

The 14:43 UTC authenticated one-parcel probe returned 13 quotes: 10 production-configured Bring/UPS options and three DHL sandbox options, with timestamps and package count 1. FedEx returned 403 (Rates API authorization); it is not verified. The unsupported multi-parcel request was rejected with HTTP 400. Verevon itself invoked `inbox_search` and reported the empty workspace without inventing a customer request. These checks passed in 31 seconds. They do not establish Visma connectivity or a quote for the hybrid order.

The Visma server URL and intended connected workspace were requested. OAuth credentials were not requested, copied or fabricated. The old Aquatiq knowledge documents are not present in this test workspace; no unrelated workspace documents were deleted or recrawled.

## Targeted learning recovery

After fixing the cause, an operator with an existing authorized NATS context can retrieve exactly the `stream_sequence` logged for the exhausted event:

```sh
nats stream get MODEL_PLANE_RUN_EVENTS <sequence> --raw > retained-event.json
```

Keep that file private; it is an original event envelope. Run the capability-core binary with its normal service credentials and backend addresses:

```sh
service --replay-learning-event /private/path/retained-event.json
```

This reviews one original event through the same retention, source-scope and skill-provenance gates. It neither resets the durable consumer nor widens its NATS permissions. It logs the run ID and persisted count. A successful recovery can legitimately persist zero skills. Remove the private recovery file when finished. Historical failed runs must not be described as recovered until their exact sequence has completed this path.

## Verification status

- Frontend typecheck and production build passed. Targeted lint still reports three existing Markdown/Mermaid/KaTeX `innerHTML` errors and reactivity warnings; it is not green.
- Earlier focused frontend tests: 157 passed; ChatPanels including failed-full-memory-read coverage passed 66 tests with the threads pool, and its final isolated rerun also passed 66/66. A later aggregate attempt passed 85 tests in three files but hit a worker startup timeout on ChatPanels; the failed aggregate is retained and not counted as green.
- Capability Core learning/reviewer/session-review/command tests passed.
- Letta bridge client/server tests passed.
- Shipping Core `go test ./...` passed, including recommendation provenance/service validation. Execution Core shipping tests: 7 passed.
- Model Gateway full suite passed before the final empty-artifact adjustment; queued-input tests passed 11/11 after adding inherited scope. Session Core full suite passed 348 tests with 22 ignored before the deletion-cutoff change.
- Disposable PostgreSQL tests passed for durable source scope, rejected widening, suppressed learning, and the new pending/in-flight forgotten-memory regression. Final Session Core suite after mirror cleanup/GDPR integration passed 348 tests, with 23 integration tests ignored by default.
- A post-fix real run emitted `learning review completed` with zero persisted skills (2026-09-19 14:16:54 UTC), instead of the previous repeated parse errors.
- After the Session Core restart, stream sequence 225 encountered a transient gRPC connection failure at 15:00:13 UTC. The consumer scheduled delayed redelivery and completed the same run (`01M2X2WZ2R18RVHDW20357KZTN`) at 15:00:46 UTC with zero saved skills. This is observed recovery of that event, not clearance of every historical exhausted event.
- The final empty-artifact listing regression passed. Both disposable PostgreSQL integration tests passed again after mirror cleanup/GDPR changes. The disposable database container was removed afterward.
- Final artifact-focused Model Gateway tests passed 11/11, including availability before/after a real create and isolation between thread registries. The final deployed Model Gateway includes this state-dependent tool offering; campaign-09 verifies the original invalid-read symptom no longer occurred in that run.
- Final deployed image IDs and start times are in `deployed-images-final.txt`; they are built from the dirty working tree and are not equivalent to a clean-commit release certification.

Observed completion times in acceptance-06 (one sample per turn, not p95): sales initial 95.2 seconds and revision 32.4 seconds; campaign initial 202.6 seconds; project initial 120.5 seconds. These exceed the proposed initial-response targets and leave Q08 open. Some verification/build activity ran concurrently, so repeat latency measurements on a quiet stable stack before attributing all delay to the serving path.

Five consecutive fresh-thread passes per scenario, connected business-data proof, Q08 performance/recovery and Q09 recording remain separate gates.

## Additional infrastructure finding

Session Core retains audit events in its outbox but repeatedly reports `no stream found for given subject`. The configured `audit-extra-nats-provisioner` was absent. A build/start of the existing provisioner reported Model NATS authorization failure and Application stream-subject overlap. It did not repair the topology. Removed the newly created exited provisioner container; did not rotate credentials, delete existing streams or reset consumers. This remains an infrastructure readiness issue requiring reconciliation of the deployed credentials/topology with the existing owning provisioner.

## Next acceptance work

1. Repair campaign conformance before further acceptance batches. Prompt guidance and voluntary word counting have not reliably enforced the brief. Add a bounded review of the **actual final artifact** against the user-requested source/format/style constraints, with deterministic checks for measurable requirements and explicit support for factual claims; repair failures before reporting completion. Validate this through normal Verevon execution, without changing the fixed user prompt or hiding errors in the recording harness. Then finish the exact campaign revision and inspect recipient labels and self-reported validation numbers. Keep the original failed attempts; parser corrections do not turn an interrupted journey into a pass.
2. Obtain five consecutive fresh-thread passes for each scenario on one recorded image/model configuration, including customer reply again after shared model guidance changes. The current default route observed in scenario traces is `verevon-balance` resolving to `claude-sonnet-4-6`.
3. Use the intended Verevon workspace and its Visma MCP URL/authorization to prove real reads; add genuine versioned knowledge and an authorized customer conversation. Fix FedEx Rates API authorization and reconcile the existing audit provisioner's deployed credentials/stream ownership.
4. Run Q08 on a quiet stable stack: first meaningful content, end-to-end latency, reconnect/replay, stop/continue/regenerate and mobile/source/artifact presentation. Source attachments currently occupy prominent preview space, so include whether the finished deliverable is immediately easy to inspect.
5. Start Q09 only after the factual, durability, trust and performance gates are met. A green browser test alone is not permission to describe the output as flawless.
