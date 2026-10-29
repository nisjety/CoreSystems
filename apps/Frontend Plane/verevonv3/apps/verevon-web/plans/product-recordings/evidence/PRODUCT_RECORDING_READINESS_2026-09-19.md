# Verevon product recording readiness and execution plan

**Pass 13 / current checkpoint (September 22):** All live inference remains exclusively **ChatGPT Terra (`gpt-5.6-terra`) via `openai-codex-subscription`**. Private source rechecks now require explicit reconfirmation, invalidate edited sections and reference-dependent passages, and retain complete document/source context. Artifact completion reuses its title and skips optional title/suggestion inference. Backend tests passed **1,273**; final live recheck controls **6/6**; sales-oracle controls **55**, scoped lint and typecheck passed. The full first-review evaluation scored **90/94**, but the shared cohort remains **82/86**, with one false acceptance, two false rejections and one timeout. Customer and sales completed fresh initial/revision/reload journeys (**110.047 s / 159.547 s** full browser tests); the first sales attempt exposed a cross-sentence test-parser bug, now corrected with its failed record retained. Campaign/project still timed out (**122.890 s / 139.593 s** initial turns), publishing no artifact. [The pass-13 record](PRODUCT_RECORDING_Q13_Q15_PASS13_2026-09-22.md) distinguishes the earlier v7 evaluation, failed recheck control, final v8 safeguards and all browser results. **Release and recording remain blocked.**

**September 20 implementation update:** The first Q11/Q12 slices add exact direct-answer word-range checks and artifact revision receipts. See [the execution record](PRODUCT_RECORDING_Q11_Q12_2026-09-20.md) and [the remaining implementation sequence](PRODUCT_RECORDING_NEXT_STEPS_2026-09-20.md). These scoped checks do not close the full scenario correctness or recording gates below.

**Q13/Q14 continuation:** [The next execution record](PRODUCT_RECORDING_Q13_Q14_2026-09-20.md) covers source review before publication, exact preservation of supported revision sections, local word counting and cancellation during checking. Failed attempts are retained; current recording approval remains blocked.

