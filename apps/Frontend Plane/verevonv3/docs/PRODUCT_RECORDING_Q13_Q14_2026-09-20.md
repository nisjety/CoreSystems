# Q13/Q14 execution record — source review and preserved revisions

Date: 2026-09-20. Status: qualification failed; recording/publication remains blocked. The historical first-pass evidence below is retained. See the [latest Q13–Q15 checkpoint](PRODUCT_RECORDING_Q13_Q15_PASS4_2026-09-20.md) for subsequent fixes, current test results and the next implementation priorities. This is a scoped continuation of [Q11/Q12](PRODUCT_RECORDING_Q11_Q12_2026-09-20.md), not completion of every Q13/Q14 acceptance item.

## Changes

- Short answers already covered by the direct word-range contract now receive a source review when the explicit conversation-only scope contains readable text attachments. The exact accepted candidate must pass both checks before it is released.
- For supported Markdown artifacts in that scope, review/repair occurs before the existing audited dispatcher assigns a version or publishes the artifact. Discarded repair candidates do not become visible versions. At most two repairs are attempted per candidate, with bounded validation time; an exhausted check produces an actionable failure.
- Review input contains numbered paragraph/table blocks of the exact candidate and numbered lines from user-attached source snapshots. Assistant prose, organizational policy and tool output are excluded as evidence. Every block needs an outcome covering all its assertions. Factual outcomes require references that the gateway resolves to exact spans in an allowed source. The review explicitly distinguishes uncertainty, necessary conditions, work ownership, proposed actions and documented completion.
- The receipt binds content, source snapshot and reviewed segments and records source offsets, accepted-review duration and token use. This is a fallible semantic review, not proof of factual truth. Its scope is distinct from the deterministic word-count receipt.
- Supported revision commands preserve the internal source-note section, or all content outside one explicitly dated post, by copying bytes from the prior artifact. Markdown headings, standalone bold labels and quoted bold notes are supported. Missing/ambiguous headings fail rather than silently dropping a requested section. Mixed edit requests are not reduced to a single-post update. A delivered mid-run correction updates the preservation instruction and available attachment snapshot. Revisions use the existing artifact identity/history and version receipt.
- A source-reviewed initial draft can complete with a deterministic pointer to Resultat, avoiding a second unrestricted description of its facts. Simple revisions retain their version-bound completion.
- A new local `count_words` tool counts multiple prose bodies together without a code sandbox or external credentials. Existing computation tools remain available for real calculations. Checked artifacts no longer solicit another model review/read-back merely to summarize them.
- A clearly marked customer body with a greeting and separate internal notes receives a final local check for an explicit `maks/max N ord/words` instruction, including after repair. Notes and subject metadata are excluded from that body count. Other layouts and campaign ranges are not claimed as locally enforced by this slice.
- A hash-bound source review avoids duplicate post-answer verification without increasing the confidence score. Questions inside attached evidence no longer trigger an unnecessary second answer when the actual user request only asked for a draft.
- Artifact checking observes cancellation before dispatch; reviewer and repair calls inherit the model route, tenant and retention/privacy constraints and have no business tools. Source text and discarded drafts are not added to a new store.
- The work panel now labels a stopped run as stopped even when an earlier counting tool finished successfully.
- Count requests are limited to two batches per delivered user request; remaining artifact tools stay available. This prevents repeated successful counts from consuming the entire tool phase. A genuine mid-run correction resets that allowance.
- Source review also evaluates the user's adopted brief/language-profile requirements. These remain fallible semantic checks, not a deterministic style/length contract. Genuine user requests are snapshotted separately from legacy tool-context messages, so runtime tool prose does not become a new requirement or attachment source.
- Successful checked-artifact and word-count events expose a short user-facing result in the work panel; private model instructions remain in the tool context.

## Verification

Final backend and live browser verification are in progress. An intermediate backend suite passed 1,210 tests with one ignored. The first three live workloads were withheld by validation; these are failed journeys, not acceptance passes. The customer and campaign each used one local word-count call and no code interpreter. Builds overlapped that diagnostic run, so its timings are not a performance qualification.

