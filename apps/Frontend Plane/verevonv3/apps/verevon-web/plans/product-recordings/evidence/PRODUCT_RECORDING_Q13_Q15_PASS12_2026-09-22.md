# Pass 12: review scope, reasoning and accurate timeout failures

**Later checkpoint:** [Pass 13](PRODUCT_RECORDING_Q13_Q15_PASS13_2026-09-22.md) records the subsequent context-sensitive recheck/completion fixes, final live controls and remaining failures. The results below remain the historical pass-12 evidence.

**Status: Improvements deployed; customer and sales journeys pass once; campaign/project and semantic accuracy remain open. Release and recording remain blocked.** This continues [pass 11](PRODUCT_RECORDING_Q13_Q15_PASS11_2026-09-22.md). Every live invocation, including source reviews and private repairs, used **`gpt-5.6-terra` through `openai-codex-subscription`**. No other model, provider or paid API fallback was used. The subscription remained active and privacy policy unchanged. UTC runs began September 21; this report uses the local September 22 date.

## Implemented

- **Reasoning for source review:** subscription reviews now request high effort on the selected model. Previously the reviewer always reset its reasoning budget to zero, which Inference Core maps to low effort. The new 4,096 request value selects high effort; it is not a measured token count. Authoring/private repair keep their existing effort. This can consume more subscription usage and take longer, and no complete-task speed improvement is assumed.
- **Shared scope rules:** authoring, repair and review use the same rules for sample proportions versus a wider population, role absence versus impossible completion, qualified risks versus existing obstacles, unknown status and business transaction types. Quotations must not silently become tenders, orders or invoiced sales. Equivalent ordinary wording remains allowed.
- **Source-bound annotations:** recognized absence, sample and explicit-unknown statements retain exact source spans. A clause can now carry multiple applicable limitations; a prerequisite no longer hides a lack of confirmation on the same clause. These annotations are advisory semantic context, not automatic factual verdicts. The original sources remain available to the reviewer.
- **Explicit scheduling assumptions:** conditional lower bounds now state that predecessors must satisfy required completion/approval criteria by their calculated finish. Later approval moves dependent work later. Drafting is instructed to retain these conditions. No approval is inferred, granted or booked by calculation.
- **Accurate timeout errors:** `response_validation_timeout` distinguishes elapsed draft/check deadlines from failed correction attempts. A failed draft explains that the work took too long and remains unpublished. It does not blame unclear sources. Direct checked-answer timeouts have a corresponding message. Durable failure persistence and terminalization precedence remain intact. The 90-second artifact-check and 150-second tool-phase limits were not raised; no fallback or automatic retry was added.
- **Versioned evidence:** new receipts identify `attachment-source-review-v6`, and browser expectations require that version. The component report records the actual requested reasoning budget without exposing private reasoning or credentials.

## Evaluation

The unchanged-policy high-effort diagnostic scored **11/15**, with two false acceptances and two false rejections; all 17 responses used Terra/subscription. It is retained as `review-effort-canary-01.json` and was not treated as qualification.

The final production-code evaluation scored **82/86**, with **one false acceptance, two false rejections and one timeout**. All **88 completed responses** used Terra/subscription with the production high-effort setting. There were no harness overrides, route violations or malformed-report failures.

| Cohort | Previous result | Current result |
| --- | ---: | ---: |
| Shared 82 cases from pass 11 | 72/82 | 78/82 |
| Four new independent controls | Not previously tested | 4/4 |
| Original 70 cases from pass 10 | 67/70 in pass 10; 63/70 in pass 11 | 66/70 |

All ten pass-11 failures passed in this run. Four previously passing cases failed, so these results do not establish a stable fix or exceed every earlier baseline. The labels, sources and candidates of all 50 prior saved cases are unchanged. Four paired controls were added for equivalent quote wording versus purchase orders, and visited stores versus population-wide prevalence; the 32 built-in cases are unchanged.

