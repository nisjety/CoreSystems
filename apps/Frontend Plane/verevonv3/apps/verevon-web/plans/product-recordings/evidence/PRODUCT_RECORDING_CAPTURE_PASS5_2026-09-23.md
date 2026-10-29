# Recording pass 5: high-effort subscription review and project completion

**Release remains blocked.** The project scenario still has no complete passing initial request, exact REGI revision, server reload or film. The customer, sales and campaign films remain private rehearsals from earlier builds. Every live inference in this pass used `gpt-5.6-terra` through `openai-codex-subscription`; there was no Claude, Verevon Balanced or API fallback. Failed takes remain failed, and none counts toward the five-run release streak.

## Changes kept

- Source-review requests now ask the existing ChatGPT subscription broker for its priority service tier while retaining **high reasoning effort**, the selected Terra model, subscription connection and privacy scope. Ordinary chat and other providers do not acquire this preference. Priority is a latency request, not a correctness verdict or an extended deadline.
- A single source-checked project-plan draft can end the internal tool-selection phase from its accepted artifact when the prompt requests only that draft. Positive send, publish, calendar-create, extra-summary or separate-question requests keep the normal tool path. The existing deterministic artifact completion still binds the final receipt to the created version.
- The project browser oracle now accepts a proposed export date explicitly conditional on later schema approval. It still rejects an affirmative approved/decided status, including one followed by another condition.

The experimental one-call compact recheck from v21 was **removed** after a failed live take. The production recheck retains complete context, explicit coverage and independent high-effort judgment. The 90-second artifact deadline was not changed.

## Fresh no-retry project rehearsals

| Take | Source review | Complete browser result |
| --- | --- | --- |
| v21 | 26 segments; first review 46.163 s, four inline edits; experimental compact recheck did not finish before deadline | Failed with `response_validation_timeout`; compact experiment removed |
| v22 | 21 segments; high-effort priority first review 36.449 s, three edits; independent recheck accepted in 32.640 s and the artifact tool succeeded | Failed after artifact creation: a redundant tool-selection inference exceeded the turn allowance; motivated the bounded completion fix |
| v23 | 23 segments; first review 46.071 s, three edits; only three of four recheck batches returned before deadline | Failed with `response_validation_timeout` |
| v24 | 20 segments; first review 39.360 s, two edits; independent recheck accepted in 27.266 s and the initial artifact completed | Browser oracle falsely rejected a truthful proposed date conditional on schema approval; oracle fixed, but this take remains failed and has no revision/reload proof |
| v25 | 20 segments; first review 29.465 s, two edits; recheck finished in 38.446 s but found further unsupported content | Further private repair could not finish before deadline; failed closed with `response_validation_timeout` |

The five private reports and raw failed browser recordings are under `C:/Users/ImaFernandesDaCosta/.codex/visualizations/2026/09/19/01a0b94f-be34-7aa3-8714-e90cb61181bc/recordings-pass7/`. Source-reviewed artifacts were not promoted to approved media by a later browser failure. In v25, the recheck's new rejection shows why a fast first patch is not itself sufficient evidence.

## Verification and remaining work

- Gateway library after the final completion-rule guard: **1,280 passed, one ignored**, including its focused mixed-request regression. Focused subscription-adapter tests: **7/7**. Focused recording-policy and oracle tests: **38/38**; frontend strict TypeScript passed.
- The final gateway image was rebuilt and its local container is healthy alongside the updated inference core. No new complete project journey was claimed for this final rebuild; v25 remains the latest recorded rehearsal.
- The high-effort priority Terra reviewer passed the complete labeled semantic matrix **104/104**, including saved project and cross-group cases. Independent repair/context controls passed **10/10**. These are component accuracy controls; they do not establish full-task reliability or a product latency percentile.
- The next implementation priority is to prevent unsupported claims in the first project draft and make a corrected complete document reliably pass independent recheck within the existing deadline. Recheck must continue to detect changed references, approval state and cross-section conflicts. Then obtain one complete initial/revision/reload journey with the exact source pack and REGI follow-up on a stable build.
- All four scenarios still have **0/5** consecutive qualified runs on one release build; all **20/20** public media fields remain null. Performance p50/p95, recovery, restart/new-browser durability, device/accessibility and editorial/media checks remain open before publication.

See the maintained [recording plan](../README.md), [workflow](../RECORDING-WORKFLOW.md) and [pass 4 baseline](PRODUCT_RECORDING_CAPTURE_PASS4_2026-09-23.md).