An exact-source quotation control subsequently passed. The original status workload still failed with invalid review evidence references. The review protocol was changed from model-transcribed quotations to supplied line indexes resolved by the gateway; invalid/missing/out-of-scope references still fail. Strict JSON schema and complete candidate-segment coverage remain required. Standard JSON code-fence wrappers are accepted, but surrounding prose or unknown schema fields are not.

The following intermediate run passed six checks including authentication, both direct-source cases, cancellation during source review, cancellation before accepted direct text and failure recovery. The customer initial draft passed, but its revision exposed the quoted-bold-note parser gap. Campaign review took 53.373 seconds for 69 separate lines and exhausted its checking budget after finding an unsupported claim. These observations prompted the heading fix and paragraph/table batching; the time budget was not increased. Both failed journeys remain in `functional-04.json`.

The updated backend suite passed **1,218 tests, one ignored**. The affected frontend suite passed **67 tests**, and project type-checking and targeted lint passed. The first frontend test invocation could not start its fork worker; running the installed Vitest directly with one thread worker completed the suite. The failed invocation is retained and is not counted as a test pass.

`journeys-06.json` retained three further failures. The customer initial draft completed in 48.991 seconds, but its revision performed eleven successful counts (all below 100 words) without updating the artifact. The campaign initial draft completed in 141.641 seconds but violated the requested plural address style. Source-review cancellation stopped without publishing an artifact, but the panel still said “Ferdig”: Vite was serving stale modules, confirmed by inspecting its served source. The local frontend was restarted and the changed module became visible. These observations prompted the counting allowance, requirement-aware review and public tool-result wording changes. Final live verification follows below; these failed runs are not recording or performance passes.

Manual review of that campaign also found unsupported benefits despite its semantic receipt: “tar liten plass”, moving the light without moving other equipment, and colours fitting different offices without refurbishment. This is an observed reviewer false negative, not just a theoretical limitation. The receipt must not be presented as editorial acceptance or a factual guarantee. The updated reviewer includes adopted task requirements, but independent review remains a release gate.

Evidence is retained under `C:/Users/ImaFernandesDaCosta/.codex/visualizations/2026/09/19/01a0b94f-be34-7aa3-8714-e90cb61181bc/q13-q14/`. Browser credentials and raw authenticated traces are private.

The next local build was Model Gateway `sha256:bda8b96e3bc67ecfeb97a665c9e26683d8e80d4abe745f18e76a40001b6291a5`, started at `2026-09-20T07:57:01.819067817Z` and observed healthy. `cargo test -p model-gateway --lib` passed **1,220 tests, one ignored** (`backend-08.log`). Targeted lint passed after the browser assertion changes. The frontend source restart was verified through both served modules. This is a local runtime update, not a production release.

`functional-08.json`: **five checks passed including authentication; four failed**. Both cancellation paths, actionable validation-failure recovery and the original direct workload passed. The direct workload produced 115 words in two attempts, with first accepted text at 19.312 seconds and completion at 22.203 seconds. The Norwegian and English verbatim controls failed review-format validation. User-request/style evaluation was subsequently restricted to documents; direct answer source review again receives only candidate/source data. Customer creation and revision completed in 44.816 and 44.788 seconds, with two revision counting calls and a new durable version. Exact note preservation still failed because quoted bold subheadings terminated the protected range; the parser now treats the entire quoted note as one container and has a regression for nested source/status labels. Campaign creation failed before publication after 154.371 seconds. These are single diagnostic observations, not percentiles; later compilation overlapped the campaign run. Build/test attempt 09 was interrupted to include the newly observed preservation fix in build 10.

Build 10 passed **1,221 backend tests, one ignored**, and was deployed locally as `sha256:21260d46ee47b1d5b75dc7f9b4ec2616198eeb88a65454bbf1070b0c156a09d6`, started `2026-09-20T10:22:20.322755578Z`, observed healthy. There was a long interruption while the deployment tool returned; runtime health was rechecked before starting the retest. The retest adds URL/title, nonblank screen, overlay and console checks to the existing revision/source/history interactions. No recording or production publication was performed.

### Final local observations

`functional-10.json`: **five automated checks passed including authentication; one failed**. This is not editorial or release acceptance.

