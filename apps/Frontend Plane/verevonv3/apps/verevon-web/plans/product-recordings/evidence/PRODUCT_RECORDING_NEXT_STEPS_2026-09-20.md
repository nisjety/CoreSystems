# Verevon: next implementation and release steps

**Current checkpoint (September 23, pass 6 capture):** All four scenarios have private rehearsal films from different builds. The project scenario now has five consecutive complete private initial/REGI/reload journeys on the final build, preserves the plan and the separate note, and has a readable 34.52-second private edited film from the final passing take (137.88-second raw capture). Prior takes timed out during independent source rechecks or were falsely rejected by narrow browser-oracle wording. A newly found unsupported “vedtatt” label for an activity date is now caught locally. The retained high-effort priority subscription reviewer passed 104/104 labeled semantic cases and 10/10 recheck controls. Private project response time still varied from 44 to 116 seconds for the plan. All four scenarios remain at 0/5 consecutive **qualified release** runs on one build; all 20 public media fields are null. Release remains blocked. All live inference used only **`gpt-5.6-terra` via `openai-codex-subscription`**. See the [current evidence](PRODUCT_RECORDING_CAPTURE_PASS6_2026-09-23.md), [maintained recording plan](../README.md) and [manifest](../manifest.json).

**Historical pass-5 checkpoint (superseded above):** The project scenario had no complete passing journey or film in v21–v25. The [pass-5 record](PRODUCT_RECORDING_CAPTURE_PASS5_2026-09-23.md) retains those failures and the reviewer-control baseline.

**Historical pass-4 checkpoint (superseded by pass 5 above):** Four fresh project attempts (v17–v20) timed out during source review/repair or recheck. The experimental lower-effort review falsely rejected a valid conditional project date (103/104) and was discarded. See the [pass-4 evidence](PRODUCT_RECORDING_CAPTURE_PASS4_2026-09-23.md).

**Historical pass-13 checkpoint (September 22; superseded by pass 4 capture above):** All live inference remains exclusively **ChatGPT Terra (`gpt-5.6-terra`) via `openai-codex-subscription`**. Private source rechecks now require explicit reconfirmation, invalidate edited sections and reference-dependent passages, and retain complete document/source context. Artifact completion reuses its title and skips optional title/suggestion inference. Backend tests passed **1,273**; final live recheck controls **6/6**; sales-oracle controls **55**, scoped lint and typecheck passed. The full first-review evaluation scored **90/94**, but the shared cohort remains **82/86**, with one false acceptance, two false rejections and one timeout. Customer and sales completed fresh initial/revision/reload journeys (**110.047 s / 159.547 s** full browser tests); the first sales attempt exposed a cross-sentence test-parser bug, now corrected with its failed record retained. Campaign/project still timed out (**122.890 s / 139.593 s** initial turns), publishing no artifact. [The pass-13 record](PRODUCT_RECORDING_Q13_Q15_PASS13_2026-09-22.md) distinguishes the earlier v7 evaluation, failed recheck control, final v8 safeguards and all browser results. **Release and recording remain blocked.**

