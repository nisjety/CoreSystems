# Q08–Q10 execution record

Date: 2026-09-19. Implementation and verification pass finished; release gates remain open. No recording or release sign-off.

Q08 delivered verified recovery/rendering fixes and measured latency, but final-output conformance and complete-scenario performance remain open. Q09 delivered controlled playback and a fail-closed publication gate; correctness and repeatability still prevent marketing recording. Q10 reconciled dated audits and the maintained manifest without erasing earlier observations.

Evidence directory: `C:/Users/ImaFernandesDaCosta/.codex/visualizations/2026/09/19/01a0b94f-be34-7aa3-8714-e90cb61181bc/q08-q10`.

Browser plugin not available; used the existing Playwright workflow against `http://localhost:5173`. Private traces can contain authentication headers and must not be published. No messages, bookings or business-system changes are part of these tests.

Flows: authenticated chat → attach or submit → interrupt/replay/revise → inspect persisted output; gated product section → choose one recording → play/pause → switch short/full version. Chat uses Chromium at 1440×1000 and 390×844. Playback QA uses the existing local Next server at `http://localhost:3001`, with an explicitly labelled temporary fixture; the real homepage was checked with the publication gate closed.

## Confirmed findings and changes

- A finite SSE response without a terminal event rendered an incomplete answer as complete. The controller now attempts replay of the same invocation once, preserving the cursor and partial text. Unavailable replay settles as stopped and offers continuation; it does not claim completion.
- Any SSE error previously triggered a second invocation with an empty model override. The transport test reproduced two invocations after a tool error. Removed this frontend fallback: tool failures stay visible, transport failures reconnect to the original run, and provider fallback remains owned by Inference.
- Replayed `stopped` now has its own callback rather than falling through to completed.
- Mobile CSS hid the selected conversation-only source label. The restricted scope now keeps a compact visible label.
- Visual review caught a second mobile defect after the page-width check passed: a long artifact title expanded an implicit grid column to 404 px inside a 333 px panel, clipping ordinary paragraphs. Explicit `minmax(0, 1fr)` column sizing and shrinkable header/body children now keep prose inside the panel. The saved real artifact passed independent desktop/mobile bounds checks after the production build and frontend restart. Wide tables retain their own horizontal scroll container. The customer test now checks inner prose/header bounds as well as page width.
- Norwegian chat used English `Today` / `Just now` labels. Day dividers, relative timestamps and work-step times now follow the selected locale.
- Tool selection initially received a short textual handoff instruction, but live telemetry showed it still drafted 3,005 discarded characters / 1,080 tokens on one long response. Replaced this with a required tool choice that includes a private `finish_tool_phase` control. The control is consumed inside Model Gateway, never dispatched as a business action or counted as evidence; a mixed finish/work response still executes its real calls. The final streaming conversation stays unchanged. Early live checks now show 23 decision tokens and zero discarded text. Bounded telemetry records counts and handoff outcome, not customer content.
- Standalone Markdown comments no longer appear as raw tags in the result viewer. Fenced/inline code keeps literal examples, and partial comment blocks remain non-executable metadata. The targeted Markdown suite passed 58 tests and the frontend production build passed afterward.
- Customer review exposed unsupported operational claims: a routine requiring an internal notification was described as a notification already sent, and unconfirmed collection became definite non-collection in the chat summary. Response discipline now distinguishes required next actions from recorded completed actions and preserves uncertainty in drafts, source notes and summaries. The acceptance oracle checks both the artifact and its summary. Its word-count boundary also recognizes blockquoted internal notes, which must remain separate from customer-facing copy.
- Controlled four-choice playback is prepared in the existing product section. The server projects only public media after all gates and factual review pass. It rejects missing files, missing full-recording captions, unreviewed packs and insufficient consecutive passes. Only the selected video mounts; it never autoplays, pauses when hidden/offscreen or replaced, supports native captions and full-task playback, and reports media loading failures.

## Recording status

No raw acceptance recording, edited clip, caption file or result poster is approved for marketing. Existing `product-showcase.mp4` remains the earlier animated example. The new player remains gated off on the public homepage until the complete approved pack exists. Playback QA fixtures must never be entered into the marketing manifest.

The maintained manifest is schema version 2. Historical September 14 runs are retained under `historicalVerifiedRuns`; `runVerified` is false for current release readiness. Input/prompt/direction SHA-256 values are recorded. The dated snapshot remains historical and points to the maintained pack. Hybrid notes now distinguish their September 14 observations from the later authenticated provenance check.

## Verification and next work

### Attempts and checks

