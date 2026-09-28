# Pass 10: live ChatGPT Terra qualification and bounded repairs

**Later checkpoint:** [Pass 11](PRODUCT_RECORDING_Q13_Q15_PASS11_2026-09-22.md) records the next subscription-only improvements, failed campaign/project replays and final 72/82 source-review evaluation. This pass-10 record remains historical evidence.

**Status: improvements deployed; recording and release remain blocked.** The user's ChatGPT connection resolves the missing-connection prerequisite from [pass 9](PRODUCT_RECORDING_Q13_Q15_PASS9_2026-09-21.md). The latest complete customer and sales journeys pass. The final campaign replay still fails at its validation deadline after alternating length and source corrections. Project validation and semantic-review reliability remain open. These results do not establish flawless behavior.

## Route and scope

Every live inference in this pass uses **`gpt-5.6-terra` through `openai-codex-subscription`**. The authenticated local test account has one active scoped connection; its ZDR policy is false. No Claude, Balance, other subscription model or paid API fallback was used. Tool decisions, authoring, private reviews and repairs retain the selected route. All 67 model responses recorded by the final 70-case component evaluation report Terra and the subscription provider. Cases rejected by local calculations need no inference. Unit and browser transport fixtures use mocks and consume no provider quota.

Private evidence directory: `C:\Users\ImaFernandesDaCosta\.codex\visualizations\2026\09\19\01a0b94f-be34-7aa3-8714-e90cb61181bc\q13-q15-pass10`. Authentication state and the private Compose helper contain credentials and are not report artifacts.

## Implemented fixes

- **Subscription broker resources.** Integration API was still limited to 0.25 CPU and 256 MiB while spawning Codex app-server subprocesses. The local Compose defaults now allow 2 CPUs and 1 GiB, configurable through `INTEGRATION_API_CPU_LIMIT` and `INTEGRATION_API_MEMORY_LIMIT`. Before recreation, cgroup counters showed CPU throttling and 130,064 memory-limit hits, with no OOM kills. Afterward, the observed peak reached 466,403,328 bytes, exceeding the old allowance; memory-limit hits and swap were zero in the saved post-change snapshot. Only Integration API was recreated for this change.
- **Required customer artifacts.** An explicit bounded customer reply with internal notes must reach a checked artifact write before its tool phase can finish. An accepted single draft can complete without another model decision round. Direct chat/no-tool instructions keep their existing answer path; mixed requests do not acquire unconditional early completion.
- **Local customer word counts.** The runtime checks the actual customer body, excluding separate internal notes, before publishing. Missing notes or an empty body cannot evade that check. The model no longer needs separate prose-counting calls for this supported contract. Provisional word-count arguments also no longer expose unchecked draft text through progress events.
- **Native review and repair schemas.** Private subscription reports now carry the broker's native output schema, including a consistent required `reason` field. Previously a project repair returned malformed JSON and consumed another inference call. Exact segment coverage, allowed indexes, real source spans and whole-result revalidation still run locally; structured JSON is not semantic proof.
- **Separate bounded repair allowances.** Two length/structure corrections no longer exhaust the source-review allowance. Each stage permits at most two repairs. The existing **90-second artifact-check deadline and 150-second tool-phase deadline remain unchanged**. Unchanged patches, invalid reports and final validation failures still fail closed. The regression covers length corrections followed by a factual correction, and proves a persistently invalid document cannot borrow the source-review allowance.
- **Subscription loading state.** A persisted Terra selection waits for its connection lookup before enabling Send. Keyboard submission during that lookup preserves the draft and does not announce disconnection. The connected user previously received an incorrect reconnect alert before campaign inference even started. Loading completion does not automatically submit the draft.
- **Accessible terminal states.** The chat live region distinguishes completed, failed, stopped and still-pending answers. A failed validation or truncated response no longer announces “Svar fullført.”
- **Qualified risk review.** The reviewer distinguishes conditional operational consequences from assertions that an obstacle currently exists. Additional paired controls retain unsupported benefits, invented current states and categorical impossibility as failures. The full evaluation still found one false acceptance in this area; the rule is not considered generally solved.

## Live journey results

Durations below are individual browser observations, not release latency percentiles. An initial failure means revision and reload were not reached.

| Journey | Latest result | Initial / revision |
| --- | --- | --- |
| Customer reply | Passed: one checked artifact, same-artifact revision, preserved internal notes, server reload and version history | 40.042 s / 40.691 s |
| Sales report | Passed: independent CSV arithmetic, weighted margins, narrative checks, condensed revision and server reload | 69.101 s / 38.589 s |
| Campaign | Failed: a source correction required another length repair, leaving insufficient time for full re-review; no draft published | 127.782 s / not reached |
| Project plan | Failed: source review, repair and re-review exceeded the 90-second document-check budget; no draft published | 133.245 s / not reached |

Customer and sales results are in `journeys-02.json`. Their initial documents and revisions were inspected. The earlier contradictory sales cost definition was absent from both fresh sales outputs. The final budget change leaves their previously exercised successful paths unchanged, but these are still single-pass observations rather than five consecutive qualifications.