| Remaining failure | Evidence |
| --- | --- |
| False acceptance: `benefit-modal-held-out` | Accepted an adjustable arm *possibly improving staff productivity* despite the source stating that no productivity or health effects are documented. Modal wording does not supply evidence. |
| False rejection: `absence-risk` | Rejected a qualified possibility of technical delay, treating the affected work as insufficiently documented. |
| False rejection: `sales-corrected-whole-pass7` | Rejected the report's statement that it uses the supplied two files and no external sources, demanding that an attachment establish that report-level provenance. |
| Component timeout: `campaign-experience-saved` | The two-report review did not finish within the existing 75-second component deadline. A partial report is not acceptance and produced no receipt. |

The full matrix overlapped a native library compilation. Its timings are diagnostic component measurements, not a performance qualification. The subsequent live browser journeys ran without builds or other live inference alongside them. Reported zero token accounting values remain unavailable data, not proof of zero quota use. No repeated full run was used to replace this score with a greener result.

## Live journeys on the new gateway

| Scenario / attempt | Result | Observed time |
| --- | --- | --- |
| Customer, `journeys-01` | **Passed** initial draft, same-artifact revision, receipt and server reload; desktop/mobile artifact inspected. | Initial 44.139 s; revision 48.246 s; complete browser test 103.906 s |
| Sales, `journeys-01` | Published a correct initial report, then failed a test that allowed only one or two margin decimals. Follow-up/reload did not run. | Initial 121.023 s; browser test 123.492 s |
| Campaign, `journeys-01` | **Failed:** `response_validation_timeout`; no artifact or accepted receipt published. | Initial 125.802 s; browser test 128.335 s |
| Project, `journeys-01` | **Failed:** `response_validation_timeout`; no artifact or accepted receipt published. | Initial 131.955 s; browser test 134.216 s |
| Sales, `journeys-02` | Published a correct initial report, then exposed a second test defect: the Skrivebord profit decrease was compared with the company-wide increase. Follow-up/reload did not run. | Browser test 74.613 s |
| Sales, `journeys-03`, corrected test checks | **Passed** initial output, concise follow-up, checked completion and server reload. | Initial 69.127 s; revision 73.314 s; complete browser test 148.792 s |

**Sales test corrections:** the maintained brief does not prescribe decimal precision. Correct four-decimal weighted margins are now accepted at their displayed precision, while wrong final digits, signs and numeric substrings are rejected. Prose profit changes are checked against the stated product group or total, using independent CSV calculations. Company profit increased 1,680 kr; Skrivebord profit decreased 18,720 kr. The source fixtures, required values and expected semantic truth were not changed. Both earlier attempts remain failed browser records; offline arithmetic checks do not retroactively turn them into full journey passes. The later replay was justified by actual test changes, not used to overwrite the earlier evidence.

**Manual review:** customer initial/revised drafts preserve 9/12 packed, warehouse-arrival versus customer-delivery uncertainty, unconfirmed pickup, proposed split delivery and the next-working-day status policy. Source notes survive revision, and the revised document is legible on desktop/mobile. Both earlier sales documents distinguish quotations from invoiced sales, measured changes from possible causes, and gross profit from operating profit. The final sales document and follow-up were also inspected before this report was published. Campaign/project failure screenshots show one chat error card with the accurate timeout explanation and retry control. No failed intermediate document is presented as accepted.

**Stage evidence:** customer reviews took 14.202 s and 19.442 s, with no source repair. First sales review/repair/re-review took 32.285 / 9.439 / 29.813 s. Campaign's 27-segment review took 58.928 s; a two-segment repair took 10.135 s, leaving about 21 s for re-review before the 90 s check deadline. Project's 12-segment review took 45.525 s; one-segment repair took 20.425 s, leaving about 24 s for re-review. Both exceeded that deadline. Second sales review took 21.674 s and accepted on its first attempt. Three background title-generation timeouts were observed across `journeys-01`/`02`; preview titles were retained. Complete stage logs, including the final replay, are retained in the private evidence folder. These observations do not establish a p95 or a provider-only bottleneck.