**Historical pass-8 checkpoint (superseded by pass 13 above):** Implementation continued; release and recording remain blocked. [Pass 8](PRODUCT_RECORDING_Q13_Q15_PASS4_2026-09-20.md#pass-8-computed-numerical-and-conditional-schedule-evidence) adds bounded local numerical checks and conditional workday schedules, with private repair before source review. Backend tests passed **1,260**; all eight local regression/control cases passed. Live source review scored **54/62**, including all four added cases, but **50/58** on the shared cases versus pass 7's 51/58: general semantic reliability has not improved. The fresh browser run has six passes and two failures. Customer completes correctly; sales arithmetic and revision/reload pass but its initial cost definition needs editing. Campaign still invents suitability claims, and project planning makes a further rewrite and times out without publishing a result. Checked task completion, complete claim/evidence binding and authoritative recovery are the immediate priorities. Earlier [Q11/Q12](PRODUCT_RECORDING_Q11_Q12_2026-09-20.md) and [Q13/Q14](PRODUCT_RECORDING_Q13_Q14_2026-09-20.md) records retain historical evidence.

**Date:** 2026-09-23 (original plan: 2026-09-20)

**Decision owners:** Product owner for release scope and quality targets; Model Plane for execution and result correctness; Frontend Plane for interaction and presentation. Existing plane authority remains unchanged.

## Objective and current evidence

Make the four complete product journeys dependable: one clear request, correct source use, a usable result, an accurate revision, durable reopening and understandable recovery. For release, “flawless” means no known acceptance failures in the selected journeys, the measured performance budgets met, and failures handled honestly. A finite test run cannot establish that no future bug or model error is possible.

The historical [Q08–Q10 record](PRODUCT_RECORDING_Q08_Q10_2026-09-19.md) established the following earlier baseline; the September 23 checkpoint above governs current release status:

- Transport/replay, stop/continue/regenerate, source-label visibility, Norwegian timestamps and artifact panel sizing have received fixes and targeted verification.
- A small repeated direct-answer workload achieved acknowledgement p95 0.167 s and first-answer p95 3.684 s. Corrected on September 20 from the authoritative saved UTF-8 messages: **0/20** final answers met its requested 100–120-word range (80–99 words). The earlier 6/20 count was distorted by network-capture character decoding. Timing success is not task success.
- Complete customer/campaign turns took 33–95 seconds in the last observed run. These are single-run observations, not complete-task p95 measurements.
- Eight automated checks passed, but manual review still found unsupported campaign properties and summaries that contradicted the saved revision. The oracles were strengthened afterward; the old pass count does not certify the stronger checks.
- The player and publication gate exist. No raw customer-task recording or marketing media is approved.

The [Q05–Q07 record](PRODUCT_RECORDING_Q05_Q07_2026-09-19.md) also leaves connected-data prerequisites and audit delivery open. Earlier work remains useful evidence; Q05/Q08/Q09 are not closed by introducing the next task numbers.

## Architecture decision: validate the delivered result

### Original source-confirmed gap

The list below records the original decision baseline. Later passes now check supported document writes before dispatch and attach bound completion receipts on supported paths. Pass 6 stages text candidates until the existing durable append and terminal acknowledgements succeed; the saved replay no longer exposes intermediate versions that disappear after failure. These separate RPCs still need an authoritative atomic completion contract. Source review remains semantically fallible. Follow the current checkpoint for deployed behavior rather than treating every baseline item as unchanged.

1. [`authored_artifact_events`](../../../../../../../Model%20Plane/rust/services/model-gateway/src/tool_loop.rs) emits the artifact and tells the model it is visible, then asks the model to review it. This is voluntary review of already-exposed content.
2. [`sse.rs`](../../../../../../../Model%20Plane/rust/services/model-gateway/src/sse.rs) runs tool rounds and then makes a separate `infer_stream` call for the answer. A validated draft and the final response can therefore diverge.
3. [`verification.rs`](../../../../../../../Model%20Plane/rust/services/model-gateway/src/verification.rs) explicitly runs after text is on the wire. It may skip long, already-evidenced or sufficiently high-scored answers. It is not an acceptance check for every artifact, instruction or revision.
4. [`RecordedArtifact`](../../../../../../../Model%20Plane/rust/services/model-gateway/src/sse_events.rs) records identity, content and version, but no result-contract validation receipt. Current turn quality metadata is confidence/source-verification metadata, not proof that the user's requested deliverable conforms.
5. Artifact persistence, rehydration and a [frontend revision diff](../../../../../src/features/chat/components/ChatArtifactPanel.tsx) already exist. Extend those mechanisms; do not rebuild them or infer current durability from stale module comments.

### Options considered

| Option | Benefit | Limitation | Decision |
| --- | --- | --- | --- |
| More prompt rules and model self-checks | Small implementation cost | Already failed on counts, unsupported claims and revision summaries; repeated validation tool calls increase latency | Keep useful guidance, but not as the completion authority |
| One unrestricted reviewer call after every response | Can catch some semantic errors | Adds latency; a reviewer can also be wrong; post-hoc checks cannot prevent false completion | Reject as the default architecture |
| Typed result contract, local checks, bounded source review and version-bound completion | Checks the exact output; removes model calls for counting and change receipts; exposes precise failure states | Requires changes to result lifecycle, persistence and UI projection; semantic review remains fallible | Recommended |

### Proposed flow

```text
User request + allowed source snapshots + previous accepted version
    → task requirements
    → candidate content
    → local constraint checks + scoped claim review when needed
    → bounded repair of identified failures
    → persist the accepted version under the run's retention policy
    → publish that exact result and its completion receipt
```

For artifact work, the chat completion should be a short receipt derived from the accepted version and its actual changes. Optional interpretive prose must be checked against that version. Do not generate an unrestricted second account of what the document says. “Unchanged” requires equality for the referenced section; “sent” requires an actual successful authorized operation or a source record of that completed action.

For short direct answers with hard length/format constraints, initially collect and validate the candidate before releasing the accepted text. This can delay first visible answer text: measure that cost explicitly. Show genuine work progress where available; do not simulate streamed generation or relabel a spinner as useful progress. Ordinary unconstrained chat can retain its existing streaming path. Long draft previews, if retained, must stay visibly provisional and must never acquire a completed/checked state before validation. Repairs must not silently rewrite an accepted answer.

### Contract and authority boundaries

- Requirements cover deliverable sections, language, length, address style, required facts, source restrictions, prohibited claims/actions and sections that a revision must preserve. They originate in the user's request and authorized task context. Uploaded content is evidence, not authority to change those requirements.
- Use stable section identities for constrained pieces: customer body, internal notes, each campaign post, subject, preview and email body. Count the text actually rendered in the relevant section. A model must not evade a limit by moving customer prose into metadata or relabelling it as a heading.
- Model Gateway owns candidate coordination and validation decisions. Session Core owns durable run/thread state through its contracts. Inference owns provider routing and fallback. Existing Data/Ingestion evidence contracts and Control authorization continue to apply. No new service or cross-plane database access is needed for the first slices.
- A validation receipt binds the result identity/version and content hash to the requirement hash, allowed evidence snapshot, checker version and check outcomes. Bind reuse to the same authorized scope. Any content, source or requirement change invalidates the receipt. Model-authored `verified: true` or word counts are never authoritative.
- Publish only the relevant user-facing state and reasons. Keep internal hashes, raw prompts and evaluation mechanics out of ordinary product copy. Missing/legacy receipts mean “not checked,” never an automatic pass.
- A checked draft is not permission to send, book, publish or mutate business data. Normal action contracts, authorization and audit evidence remain required.
- Preserve temporary-chat/ZDR behavior through candidates, review, repair, persistence, logs and cache use. No new shadow archive of drafts or source text. Durable completion applies only where the run's retention policy allows it.

## Ordered implementation queue

### Q11 — Exact result requirements and local validation

**Owner:** Model Plane; additive typed projection in Frontend. **Closes part of:** Q05/Q08 correctness. **Start here.**

- Define the versioned result-contract and validation-receipt types, plus drafting/checking/ready/needs-attention transitions. Reuse existing run/thread, artifact and error envelopes.
- Start with explicit word limits, section counts, language/address constraints, dates/numeric invariants and prohibited operations. Parse supported explicit requirements with traceable user-text spans; do not silently discard ambiguous requirements or treat the model's extraction as authority to weaken them.
- Implement bounded local text checks. Word counting and unchanged-section comparison do not require a sandbox or an inference round. Define Unicode/hyphen/hashtag and heading/internal-note rules explicitly.
- Run checks on the final candidate, including the actual streamed/direct-answer text where applicable. Emit counts from the validator, not from a model assertion.
- Capture timing for each existing phase now, so later optimization has a useful baseline.

**Acceptance:** All saved length failures are rejected by the runtime check; valid boundary cases pass; no business/code tool calls appear for “answer without tools”; no ready receipt survives a content change; direct-answer checks cannot be bypassed by Markdown formatting. Include held-out Norwegian/English requests and different names, counts and source layouts, not just the four fixtures.

**First deployable slice:** The existing 100–120-word direct-answer workload, with exact candidate validation and an honest unresolved state when it fails. It must not report success merely because inference returned successfully. This slice does not yet certify semantic truth.

### Q12 — Accurate revisions and completion receipts

**Owner:** Model + Frontend. **Depends on:** Q11.

- Capture the prior accepted version and derive changed/unchanged sections from actual before/after content. Reuse the existing UI diff rather than adding a second competing revision interface.
- Preserve the same artifact ID. Internal repair candidates should not create a confusing sequence of user-facing “final” versions.
- Produce a concise localized completion receipt from stored facts: artifact title/version, checked counts, changed sections, preserved sections and unresolved checks. Keep the full document in Resultat.
- Persist/check the receipt with the version through existing Session Core contracts. Reject a stale completion against a newer edit; prove restart, reload, replay and concurrent revision behavior. Do not assume current per-turn metadata alone proves atomicity for the new lifecycle.

**Acceptance:** The saved customer “identical source table” and campaign “unchanged overview” contradictions cannot be emitted as accepted completion receipts. Copy, download, displayed content, history and summary reference the same version after reload. Changed contract/evidence invalidates prior approval. No extra full-answer model call is needed for a simple artifact completion receipt.

### Q13 — Source support, uncertainty and bounded repair

**Owner:** Model, consuming existing source/action evidence. **Depends on:** Q11/Q12.

- Build the review input from the exact candidate and allowed source versions. For source-bounded tasks, distinguish supported facts, proposals, unknowns, contradicted claims and required-but-unperformed actions.
- Check deterministic values in code: the sales arithmetic, calendar arithmetic, exact quantities, source precedence and actual recorded action outcomes. A successful calculation tool does not by itself validate unrelated prose.
- Review factual claims against cited source spans. Confirm the spans exist and are in scope. A citation ID or a review model's agreement alone is insufficient proof. Reuse applicable entailment helpers, but do not repurpose the current post-hoc confidence badge as a release gate.
- Preserve negation and uncertainty: “not confirmed collected” cannot become “not collected”; a notification procedure cannot become a notification already sent; dimensions/colours cannot establish suitability or performance.
- Repair only the failed sections, retaining unaffected content and source scope. Proposed initial bound: at most two repair attempts, plus a task-wide time/token limit. Revalidate the whole required result after each repair. An unchanged failed candidate or exhausted budget ends as needs-attention, retaining a usable labelled draft and a specific reason.
- The reviewer receives no business-mutation tools. Review must not expand a conversation-only task into workspace/web search or send source content outside its permitted route.

**Acceptance:** Known unsupported campaign claims, fabricated completion claims and uncertainty inversions are caught; legitimate qualified statements are not rejected. Every required section and high-impact claim has a traceable outcome. Evaluate missed errors and false alarms with independent labelled examples and manual review; do not claim that a semantic model verifier guarantees truth.

### Q14 — Complete-task speed and useful progress

**Owner:** Model + Frontend. **Depends on:** Q11–Q13; measurement starts in Q11.

- Record source-ready, tool decision, dispatch, generation, validation, repair, durable completion and rendering times, along with round counts, cache observations and token use. Separate provider delay from orchestration and frontend delay.
- Remove model/code-interpreter calls used only for word counts, deterministic checks or recounting an artifact in a second answer. Combine checks for all campaign pieces in one pass. Validate only changed inputs where the version-bound receipt permits reuse; rerun dependent whole-result invariants.
- Preserve actual computation tools for tasks that need them. Parallelize only independent reads/checks; never repeat or parallelize dependent mutations for speed.
- Keep source-scope/privacy/provider choice intact. Do not reintroduce the removed prompt-keyed answer cache or hide fallback routing to obtain better benchmark numbers.
- Show progress derived from completed source/evidence/calculation events. Make the result immediately easy to inspect, with source previews available without displacing it. Avoid focus jumps and repeated chat copies of the document.

**Acceptance targets:** acknowledgement ≤1 s; meaningful work progress or first answer p95 ≤5 s; source-attached draft/plan completion p95 ≤30 s; computed report p95 ≤60 s. Report first draft text and first validated result separately. These are targets to prove, not promises that a selected provider can meet before measurement.

Measure complete initial+revision journeys on a stable build with no competing builds/tests. Use at least 20 samples for each reported workload/condition percentile, label cold/warm conditions from observable cache state, retain failures/timeouts and report maximum, cost and repair frequency. A fresh browser is not a cold provider. Do not flush shared production caches to manufacture cold runs. Reconsider route/architecture against the same correctness gates if a target remains infeasible.

### Q15 — Recovery, result UX and infrastructure completion

**Owner:** Frontend + relevant Model/Control/Application service owners. **Depends on:** Q11 lifecycle; infrastructure work can begin independently.

- Extend the working resume/cancel flows to candidate validation and repair: disconnect during checking, reload during repair, cancel before publication, expired replay buffer, stale authorization, tool timeout and provider failure.
- Reconnect to the original invocation where possible; do not repeat mutations or silently swap the selected provider. Preserve the last accepted result and distinguish an incomplete new draft from it.
- Test desktop/mobile, keyboard, reduced motion, 200% zoom and the supported browser matrix. Include inner panel/prose bounds, accessible horizontally scrolling tables, useful error recovery and source/result visibility. DOM width and a green console are not substitutes for screenshot review.
- Reconcile audit provisioner credentials/subject ownership through the existing owners; prove outbox delivery and recovery with no lost/duplicated business effect. Diagnose before topology changes; do not delete streams or reset consumers to make errors disappear.

**Acceptance:** The actual recorded failures recover without false completion, lost content, duplicate effects or unhandled browser errors. Current selected journeys have no known critical rendering/usability defects. Audit delivery is observed, not inferred from healthy containers.

### Q16 — Qualification of all four journeys

**Owner:** Product acceptance across Model/Frontend. **Depends on:** Q11–Q15.

- Keep the exact maintained prompts/sources/REGI follow-ups and independent arithmetic/content expectations. Runtime correctness must work without receiving the acceptance oracle as privileged prompt content.
- Use saved failures as deterministic regressions, plus held-out variants to detect fixture-specific fixes and brittle parsers. A parser correction never retroactively turns an interrupted journey into a complete pass.
- Review both the artifact and accompanying chat: required facts, quantities, uncertainty, source precedence, separate internal notes, recipient/address style, length and revision preservation.
- Obtain five consecutive complete accepted journeys per scenario on one identified build/model configuration. Keep all attempts and raw timing distributions. A relevant change restarts qualification for affected journeys; shared completion changes affect all four.
- Verify persistence across reload/restart, isolation from workspace memory, and the source/execution receipts. Run the full performance cohorts after functional behavior stabilizes.

**Acceptance:** All seven existing manifest gates have current evidence for their scoped claim. Unit tests, browser success, editorial acceptance, performance and integration proof remain separate results. Five consecutive passes are a minimum release check, not a statistical guarantee of zero future errors.

### Q17 — Approved capture and product-section release

**Owner:** verevon-web + factual/media review. **Depends on:** Q16.

- Use the already-built player and capture/publication gates. Record the four real tasks, including follow-ups, from the approved build. Inspect the continuous raw footage before editing.
- Verify the capture specification. Current technical capture was 25 fps; a 30 fps export cannot establish a true 30 fps source if that remains the required recording target.
- Produce real result posters and Norwegian captions for both short and full versions. Disclose cuts/speed changes; film duration must not imply an unmeasured task latency.
- Perform factual, privacy, readability, captions and playback review on the actual media. Populate approval/build/evidence fields only from completed checks, then activate the four media choices together.
- Check the real homepage's controls, load behavior, captions, keyboard/mobile use and errors. Keep the existing manifest-controlled rollback path and production error/latency observation with content-minimized telemetry.

**Acceptance:** Four reviewed recordings and their media assets exist, the production page uses them correctly, and every visible product claim matches the recorded behavior and its stated data provenance.

## Connected-data work remains a distinct prerequisite

Continue Q07 for the hybrid scenarios: the intended Verevon workspace and authorized Visma endpoint, genuine customer conversation and versioned knowledge, carrier environment/quote provenance, FedEx Rates authorization, and a valid multi-parcel contract before claiming a complete shipment quote. Keep the current empty/sandbox/unauthorized results visible until replaced by real proof.

These are required for the corresponding connected-product claims. They should not be fabricated to unblock fictional-data demonstrations, nor should unrelated carrier authorization be described as a failure of a fictional customer draft. The existing audit/authorization/source-isolation requirements still apply to the release itself.

## Recommended next execution batch

1. Make the project first draft respect source-stated authority, approval state, dates, prerequisites and completed work. Keep exact source/prompt/REGI fixtures unchanged. Improve bounded repair and independent recheck of changed and dependent passages so the final document is both correct and ready inside the existing 90-second artifact deadline. Preserve the 104/104 high-effort controls and held-out valid/invalid examples; a quicker but less accurate checker is not a pass.
2. Obtain one complete normal project journey: accepted initial document, exact REGI revision, checked final artifact, server reload and no unhandled browser error. Keep all timeout and diagnostic captures as failures. Make its private rehearsal film only from a passing continuous raw capture.
3. Stabilize one identified release build and complete Q15 recovery/durability/experience work and Q16 qualification on **all four** journeys. Require five consecutive complete passes for each scenario on the same build, route and unchanged fixtures; a failure resets that scenario's streak. The three earlier private films cannot satisfy this gate.
4. Measure full-task p50/p95 against the declared budgets, including failures, timeouts, repair frequency and cold/warm labels. Verify restart/new-browser durability, disconnect/cancel/provider-failure recovery, desktop/mobile/keyboard/captions/reduced motion, and editorial truth. Only then run Q17 approved release capture, fill public media fields and activate the product section.

Keep this work in small reviewable changes against the dirty checkout. Preserve user changes, existing source-scope/retention controls and earlier evidence. The linked execution records identify local code/runtime updates. No production deployment or recording approval is established by this plan.