| Attempt/check | Result and limits |
| --- | --- |
| experience-01 | Reproduced false completion on truncated SSE and two invocations after a tool error. Live reload lost its assistant turn. The stop test also had an ambiguous selector (two valid stop buttons); corrected the test. Mobile exposed the hidden source-scope label. |
| experience-02 | Four recovery journeys passed, plus auth setup: truncated stream, error without reinvocation, real stop/continue/regenerate, and real mid-answer reload with one original invocation. Mobile failed because the initial fix used an unsupported Solid 2 `classList`; TypeScript caught this too. Replaced it with the supported class expression. |
| mobile-03 | Real mobile table, KaTeX and code rendering, keyboard focus, reduced motion, source label and width checks passed after the correction. |
| experience-04 | All seven automated checks passed (auth, five experience tests and the customer artifact/revision/reload journey), including corrected Norwegian timestamps and the mobile artifact view with no console errors. Manual customer-copy review found a factual nuance the old oracle missed: “pickup not confirmed” became “the carrier has not picked up.” The revision also described its changed internal table as unchanged. Strengthened the pickup assertion; this attempt is durability/experience evidence, not editorial sign-off. |
| experience-05 | All five recovery/mobile tests and auth passed on the explicit finish-control build. The customer test stopped at a word-count failure: its parser included a blockquoted internal note. Manual review confirmed the customer body was within 150 words, but exposed genuine unsupported notification and pickup claims. Fixed the parser boundary and strengthened factual checks rather than accepting this run. |
| experience-06 | Eight automated checks passed: auth, five experience checks, customer initial/revision/reload and campaign initial/revision/reload. The new notification/uncertainty checks passed. Manual review still rejected editorial approval: the customer summary called a changed source table identical; campaign copy inferred “takes little space” and suitability for neutral offices from dimensions/colours; its revision summary said the publication overview was unchanged and still had the old angle although the artifact had updated it. Added assertions for these findings. This run also exposed the artifact panel clipping fixed afterward. The old automated pass does not imply the strengthened oracles pass. |
| Artifact layout follow-up | `artifact-layout-qa.cjs after` reopened the same real customer thread without model generation. Passed panel/header/prose bounds and no page exceptions at 390×844 and 1440×1000 on the final frontend build. Screenshots confirm wrapped prose. Artifact timestamps now also use Norwegian. Source packs and generated business artifacts were not edited by the QA script. |
| Model Gateway | Full library suite after the explicit finish control: 1,195 passed, one ignored. After preserving truncation identity when an internal finish is removed, 145 tool-loop tests passed. An earlier `--bin` filtered command ran zero tests and is not counted as validation. |
| Focused frontend/contracts | 79 tests passed across five files, including interrupted replay, late replay after replacement, continuation, API events and the publication contract. |
| Builds | Frontend production build/typecheck passed. verevon-web production build passed after removing the generated type file for the temporary QA page. The initial stale generated-type failure is retained. |
| Player browser QA | Passed at 1440×1000 and 390×844 with reduced motion: no autoplay or unselected-video fetch, previous video paused, full-task switch, loaded Norwegian captions, keyboard selection, no horizontal page overflow, visible media-error state and gated homepage. Zero page exceptions. The first QA attempt had an ambiguous alert locator due to Next's route announcer; scoped it to the player and reran successfully. |
| Capture mechanism | Saved a continuous 558,846-byte WEBM of the explicitly labelled animated playback fixture. ffprobe confirmed VP8, 1440×1000, 25 fps, 7.96 seconds. This is a technical capture check, not one of the four marketing recordings. The temporary page/caption fixture and its generated route type were removed. |
| Capture gate | `PRODUCT_RECORDING_CAPTURE=1` is rejected with the current manifest, before authentication or recording. Ordinary QA remains available. |
| Documentation | Final checker verified 12 matched current-status links, 13 unchanged source/prompt/direction hashes, four retained historical runs, zero approved media and blocked recording readiness. |

Browser page identity and meaningful content were checked in the existing authenticated flows. The mobile chat check asserts no Vite overlay and no page/console errors; player QA reports no page exceptions, native caption loading and working selection/full-task controls. Screenshots were saved outside the repository and inspected, including `experience-05/.../mobile.png`, `player-desktop.png` and `player-mobile.png`. The player's screenshot contains an animated QA example, not a customer task or marketing approval. Targeted web lint passed; this is not a claim that the entire monorepo lint/test suite is green.

### First latency cohort — textual handoff attempt

Twenty sequential fresh-context, attachment-grounded status summaries on the deployed `verevon-balance` → `claude-sonnet-4-6` route. Workload: a fixed small fictional brief, asking for 100–120 words directly in chat; the test checks the central facts, not complete campaign-style editorial conformance. All 20 requests completed and passed those factual checks. Builds and other test workloads had finished before this cohort. Source/prompt hashes and thread IDs are in the JSON evidence; image IDs are in `deployed-images.txt`.

| Measurement | p50 | p95 | Maximum | First sample |
| --- | ---: | ---: | ---: | ---: |
| Visible acknowledgement | 0.134 s | 0.168 s | 0.181 s | 0.137 s |
| First visible answer text | 1.810 s | **6.345 s** | 8.619 s | 8.619 s |
| HTTP response headers | 0.334 s | 0.443 s | 1.291 s | 1.291 s |
| Completion | 7.802 s | 14.001 s | 16.037 s | 16.037 s |