**Historical pass-8 checkpoint (September 21, before Terra-only routing):** [Pass 8: computed numerical and conditional schedule evidence](PRODUCT_RECORDING_Q13_Q15_PASS4_2026-09-20.md#pass-8-computed-numerical-and-conditional-schedule-evidence) adds local checks for supported numerical claims and conditional task-chain calculations, retaining source spans, possible approvals and unknown repair durations. The library suite passed 1,260 tests and the eight local replay/control cases passed. Source review scored 54/62, with four false acceptances and four false rejections; the shared 58-case score fell from 51 to 50. The browser run has six passes and two failures: customer completes correctly; sales arithmetic/revision/reload pass but the initial cost definition needs editing; campaign still invents suitability claims; project planning times out after a further rewrite and publishes no artifact. The campaign oracle's date-metadata parsing was corrected and replayed with valid/invalid controls. Checked task completion, broader claim/evidence binding and authoritative recovery remain blockers. All 13 fixture hashes are unchanged; no recording gate was opened.

Reviewed: 2026-09-19. Scope: the four product scenarios for `apps/verevon-web`, their real Verevon execution paths, and the five requested audits. This is an assessment and task plan, not a new certification of the scenarios.

Execution update, 2026-09-19: **Q01–Q04 are implemented and locally verified.** See [the first execution record](PRODUCT_RECORDING_Q01_Q04_2026-09-19.md) for fixes, retained failed attempts, the final 6/6 browser pass and live compaction/artifact restart evidence. [Q05–Q07 execution](PRODUCT_RECORDING_Q05_Q07_2026-09-19.md) now records scenario coverage, enforced source isolation, verified memory correction/forgetting across a background cycle, learning response/retry fixes, and connected read/quote provenance checks. Q05 repeatability and campaign review, Q07 real business-data prerequisites, infrastructure audit delivery, Q08 performance/recovery and Q09 recording remain open. The assessment below preserves the original baseline.

**Decision: stabilize and prove the four complete customer journeys before recording them.** Use the existing fictional scenarios as repeatable acceptance cases, then prove connected-data variants separately. Make the customer reply the first complete journey and main film. The immediate work is reliable inputs, trustworthy outputs, durable follow-up, and measurable execution; broad action parity, A2A, and new memory features are later work unless a selected journey needs them.

**Q08–Q10 update:** [The current execution record](PRODUCT_RECORDING_Q08_Q10_2026-09-19.md) covers corrected truncated-stream/retry behavior, live stop/continue/regenerate and reload recovery, mobile rendering, three measured latency cohorts, controlled media playback preparation and reconciled manifests. Customer-copy review reopened correctness for unsupported notification and pickup claims; earlier browser passes are not current editorial approval. The final September 19 small-workload cohort measured acknowledgement p95 **0.167 s**, first-answer p95 **3.684 s** and zero tool calls, but **0/20** responses met its word range. This count was corrected on September 20 using authoritative saved UTF-8 text; the earlier 6/20 came from misdecoded network captures. The first two cohorts and their failures remain recorded. Q08 release proof and Q09 recording are still gated by final-output conformance and complete scenario evidence. Historical September 14 manifest approvals are retained separately from current status.

The intended experience is: give Verevon a task once, see useful progress, inspect a correct and usable result, refine it naturally, and return to the same work later. A polished answer that loses its attachment, changes a number during revision, or cannot be reopened does not pass.

## 1. Evidence and limits of this review

Read the current-checkout versions of:

- [Learning edge features](../../system-audits/LEARNING_EDGE_FEATURES_2026-09-17.md), including sections 5–6.
- [Cache and compaction audit](../../system-audits/CACHE_COMPACTION_AUDIT_2026-09-17.md), including section 7.
- [AI-first creed status](../../system-audits/AI_FIRST_CREED_STATUS_2026-08-28.md), including both September 17 updates.
- [Chat workspace implementation plan](../../chat-workspace/VEREVON_CHAT_WORKSPACE_IMPLEMENTATION_PLAN.md), including sections 21–23 and its product invariants/release gates.
- [Chat parity audit](../../system-audits/CHAT_PARITY_AUDIT_2026-09-15.md), including section 0.2 at the end.
- The three recording folders, source packs, prompts, control notes, direction notes, manifests, and run logs.

Cross-checked relevant Frontend, Model, and Ingestion source, current container metadata, bounded capability-core logs, saved Playwright failures, and selected tests. Did not rerun the four live business scenarios, redeploy services, modify business records, or record/publish media in this assessment.

Fresh checks:

| Check | Result | What it establishes |
|---|---|---|
| Five focused Vitest files | **68/68 tests passed**, 5/5 files, 72.02 seconds | Existing regression coverage for pending attachments, long paste, stopped-turn state, continuation, and Markdown is green. This does not prove browser or provider behavior. |
| Action-surface contract check | **1/1 passed** | The 167 registered action IDs match gateway dispatchers. It does not certify that every dispatcher is deployed or AI-executable. |
| Model-executable action allowlist | **8 IDs** in current source | Narrow governed mutation admission is still deliberate. Built-in read/research/artifact tools are separate; this is not a claim that Verevon can do only eight things. |
| Container snapshot | **107 running; 0 marked unhealthy** | Availability snapshot only. Selected Model/Frontend containers were created September 17, later than several audit entries. Old “needs deployment” notes require a live check before rebuilding. |
| Session-core runtime flag | `MEMORY_GROUNDING_ENABLED=0` | Reconciliation is implemented but disabled in this environment. |
| Capability-core previous 24-hour logs | One consumer-bound event; zero matches for the selected review/credential/ACL error strings | The old JSON failure was not observed in this window. Quiet logs do not prove a successful review or clearance of exhausted messages. |
| Saved Playwright `.last-run.json` | **Failed; 8 failed test IDs**, September 17 | The suite left running in workspace-plan section 23 did not finish green. Inspected error contexts show onboarding instead of chat and `ERR_EMPTY_RESPONSE`. Diagnose setup/availability before calling these eight chat defects. |
| Fictional CSV recalculation | Revenue 360,000 both weeks; profit 123,000 → 124,680; margin 34.1667% → 34.6333% | Independently confirms the numeric oracle. These are calculations from supplied fixtures, not a fresh Verevon answer. |
| Shipping carrier-list probe | HTTP 401 without authentication | No current authenticated carrier/environment verdict was obtained. Do not infer that mock quotes are fixed live. |

## 2. Reconciliation of the older plans

| Area | Current conclusion | Next useful action |
|---|---|---|
| Chat parity | Continue, KaTeX/Mermaid, long paste, and forget controls are implemented. The focused tests pass. The exact stop → regenerate → Resultat observation was incomplete in the last audit. | Exercise those transitions in the browser; do not implement the same features again. |
| Attachment URL lifetime | The old non-image `blob:` bug is already fixed in `pending-chat-launch.ts`: all blob URLs are converted. | Fix remaining silent-failure behavior and prove actual file use from both dashboard and chat. |
| Memory correction | PATCH exists in `apps/gateway/src/domains/memory.rs`; the running gateway was created after that file's recorded modification time. | Reproduce authenticated edit and independent reload. The old 405 is historical, not a current confirmed deployment defect. |
| Learning | RRF recall and non-destructive grounding exist; grounding is disabled. Skill-review response handling and immediate negative acknowledgements still need work. | Validate current review outcome/retry behavior; enable grounding only after scoped write/read/delete and isolation proof. |
| Cache/compaction | Telemetry and native API request wiring exist, but source review found important gaps below. | Correct the fallback decision and native summary round-trip before treating long conversations as complete. |
| AI-first creed | 167 action contracts and 8 admitted mutation IDs confirmed; the old 83.5% uses a historical 200-call-site denominator. Inbox read tools now exist in Model Gateway. | Measure the actual capabilities needed by these scenarios. Do not widen the entire mutation allowlist to hit a percentage. |
| Workspace plan | Authentication was unblocked historically, but saved browser results failed before reaching the intended surfaces. | Provision a stable, onboarded acceptance workspace and rerun the suite. Add scenario and replay coverage beyond its current base-chat/accessibility scope. |
| Hybrid shipping | Source now drops mock adapters when real adapters exist, unless explicitly overridden; mock-only fallback remains. | Verify deployed fleet and production versus sandbox provenance. “Real adapter” alone is not production proof. |
| Hybrid inbox | `inbox_search` and `inbox_get_conversation` definitions and dispatch exist in `model-gateway/src/tool_loop.rs`. | Test authorized scoped reads in Verevon itself. The September 14 “no inbox tools” statement is stale. |

The repository orientation documents contain older July runtime warnings. They remain useful for ownership boundaries, but are not current availability evidence.

## 3. Source-confirmed defects and important unproven paths

### R-01 — Attachment failures can still silently remove the user's evidence

`src/features/chat/lib/pending-chat-launch.ts` catches blob conversion errors and continues without that attachment. Its test explicitly expects the attachment to disappear. The function returns success, and `DashboardComposer.tsx` then resets the draft and navigates.

Separately, the base64 payload goes through `src/shared/session/client-storage.ts`, which catches storage errors and reports no failure to the caller. Large files or unavailable/full storage can therefore lose a pending launch while the composer proceeds. Both local and session storage are attempted; temporary-chat retention needs to be part of this correction.

**Required behavior:** retain the draft and files until handoff succeeds; tell the user which file failed and provide retry/remove choices; enforce a bounded handoff strategy appropriate to the retention policy. Never answer as though an omitted attachment was received. Cover unreadable files, full/unavailable storage, multi-file input, duplicate filenames, and temporary chat. The conversion fix alone is insufficient.

### R-02 — Local compaction is disabled more broadly than native compaction is available

`model-gateway/src/compaction.rs:184` skips local head compaction for every model whose name starts with `claude`; tier-one tool clearing uses the same family check. In contrast, `inference-core/src/provider/anthropic.rs:560` excludes Haiku from native head compaction, and the inference-side `ANTHROPIC_NATIVE_CONTEXT_MANAGEMENT` flag can disable native processing entirely. The gateway decision reads neither condition. The flag is also absent from the inspected Compose wiring.

**Consequences established by source:** Haiku skips both head-summary implementations; disabling the native path does not restore the advertised local fallback. Actual overflow or quality degradation has not been reproduced in this review.

**Required behavior:** decide tool clearing and head summarization separately from verified serving capabilities, account for disabled native processing and provider fallback, and make the rollback switch operable. Test Haiku, a supported Sonnet/Opus deployment, non-Anthropic, native-off, and cross-provider fallback.

### R-03 — Native compaction summaries have no round-trip representation

Anthropic's [compaction contract](https://platform.claude.com/docs/en/build-with-claude/compaction#passing-compaction-blocks-back) requires carrying the returned compaction block into subsequent requests. Current `anthropic.rs:837` collects text/tool results without retaining the compaction block; the streaming handler has no compaction-block assembly. `proto/model_plane/v1/inference.proto:188` represents a chat message as role/content/name, with no typed native context state.

**Inference from that contract and the inspected code:** request acceptance and logged `applied_edits` do not establish reusable multi-turn native compaction. The current adapter cannot round-trip the returned summary in its required form. Repeated work/cost and context-continuity behavior need a multi-turn reproduction.

**Required behavior:** carry provider context state through an explicit typed contract, retain it under the thread's policy, reconstruct it after reload/restart, and protect pins/constraints. Keep compaction content separate from the visible answer. Prove a follow-up after actual compaction, not only an HTTP 200 below the threshold.

### R-04 — Test readiness is incomplete even when login succeeds

Saved test failures show the seeded user on onboarding step 2 rather than `/chat`; other cases fail with an empty response from port 5173. The application shell still has the selector the tests use.

**Required behavior:** establish workspace membership and completed onboarding using owning application contracts; assert readiness before the suite; prevent a deployment/restart during an acceptance run; preserve traces. Do not bypass production onboarding/auth checks or increase timeouts as the first response.

### R-05 — Skill learning has an unresolved response/retry boundary

The latest learning audit reports model replies without a JSON object and exhausted retries. Current `internal/llmreviewer/reviewer.go` still requests a 2,048-token response and directly parses `resp.GetContent()`. `internal/sessionreview/consumer.go:137` still calls immediate `msg.Nak()` on failure. The earlier incident was not reproduced in this review.

**Required behavior:** inspect response type, termination reason, and bounded/redacted diagnostics; validate schema; distinguish refusal/truncation/permanent errors from retryable outages; add delayed retry and an inspectable recovery path for exhausted events. Do not routinely log raw customer transcripts or weaken retention/approval gates to make the pipeline pass.

## 4. Canonical scenarios and the result that should impress a buyer

Use `apps/verevon-web/plans/product-recordings` as the maintained fictional acceptance pack. The dated `verevon-produktopptak-2026-09-14/product-recordings` folder is an initial snapshot: its manifest and run log differ, while matching other files have identical hashes. Its manifest still says `prepared`; the maintained manifest records the September 14 verified runs. The maintained README's “none run yet” sentence is also stale.

Keep `product-recordings-hybrid` as an integration test pack with explicit provenance. Real ERP values pasted into a source file prove analysis of supplied data, not that Verevon fetched them. A scenario using fictional notes/routines remains hybrid even when its ERP connection works.

| Journey | Hard acceptance criteria | Useful finishing touch |
|---|---|---|
| 01 Customer reply — first and main film | ≤150-word Norwegian draft; 9 packed/3 pending; warehouse arrival is not customer delivery; no shipment or delivery promise; current K3 wins over expired K4; separate internal citations; no send/order mutation. Follow the exact REGI revision. | A warm draft ready to use, an inspectable source for the important statement, and the next clarification clearly identified. The shorter follow-up updates the same artifact. |
| 02 Sales report | CSV and notes both read; totals match the oracle; weighted margin and percentage points correct; unsigned offers excluded; hypotheses labelled; follow-up ≤120 words with unchanged numbers. | Clear comparison table, three prioritized actions with proposed owners, and a concise management note. Add a chart/export only when genuinely generated and checked. |
| 03 Campaign | Three distinct posts, each 60–90 words; email body 80–120 words plus subject/preview; correct dates/tone/facts; no prohibited claims, invented URL or testimonial; internal evidence separate; remains draft. | Four usable pieces in one coherent artifact. Revising September 23 changes that piece and preserves the other content. |
| 04 Project plan | Owners, dates, dependencies and completion criteria; September 22 invitation conflict found; September 24 is conditional earliest, not confirmed; technical absence and unestimated rework visible; no invented substitute, meetings or notifications. | A readable plan plus a ≤100-word status identifying the first decision and two main risks. A task table is described as a plan until actual task creation is verified. |

The fixture date stays **2026-09-14** so the existing oracle is valid. A current-data recording must use a separately versioned prompt/source pack and recalculated dates; do not silently mix today's context into this fixture.

## 5. Ordered work queue

Effort is a planning estimate, not a delivery commitment: S = up to about a focused day; M = several days; L = several days to a week or more across planes. Scores use `(impact + risk) × (6 − effort)` on 1–5 scales. Dependencies and customer-blocking correctness take precedence over the score.

| Order / ID | Work and owner | Impact / risk / effort; score | Why it matters / completion evidence |
|---|---|---|---|
| 1 / Q01 | Establish an onboarded acceptance workspace, baseline build identity, source hashes, provider route and fresh browser evidence. Frontend + Control/Application. **S–M** | 5 / 5 / 2; **40** | Every later pass/fail depends on this. Green setup and actual `/chat`; no stack changes during the run; record deployed image IDs separately from working-tree state. |
| 2 / Q02 | Correct attachment handoff failure semantics (R-01), then prove ingestion readiness for supported PDF/DOCX/CSV/MD inputs. Frontend + gateway + Ingestion/Data where needed. **M** | 5 / 5 / 3; **30** | All four tasks depend on their evidence reaching the model. Dashboard and chat uploads must survive navigation/reload or fail visibly with the draft intact. No silent truncation or “upload accepted” treated as content ready. |
| 3 / Q03 | Fix compaction capability selection and summary round-trip (R-02/R-03). Model. **M–L** | 5 / 5 / 4; **20** | Long tasks must retain decisions and remain fast. Cross-provider tests, native-off rollback, threshold-crossing follow-up, pins and restart recovery. Cache telemetry must reflect real reuse. |
| 4 / Q04 | Run 01 end to end; fix each reproduced defect; verify artifact identity/version/reload and source-to-answer consistency. Frontend + Model. **M** | 5 / 5 / 3; **30** | Delivers the main buyer journey. Same artifact revised; exact facts survive; draft status honest; no unexplained failed steps. |
| 5 / Q05 | Turn 02–04 plus follow-ups into repeatable acceptance coverage, with independent factual/numeric/format checks. Frontend + Model. **M–L** | 5 / 4 / 3; **27** | Prevents “one good answer” from standing in for reliable behavior. Run fixed prompts and record every attempt, including failures. |
| 6 / Q06 | Prove memory edit/forget and scenario isolation; close background review retry/parse failures (R-05). Model + Frontend. **M** | 4 / 5 / 3; **27** | Prevents demo facts contaminating later customer work. Authenticated edit/delete survives reload and semantic recall; background pipeline has a visible successful or correctly skipped outcome. |
| 7 / Q07 | Validate the connected-data versions: Verevon-owned Visma authorization and reads, inbox reads, carrier provenance, knowledge quality. Ingestion + Application + Model + gateway. **M–L**, external access dependent | 5 / 5 / 4; **20** | Required before claiming connected ERP/inbox/live shipping. Prove tool calls from Verevon with source timestamps and scope; no fabricated source replacements. |
| 8 / Q08 | Measure and improve latency, reconnect/replay, stop/continue/regenerate, mobile and browser rendering. Frontend + Model. **M** | 5 / 4 / 3; **27** | Makes the successful journey feel dependable. Measure during Q04/Q05 too; this is the final performance/recovery gate. |
| 9 / Q09 | Record approved runs and integrate four controlled media choices in the existing product section. verevon-web. **M** | 4 / 3 / 2; **28** | Shows the product doing real work. Raw video, factual review, poster, captions, accurate copy and usable playback all required. |
| 10 / Q10 | Reconcile old document summaries and manifests with dated evidence. Documentation. **S** | 3 / 3 / 1; **30** | Stops future work from reopening fixed issues or trusting incomplete verification. Preserve historical findings; link the latest status. |

Start with **Q01 → Q02 → Q03 → Q04**. Q06/Q07 are required before the connected-data release and its claims. Q09 waits for scenario sign-off; visual polish should not hide failures discovered in Q04/Q05.

For any patch: reproduce the symptom, identify the owning plane, add the smallest meaningful regression coverage, run the relevant existing suite, deploy through `build-verevon-services.sh` when needed, then rerun the original journey. A source fix and a live pass are separate evidence states. Do not sweep the large pre-existing working tree into a commit or reset it.

## 6. Connected-data prerequisites

- **Visma:** inspect the current Verevon MCP registration and approved read-only tool allowlist. An assistant-side connector does not establish a connection inside Verevon. If fresh interactive OAuth is needed, use the user's login at that concrete step. Verify complete pagination, date boundaries and order types from the actual schema.
- **Sales semantics:** preserve “order value” versus “invoiced revenue.” Missing `unitCost` cannot become zero cost. Do not calculate an authoritative margin from mixed taxes, currencies, quotes, cancellations or incomplete line data. Display unavailable measures honestly until the basis reconciles.
- **Shipping:** verify quote environment per carrier and deployed mock filtering. One 66 kg representative parcel from a five-parcel shipment does not establish the total freight price. Require the actual shipment shape and service eligibility; no unsupported delivery promise or booking.
- **Knowledge:** recheck the reported redirect-only Aquatiq documents before recrawling. Add genuine current product sheets and customer-service routines through Data/Ingestion contracts. Record version, extraction quality, indexing readiness and source scope.
- **Campaign:** a product name or a customer list does not prove efficacy, permitted uses or that a named customer uses a specific product. Unsupported claims are failures even if plausible. Extra source material is not a substitute for following exclusions.
- **Project:** obtain a real meeting record for a real-data claim, or retain the fictional scenario label. Treat the source pack's instructions as content, not authorization to send invitations.
- **Source isolation:** implement or verify an enforceable per-task grounding scope (selected attachments versus organization/connected sources). A prompt saying “use only these files” and a relevance instruction are not isolation mechanisms. Preserve tenant authority and retention at every boundary.

## 7. Release and recording gates

“Flawless” becomes a measurable acceptance bar for the supported journeys. Zero known blocking defects is required; a finite test run cannot establish that no bug can ever exist.

1. **Correctness:** every hard oracle condition passes, including revisions. No invented numbers, sources, statuses, product claims or performed actions. Validate the underlying artifact and chat summary, not just the visible closing sentence.
2. **Repeatability:** at least five consecutive fresh-thread passes per scenario on the chosen release build/model configuration, after fixes, including the specified follow-up. Keep failed attempts in the run log. This is a minimum recording gate, not a statistical production-reliability estimate. Test other advertised provider routes separately.
3. **Durability:** navigate away/back, reload, and exercise interrupted-stream replay. Stable artifact IDs, versions, files, citations and run outcome; no duplicate messages/effects. Include one controlled service-restart check before release, outside recordings.
4. **Failure recovery:** unreadable input, provider timeout, unavailable connector and expired authorization produce actionable states. Partial output is labelled; retry/cancel works; an unavailable tool is never replaced by an invented result.
5. **Trust:** no cross-tenant leakage; no demo document promoted into personal memory or unrelated knowledge; temporary-chat policy respected; expired policies lose to current ones. Draft-only scenarios cause no sends, bookings, publication or calendar changes.
6. **Experience:** readable result, correct native viewer, usable source links, no unexplained panel focus jumps, clean stop/continue/regenerate, consistent Norwegian, keyboard/reduced-motion/zoom/mobile checks, zero unhandled browser errors on the selected journey.
7. **Proposed performance budgets:** visible acknowledgement within 1 second; meaningful progress or first answer content p95 within 5 seconds; source-attached drafts/plans p95 within 30 seconds and computed/tool-driven reports within 60 seconds on the selected route. These are proposed targets, not measured current performance. Distinguish a spinner from substantive progress. Report cold/warm latency and maximum alongside p50/p95; use at least 20 measured runs for an initial performance sample. Connected-provider budgets must be measured separately.
8. **Evidence:** record timestamp, source hash, fixture date, workspace, thread/run IDs, provider/model, image IDs, tool outcomes, first-content and completion times, artifact IDs/versions, oracle results and any recovery. Keep operational evidence free of credentials.

Correctness, useful structure, and an effortless follow-up create the strongest demonstration. Additional charts, exports and suggested actions count only if they are actually useful, generated, accurate and working.

## 8. Product section delivery

`src/components/home/sections/ProductLoopSection.tsx` currently uses `product-showcase.mp4`. The recording plan describes it as an animated unsent example; it is not evidence of these four completed tasks. `ProductLoopProductDemos.tsx` currently provides ambient video behavior, not the planned four-task controlled player.

After the gates pass:

1. Preserve an uninterrupted raw recording of each fresh task and its follow-up. A repeatable recording mechanism must be established; old notes about one tool lacking recording are not proof that recording is impossible now.
2. Produce the planned 45–60-second main film and 25–40-second supporting clips. Label substantial time cuts/speed-up; these durations are edit lengths, not execution-speed claims.
3. Export genuine result posters and Norwegian VTT captions; set the manifest's currently null media fields only when files exist.
4. Integrate **Kundesvar / Salgsanalyse / Kampanje / Prosjektplan** into the existing section with explicit play/pause and a full-task view. Selecting a task stops the previous video. Load poster/metadata first and selected media on demand.
5. Verify mobile readability, keyboard controls, caption access and static reduced-motion behavior. Label fictional versus hybrid/connected evidence accurately, and review any identifiable customer data before using it in public material.

Do not expand the first release to A2A, scheduled actions, universal mutation parity, derived profiles, or an external Channel Plane widget. Track those separately against their own contracts and evidence. If a sales claim requires one of them, that claim introduces an explicit prerequisite instead of silently borrowing credibility from these four demos.
