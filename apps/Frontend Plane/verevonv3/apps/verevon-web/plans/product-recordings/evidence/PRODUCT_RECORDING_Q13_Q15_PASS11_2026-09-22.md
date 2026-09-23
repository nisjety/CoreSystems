# Pass 11: campaign repair budgets, bounded source review and failure presentation

**Later checkpoint:** [Pass 12](PRODUCT_RECORDING_Q13_Q15_PASS12_2026-09-22.md) records the next subscription-only improvements, fresh customer/sales passes, blocked campaign/project runs and the final 82/86 source-review evaluation. This pass-11 record remains historical evidence.

**Status: improvements implemented; campaign/project completion and semantic accuracy still fail qualification. Release and recording remain blocked.** This continues [pass 10](PRODUCT_RECORDING_Q13_Q15_PASS10_2026-09-21.md). All live inference used **`gpt-5.6-terra` through `openai-codex-subscription`**, including reviews and repairs. No other model, provider or paid API fallback was used. The authenticated account retained its active subscription and existing privacy policy. Runs began September 21; this checkpoint uses the local September 22 date.

## Changes

- **Campaign repair word budgets:** the gateway subtracts unchanged body text from each section's bounds and sends the remaining allowance to the repair model. Multiple failed paragraphs share one section allowance. Metadata and internal notes do not count as customer copy. Existing patch binding protects unaffected sections, and the complete candidate undergoes local and source checks before publication.
- **Initial campaign guidance:** aim near the middle of each required range using sourced facts and relevant invitations, leaving room for factual corrections without padding or invented benefits.
- **Bounded parallel reviews:** subscription documents with at least 12 segments use at most two concurrent reports. Both calls receive all sources, all document segments, all requirements and the same route/privacy settings. Only their reporting assignments differ. The parser rejects missing, duplicate and unassigned indexes, malformed evidence and incomplete responses. A successful receipt requires complete coverage and acceptance from both reports. A malformed report alone is retried; a valid sibling is retained for that exact candidate. Cancellation drops both futures. The existing **90-second artifact-check and 150-second tool-phase deadlines remain unchanged**. Shared input is sent twice and can consume more subscription quota. This orchestration has not qualified complete-task latency or semantic accuracy.
- **Broker memory:** the configurable default is now 2 GiB, retaining the 2-CPU limit. The final observed peak was 1,106,071,552 bytes, above the former 1-GiB allowance. The live limit changed without restarting the broker or interrupting its connection. Saved before/after/final snapshots show no memory-limit hits or OOM kills. This is capacity headroom, not proof that resource pressure caused the validation deadlines.
- **One actionable chat error:** the browser replay exposed the same failure twice beneath the assistant. The UI now suppresses the duplicate global alert only when its text matches the latest failed assistant turn. Keyboard focus moves to that turn and the existing live region announces failure. A separate transport error remains visible and receives focus when the assistant contains different partial output. No automatic retry or provider fallback was added.

The source-review policy and full required report schema from pass 10 remain in place. The rejected wording/schema experiments below are not deployed.

## Live journeys

The first deployed word-budget fix was tested before parallel review was added. Campaign and project both failed validation, after **129.308 s** and **131.856 s**, and published no artifact. Campaign had no local length rejection: both its initial draft and factual repair passed the word ranges. Its source review took **52.021 s** and repair **26.280 s**, leaving too little of the 90-second check budget for re-review. Evidence: `journeys-02.json` and `journeys-02-stage-events.json`.

The final replay used the deployed parallel-review gateway:

| Scenario | First source review | Factual repair | Initial turn elapsed | Result |
| --- | ---: | ---: | ---: | --- |
| Campaign, 24 segments / 2 reports | 39.598 s | 30.844 s | 140.848 s | Re-review reached the validation deadline; no artifact published |
| Project, 14 segments / 2 reports | 33.345 s | 23.504 s | 141.139 s | Re-review reached the validation deadline; no artifact published |

Neither campaign run re-entered a word-count repair loop. The final campaign's first review was faster than the earlier observation, but its whole task was slower and still failed. Project also failed. These measurements do **not** establish an end-to-end performance improvement. Both failures remained visible and actionable, with no accepted result or success receipt. Their follow-up/reload assertions could not run because the initial deliverable failed. Evidence: `journeys-03.json`, its summary/stage events and the inspected failure screenshots.

These are individual observations, not latency percentiles or five consecutive successful qualifications. Customer and sales retain their latest pass-10 live evidence; they were not replayed in this pass. No recording media is approved.

## Source-review evaluation and rejected experiments

The shortened schema experiment let the model report every examined paragraph as `no_assertions`, including factual paragraphs. It produced four false acceptances in seven completed cases before being stopped. The schema and relaxed parser fields were restored before deployment. Its three nominally correct binary outcomes do not establish proper claim review.

A wording experiment first scored **11/12** in a focused canary. Its full run then scored **71/78**, including four false acceptances and three false rejections. On the original 70 cases it fell to **64/70**, versus pass 10's **67/70**, with six previously passing cases failing. The wording was removed before deployment. `review-full-01.json` describes that rejected experiment, not the deployed reviewer. The source snapshot is retained privately in `source-validation-experiment.rs`.