| Workload | Observed result | Submit-to-completion |
| --- | --- | --- |
| Direct status, 100–120 words | Automated pass, 105 words, two candidates; manual source review below rejects an uncertainty implication | 30.945 s; first accepted text 27.758 s |
| Norwegian exact quotation | Exact text and source receipt passed | 10.601 s |
| English exact quotation | Exact text and source receipt passed | 10.097 s |
| Customer initial + revision | Same artifact v1 → v2; exact internal notes, ≤100-word revised body, persisted receipt, reload, source preview and version navigation passed | 44.334 s + 44.632 s |
| Campaign initial | Failed before publication; no follow-up qualified | 138.464 s |

The customer used one local count for creation and two for revision, with no code interpreter. Manual inspection of the final customer text retained the 9/3 split, provisional supplier date, unconfirmed collection/delivery, proposed logistics follow-up and next-working-day response. The internal note bytes matched the original, and the chat named the actual saved version. Desktop and mobile screenshots showed the result and history; the floating feedback overlay remains a visual defect as described below. Customer console/runtime errors and framework overlays were absent.

The campaign reviewer took **56.463 seconds for 42 blocks**, then returned incomplete coverage and exhausted the bounded validation path. The candidate was withheld, so no accepted campaign artifact or revision is claimed.

Manual review of the automated-pass direct status found “Disse to forholdene er uavklarte” referring to technical clarification and the budget. The source does not establish that technical clarification is incomplete/unresolved. This is another observed semantic-review false negative. The automated count/receipt assertions remain passed, but **the status copy is not editorially accepted**. Both exact-quotation controls passed after separating direct factual review from document requirement review.

No p50/p95 or cold/warm claim is reported: these are single observations. The ≥20-sample performance cohorts were not run because correctness and complete journeys are not stable. All failed attempts are retained. Final document checks found 12 status links, 13 unchanged source hashes, four historical runs, zero approved media and blocked readiness. Browser plugin was unavailable; existing Playwright was used at `http://localhost:5173`, desktop 1440×1000 and customer mobile 390×844.

### Immediate next implementation priorities

1. Give campaign pieces explicit section contracts and local final-body checks for address style, length and prohibited claims, including after repair. Remove count labels that are not bound to the final copy.
2. Separate invalid reviewer-format/coverage recovery from author-content repair. Reduce review output overhead, instrument all attempts and enforce a cumulative task budget. Do not increase timeouts or relax complete coverage merely to make this fixture pass.
3. Evaluate source entailment independently with the saved false negatives and valid quotation controls, especially unknown status versus incomplete status, conditions versus completed actions, and specifications versus benefits. A receipt cannot replace this acceptance work.
4. Close the mobile feedback overlap, Norwegian work-step labels and preservation-only progress wording, then resume complete-journey qualification. Recording remains blocked until the existing gates have current evidence.

## Remaining scope

- The source reviewer can miss errors or raise false alarms. Resolving source-line references prevents invented spans; it does not establish semantic entailment by itself. Independent labelled evaluation and manual review remain necessary.
- The initial source gate covers the existing short word-range path and supported authored Markdown documents. Ordinary unconstrained answers, other source modes, binary/code/HTML artifacts, unreadable/oversized evidence and separate agentic/vision/ZDR flows are not newly certified.
- Preservation parsing supports specific unambiguous commands and Markdown headings; it is not a complete natural-language contract extractor. Broader section identities, exact numeric/calendar invariants and every artifact body limit remain open.
- Word counting is local, but the author must still supply the actual body text. A count-tool result is not a receipt for the complete final artifact, especially after a semantic repair.
- Full phase instrumentation, cumulative task budgets, concurrent-edit proof, broader recovery and repeated complete-journey performance qualification remain open. No performance target is declared met by adding these mechanisms.
- The preservation-only branch outside conversation-source review still needs distinct progress/private completion wording; the current shared wording can overstate which checks ran. That branch is not qualified by these conversation-only journeys.
- Screenshot review of the final customer mobile view found the floating “Tilbakemelding” control overlapping body copy. The prose fits its panel, but this overlay is a separate Q15 usability defect; passing width assertions does not close it. Work-step tool titles also remain partly English in the Norwegian UI.
- The existing connected-data/audit prerequisites and all recording approval gates remain in effect.