Nearest-rank percentiles. Browser MutationObserver measures acknowledgement and non-empty rendered Markdown; neither a spinner nor response headers counts as first answer text. Completion includes the response body and observation overhead. Every browser context was fresh, but the backend/provider caches were allowed to warm naturally; first sample is **not** a certified cold-provider run. A separate three-run pre-change sample is retained (first-answer p50 2.356 s, completion p50 8.610 s), too small and cache-sensitive to establish a causal speed-up. Fresh-source/uncached cohorts and the complete four tasks need separate distributions. The first-answer p95 fails the proposed five-second budget even on this small workload.

### Second latency cohort — explicit finish control

Same twenty-request workload on the explicit finish-control build (`deployed-images-finish.txt`), with no overlapping build or test workload. All requests passed the then-current central-fact assertions. Every handoff emitted 23 tokens and zero discarded text, but latency did **not** improve reliably:

| Measurement | p50 | p95 | Maximum | First sample |
| --- | ---: | ---: | ---: | ---: |
| Visible acknowledgement | 0.129 s | 0.161 s | 0.179 s | 0.179 s |
| First visible answer text | 2.053 s | **20.052 s** | 22.669 s | 12.070 s |
| HTTP response headers | 0.305 s | 0.817 s | 0.914 s | 0.914 s |
| Completion | 8.729 s | 24.226 s | 29.239 s | 19.011 s |

`latency-finish-runtime.jsonl` records 52 code-interpreter dispatches across these 20 requests. The slow path included multiple model rounds for word-count validation even though the prompt explicitly requested no tools. The standing validation instruction previously said to use the interpreter whenever available, without respecting that restriction. It now requires permission under the user's request; artifact scenarios that allow tools retain their validation. The benchmark now asserts no tool calls as well as its central facts. This is a corrected instruction conflict, not permission to waive factual or length constraints. These two cohorts predate that final instruction correction and cannot certify its speed.

### Final latency cohort — respecting the no-tools request

Twenty sequential runs on the final instruction build (`deployed-images-discipline.txt`), same source/prompt hashes and model route. All completed, preserved the checked central facts and invoked zero business/artifact/code tools. No build or other test workload overlapped this cohort.

| Measurement | p50 | p95 | Maximum | First sample |
| --- | ---: | ---: | ---: | ---: |
| Visible acknowledgement | 0.131 s | **0.167 s** | 0.174 s | 0.153 s |
| First visible answer text | 1.640 s | **3.684 s** | 4.500 s | 4.500 s |
| HTTP response headers | 0.345 s | 0.381 s | 0.504 s | 0.353 s |
| Completion | 8.039 s | 15.373 s | 16.788 s | 16.788 s |

This meets the acknowledgement/first-answer timing targets for this repeated small workload. It is **not a full acceptance pass**. **September 20 correction:** the authoritative saved UTF-8 messages show **0/20** outputs within the requested 100–120 words (80–99 observed, excluding section headings). The earlier 6/20 count was distorted by character decoding in DevTools network captures. The QA capture now uses browser Fetch's UTF-8 decoder; see [the correction and new runtime checks](PRODUCT_RECORDING_Q11_Q12_2026-09-20.md). Explicit minimum/maximum assertions prevent future timing runs from silently passing this defect. The measured cohort ran before those assertions were added; its automated pass count must not be mistaken for length-conformance approval.

The same September 20 durable-message audit corrects the first cohort to **1/20** within range (81–100 words) and the second to **10/20** (107–174 words), superseding the earlier 7/20 and 8/20 network-capture counts. Even using code to count a draft did not ensure the separately streamed final text preserved its length. This needs a general result-validation solution, not a relaxed oracle or a selected lucky answer. The original reports remain retained, alongside the corrected durable-message audits. Caches were allowed to warm naturally; these measurements do not establish an uncached or cross-provider speed-up.

### Complete-journey timing observations

On the final Model build, `experience-06` recorded the following single-run timings. These are observations, not p95 distributions. They remain above the proposed 30-second draft/plan completion budget, despite the faster small direct-answer workload:

| Turn | First answer text | Completion |
| --- | ---: | ---: |
| Customer initial artifact | 31.815 s | 45.395 s |
| Customer revision | 27.313 s | 33.147 s |
| Campaign initial artifact | 49.793 s | 62.388 s |
| Campaign revision | 85.496 s | 94.680 s |

Tool/work progress may precede answer text; this harness does not yet quantify the first meaningful tool-progress display. The final frontend-only artifact sizing/timestamp patch was applied afterward and checked against the saved customer result; no model configuration changed after these observations.

### Remaining release work

1. Enforce final-output conformance across artifact and streamed summaries: requested lengths, supported claims, uncertainty, actual action status and the campaign's required form of address. Re-run exact original prompts and revisions; retain all failures.
2. Establish five consecutive complete passes per scenario on one recorded release build/route, including durability and manual source-to-answer review. A latency microbenchmark is not one of these passes.
3. Measure at least 20 complete runs per selected workload with labelled cold/warm conditions. Prove offline/reconnect and expired authorization recovery beyond the tested paths, plus Firefox/Safari, real-device and 200% browser-zoom checks. These Chromium checks do not certify those environments.
4. Close the connected-data/audit-delivery prerequisites from Q05–Q07 for the corresponding release claims. Then record the four approved journeys, review raw footage/posters/captions, and activate the gated player through the maintained manifest.