The final campaign replay (`campaign-04.json`, final gateway image) reached a local length repair (12.989 s), source review (40.098 s), a correction of three rejected segments (19.039 s), then another length repair (13.813 s). The source correction had broken the word-range contract. Full re-review started with about four seconds left and hit the unchanged 90-second artifact-check deadline. Both repair stages ran with their separate allowances, and every repair report parsed on its first attempt. This verifies that the shared allowance and malformed-report obstacles were removed in this attempt, but does not establish successful campaign completion. No artifact, unchecked draft body or success receipt was emitted. `campaign-04-stage-events.json` retains the restricted timing metadata. The next campaign change must preserve all document constraints during factual repair and reduce review latency; further retries alone will not fit the budget.

Retained failures explain the changes: `customer-01` timed out after redundant counting; `customer-02` returned plain chat without an artifact; `customer-03` created a valid checked artifact but the test accepted only digits instead of the valid Norwegian words “ni” and “tre”. The quantity assertion now accepts either spelling without changing the expected values. `scenarios-01` passed sales but timed out campaign and project. `journeys-02` passed customer and sales, exposed the subscription-loading race before campaign submission, and still timed out project. `campaign-03` reached inference after the UI fix, but two local length repairs exhausted the shared allowance before a source correction could run. Its final unsupported draft was withheld after 95.358 seconds.

The four matched review cases took 279.93 seconds before the resource change, including two timeouts, and 49.82 seconds afterward with no timeouts. This small sequential replay supports the resource fix; it is not a p95 or load-capacity certification.

## Source-review evaluation

`review-full-01.json` records **67/70 correct**: **one false acceptance, one false rejection and one protocol failure**. No timeout or route violation occurred in that run. The original 62 cases scored **60/62**, compared with the historical pass-8 **54/62**; all 30 saved source/candidate/label entries are unchanged. Six former failures now pass, two remain, and no previously correct original case regressed. The eight additional paired controls scored **7/8**. Pass 8 used a different model and runtime configuration, so this comparison does not isolate the effect of a prompt change.

| Unresolved case | Observed failure | Consequence |
| --- | --- | --- |
| `risk-impossible-held-out` | Accepted “rollout cannot happen Monday or Tuesday” because the owner is absent, although whether required signoff is already complete is explicitly unknown | Unsupported scheduling certainty can still pass |
| `experience-supported` | Rejected “most interviewed managers” despite a source documenting five of six; its reason itself acknowledged that five of six is most | Valid content can be rejected |
| `status-saved-whole` | Both reports assigned source citations to `no_assertions` segments, violating the reporting contract | Failed closed as a protocol error; not counted as a correct rejection |

The earlier focused 12-case run scored 11/12; the final schema canary scored 4/4. Neither supersedes the full-matrix failures. The final matrix used the final reviewer policy and schema before the subsequent repair-budget separation; that separation changes artifact repair orchestration, not the component review function evaluated here.

## Verification and deployment

- Model Gateway library: **1,258 passed, 0 failed, 1 ignored** (`gateway-unit-07.log`). Intermediate runs are retained, including the new regression's incorrectly segmented mock report, which was corrected before the passing run.
- Frontend: **24 targeted tests passed**, typecheck passed, scoped ESLint had no errors and one existing dictation reactivity warning.
- Browser transport/recovery: **4/4 passed**, including a deliberately delayed connection lookup, failed validation, truncated transport and provider failure. These tests pin Terra but mock inference; no fallback or automatic second invocation occurs.
- The source-mounted frontend was restarted after the component evaluation. Served JavaScript was checked for both new UI behaviors before the browser replay.
- Final gateway image: `sha256:1048a4ec7170baefc84fa90849efab7dd3442f1ccc472db4d8a5e151837a033a`. Inference Core and broker images remain the pass-9 builds. `runtime-final.json`, `source-fingerprints-final.json`, `frontend-served-final.json` and `final-check.json` record the final state.
- All **13 recording fixture hashes remain unchanged**. No source facts or marketing acceptance ranges were relaxed. Recording approval remains blocked and no media was approved.

## Next priorities

1. Resolve the unsupported rollout acceptance, the contradictory “most” rejection and the reporting-contract failure. Enforce verdict/evidence consistency without treating malformed reports or unsupported claims as accepted results. Preserve the paired controls and full shared-case comparison.
2. Make campaign factual repairs preserve the already-checked word ranges and structure, and reduce repeated full-review latency without weakening source checks. For project planning, reduce source-review/repair latency and improve first-candidate consistency within the existing budget. Local CSV and conditional schedule evidence already reach both authoring and review; adding the same calculations again is not the missing implementation.
3. Complete authoritative atomic run/result recovery, five consecutive independently accepted journeys per scenario, the 20-sample complete-task latency cohort, connected business-data prerequisites, device/recovery review and reviewed recording media. Customer/sales passes and routing fixes do not close these release gates.