Final sales replay: initial review accepted in 20.186 s. Its follow-up required a 21.857 s review, 9.653 s private repair and 17.971 s re-review before acceptance. A fourth background title timeout retained the preview title. No title fix is claimed; auxiliary generation remains part of the reliability and resource review.

These are individual observations. They do not satisfy five consecutive independent passes per scenario or the complete-task latency cohort. Successful browser assertions also require inspection of the generated text; a model review receipt is fallible evidence.

## Verification and runtime

- **1,267 Model Gateway library tests passed**, zero failed, one ignored (`gateway-unit-02.log`). Tests cover multiple exact source roles, Unicode/negative controls, selected route/privacy and reasoning across review/repair, batched coverage/cancellation, and timeout classification without hiding failed durable terminalization.
- Gateway release build and local deployment succeeded. Image: **`sha256:8b26e3f5efd4ea45b1aa57324ee7be6a22e2915b647ef072bd6126c424566e5e`**. Inference Core and Integration API images are unchanged. The broker retains 2 CPUs / 2 GiB; no memory-limit hits or OOM kills occurred in the saved snapshot.
- Browser: maintained Playwright `product-readiness` project at `http://localhost:5173/chat`, 1440 × 1000 desktop viewport. No frontend runtime code changed in this pass; receipt expectations and the two sales acceptance checks changed, with a pure test helper and independent regression controls. Browser evidence uses real subscription inference. No marketing recording was enabled.
- **43 sales-oracle regression tests passed** with an isolated Node/thread Vitest configuration (`sales-oracle-unit-03.log`); scoped ESLint and frontend typecheck passed. The first 32-case run passed under the normal jsdom configuration; the expanded suite then hit a worker-start timeout before running any tests. That failed runner attempt is retained as `sales-oracle-unit-02.log`. The isolated configuration executes the same pure maintained tests without browser setup. Final source hashes, report links, 50 unchanged prior cases, all 13 fixture hashes, route evidence, runtime health and scoped diff checks are recorded in `final-check.json`. No approved media or recording gate changed.

Private evidence: `C:\Users\ImaFernandesDaCosta\.codex\visualizations\2026\09\19\01a0b94f-be34-7aa3-8714-e90cb61181bc\q13-q15-pass12`. Credentials and the private Compose helper are not report artifacts. The rejected/failed canary, complete matrix and browser attempts remain available.

## Next priorities

1. **Close the remaining semantic failures with independent paired controls.** Unsupported possible benefits still need evidence. Legitimate qualified absence risks must pass. Report-level source provenance should come from actual authorized source use, not require a source document to describe how the assistant wrote the report. Keep the entire saved matrix and held-out controls; a better score on one cohort is insufficient.
2. **Reduce repair/re-review cost while preserving correctness.** The two blocked tasks spend most of their check budget reviewing, repairing and reviewing the document again. Evaluate reuse only for unchanged claims with matching content, source, requirement and checker bindings, accounting for cross-section dependencies and rerunning whole-result invariants. Keep the selected subscription, publication gate and existing deadlines. Reproduce campaign/project completion before collecting a latency cohort; raising the deadline alone does not meet the speed target.
3. **Prove repeatability and durable recovery.** Customer and sales have fresh successful journeys, not five consecutive independent passes. Campaign/project follow-ups and reloads remain untested on this build because their initial outputs failed. Finish all four paths, then run cancellation/disconnection/restart/stale-version cases and the authoritative atomic completion work.
4. **Qualify performance and recording only after correctness.** Run the planned 20-sample complete-task cohorts on a stable build, retaining timeouts and failures. Finish connected-data/audit prerequisites and device review before preparing and approving actual media.

Broader release gates remain: authoritative atomic run/result recovery, five consecutive accepted journeys, the 20-sample complete-task latency cohort, connected business-data prerequisites/audit delivery, device/recovery review and approved recording media. The product is not certified flawless.