The subsequent parallel-review canary scored **6/7**: all four new batch coverage/cross-group controls passed, and the unsupported campaign was rejected. One valid saved sales report was falsely rejected. All 13 responses used Terra/subscription; there were no route or protocol errors. This result remains in `review-batches-01.json` and does not supersede the full evaluation.

The **final complete evaluation scored 72/82**, with **six false acceptances and four false rejections**. All **84 responses** used Terra/subscription. There were no protocol errors, timeouts or route violations in this component run. It is not a complete product-journey run.

| Failure | Case IDs |
| --- | --- |
| False acceptance: unsupported impossibility, terminology, ownership or population expansion | `partial-work-impossibility`, `absence-impossibility`, `sales-terminology`, `risk-impossible-held-out`, `risk-unrelated-owner`, `majority-expanded-population` |
| False rejection: supported experience, conditional scheduling, majority arithmetic or dependency chain | `experience-supported`, `project-conditional-pass8`, `majority-eight-of-eleven`, `batch-cross-group-valid` |

On the original 70 cases, the final run scored **63/70**, below pass 10's 67/70. Five previously passing cases failed and one previously failing case passed. The four new batch controls scored 3/4 in the full run after scoring 4/4 in the focused canary. The saved sales case passed in the full run after failing the canary; the valid project case did the reverse. Several changed outcomes use the unchanged single-report path, so these observations do not isolate batching as their cause. They do establish that the reviewer remains inconsistent and cannot support a correctness claim. No additional full rerun was used to replace these failures with a better-looking score.

All 38 prior saved cases remain unchanged. Eight new paired controls cover quantified sample scope and approval status; four more cover batch completeness and cross-group dependencies. Together with the unchanged 32 built-in cases, the final matrix contains **82 cases**. Full evidence: `review-full-02.json` and `review-full-02-summary.json`. Reported zero token fields are unavailable accounting data, not evidence of free inference.

## Verification and runtime

- Model Gateway: **1,265 tests passed, zero failed, one ignored** (`gateway-unit-05.log`). New regressions cover shared word budgets, metadata/notes, Unicode and LF/CRLF, overlapping reviews with complete context, index coverage, selective retry, cross-group rejection and cancellation without partial results.
- Frontend: typecheck and scoped ESLint passed; **17 existing accessibility checks passed**. The maintained browser tests passed **4/4** for duplicate validation errors, distinct transport errors with partial output, truncated streams and delayed subscription loading. They assert keyboard focus, announcements and a single explicit invocation. These UI tests mock inference and do not count as live model quality evidence. Corrected and partial-error screenshots were inspected.
- Browser environment: `http://localhost:5173/chat`, maintained Playwright `product-readiness` project, 1440 × 1000 desktop viewport. The browser plugin was unavailable during this pass, so the existing Playwright workflow was used. The frontend was restarted after the live journeys and its served module was checked for the UI fix before the mock tests. The initial journey launcher failed on PowerShell reporter-argument quoting before browser/inference startup; that harness error was corrected and retained.
- Deployed Model Gateway image: `sha256:f10cfc895854810e89c039d602abe2abad4160043f44af09e6b3bbbdcd6dc653` (`gateway-build-04.log`, `gateway-deploy-02.log`). Inference Core and Integration API images are unchanged from pass 9. The broker has 2 CPUs / 2 GiB. `runtime-final.json` records exact images and service health.
- All **13 recording fixture hashes are unchanged**; no source facts, expected verdicts or acceptance ranges were relaxed. `final-check.json` records source fingerprints, fixture/case comparisons, scoped diff validation and service health. Recording remains blocked, with zero approved media.

Private evidence: `C:\Users\ImaFernandesDaCosta\.codex\visualizations\2026\09\19\01a0b94f-be34-7aa3-8714-e90cb61181bc\q13-q15-pass11`. Auth state and the private Compose helper contain credentials and are not report artifacts.

## Remaining priorities

1. **Make semantic review dependable before treating its receipt as release evidence.** Preserve all ten failing cases and their valid/invalid controls. Resolve unsupported impossibility, population scope, terminology and ownership without rejecting valid majority statements, conditional dates or dependency chains. Check factual-segment coverage as well as binary verdicts; the rejected short-schema experiment shows why a nominal acceptance can conceal skipped claims. Validate any change on the unchanged full matrix and independent repeated cases, always using Terra/subscription.
2. **Complete campaign and project inside the task budget.** Review, repair and complete re-review currently exhaust 90 seconds. Reduce avoidable initial claims and repair work, then investigate version-bound evidence reuse only with explicit invalidation for changed source, contract and cross-section dependencies. The current two-report split alone is insufficient. Do not publish a candidate, bypass a failed review or claim latency success because one stage becomes faster.
3. **Requalify complete journeys after those fixes.** Initial deliverable, follow-up, exact revision preservation and authoritative reopening must all pass on the same build. Retain failed attempts. Then collect five consecutive independently accepted runs per scenario and the 20-sample complete-task latency cohort before approving recording.

The broader gates also remain: authoritative atomic run/result recovery, connected business-data prerequisites and audit delivery, device/recovery review and reviewed recording media. The latest passing customer/sales observations and this pass's unit/UI results do not close those gates.
