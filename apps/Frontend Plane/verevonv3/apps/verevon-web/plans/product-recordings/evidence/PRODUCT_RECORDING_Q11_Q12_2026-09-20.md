# Q11–Q12: exact answer checks and artifact revision receipts

Date: 2026-09-20. Status: first implementation slices deployed to the local development stack; release and recording gates remain open.

## Changes

The Model Gateway now checks supported short direct answers before releasing their text on the regular chat SSE path. The initial contract recognizes one explicit Norwegian/English word range in the user's first instruction paragraph, for a direct chat answer. It records the original requirement span and counts Unicode words, allowing internal hyphens/apostrophes. Only standalone section labels explicitly requested by the user are excluded. It does not promote attachments, quoted material, or tool output to requirement authority. Separate ZDR, agentic, vision and JSON invocation paths are not certified by this slice.

The exact candidate must pass. A failed candidate receives at most two repairs, within a 60-second check budget, using the same provider selection, authorized context, privacy floor and ZDR flag. The checker does not call business tools or a code sandbox. It rejects truncated responses, hidden-comment padding, Markdown link destinations and code fences for this plain-prose contract. Exhaustion produces `response_validation_failed`, with no successful completion or accepted candidate text. Ordinary unconstrained answers retain their streaming path.

Successful checks produce a receipt binding the answer hash, instruction hash, context hash (including compaction checkpoints), requirement, count, checker version and attempt count. Whitespace is canonicalized before checking so checked, delivered and persisted bytes match. Repair token counts are recorded separately; this is not a replacement for the existing provider billing ledger. The receipt is appended to the same authenticated Session Core message as the answer and emitted after durable completion. Writing/checking progress and the completed word count use existing step events. This proves the named local check, not factual accuracy, language quality or complete task compliance.

Simple single-artifact revisions now finish with a localized version receipt: artifact title and the actual previous/new version. This removes a separate unrestricted final-answer model call for those revisions. The receipt binds the actual artifact and summary hashes and never invents claims about preserved sections, completed actions or document content. Mixed requests/questions, tool failures, multiple artifacts and initial artifact creation retain the existing answer path. The artifact ID, version history, copy/download and reload behavior remain the existing product surfaces.

## Verification

- Model Gateway library suite on the final source: **1,204 passed, 1 ignored**.
- Nine focused tests include the authoritative saved 20-answer September 19 cohort: **all 20 incorrect lengths are rejected** (80–99 words). The previously reported 6/20 valid answers came from mojibake in DevTools response-text captures and is superseded. The fixture now comes from the durable UTF-8 messages. Tests also cover limits, Unicode/formatting, request authority, changed-content rejection, bounded repair, truncation, streamed-candidate assembly, privacy/provider preservation and the historical revision-summary contradiction.
- Playwright failure UI: passed. A failed check presents an error and retry affordance, keeps the composer available and does not silently invoke another model.
- Initial live smoke: the server rejected a 93-word candidate and repaired it into range. The first persistence assertion used `metadata.resultReceipt`; the API intentionally flattens metadata onto the message. The test was corrected to read `resultReceipt`. The failed test report remains retained.
- An intermediate 20-run cohort passed the length/persistence assertions but reused one cached answer through inference-core's unary response cache. Its sub-second p95 is **not independent-generation performance evidence**. The checker was changed to collect the normal provider stream, preserving chat's existing response-cache posture; no shared cache was flushed and no cache-busting prompt text was added. `latency-20.json` remains retained as diagnostic evidence only.
- An intermediate customer run emitted and persisted the correct v1 → v2 revision receipt. Its added oracle incorrectly compared client-assembled history with one server-stored version; the assertion now compares only that version's id/title/content/version. The campaign run took 228.335 seconds for its initial turn and exceeded the scenario timeout during follow-up. Eight code-interpreter calls preceded artifact creation. It is a retained failure, not a performance pass; the run overlapped a local build and is not a controlled latency benchmark.
- Final streaming cohort: **20/20 delivered answers meet the 100–120-word range**, with 20 distinct outputs (104–116 words), no business/code tool calls, and matching durable receipts. All required one repair. The original browser report shows 18 passes and two failures because its uncertainty regex missed the valid phrase “ikke en bekreftet”; that oracle is corrected. All 20 receipts and counts were independently checked against authoritative saved messages. This is a length-conformance result, not full factual acceptance.
- Final-build browser checks: live cancellation before accepted text, validation-error recovery, and the complete customer initial/revision/reload/mobile flow all passed (4/4 including authentication). Customer initial/revision completion took 38.483/19.457 seconds; these are single observations, not p95 values.
- The QA response capture now uses browser Fetch's UTF-8 decoder, and compares captured answer text to the visible transcript. This prevents Norwegian encoding corruption from silently changing future evidence and word counts.
- Three additional live runs with the corrected capture passed every existing automated assertion (4/4 including authentication); captured text, visible answer, saved message and receipt agreed. These confirm the collector, not a new 20-run performance qualification.
- A customer thread created before the final Model Gateway restart was reopened in a fresh browser context and revised from **v2 to v3**. The new receipt, durable artifact and reloaded version matched. The first harness attempt submitted before history had loaded; waiting for the saved assistant message resolved that harness race. The successful attempt took 19.384 seconds including reload/screenshot; it is not a latency benchmark.
- Durable-message correction for all historical cohorts: first **1/20**, second **10/20**, final **0/20** within range, replacing the earlier misdecoded counts of 7/20, 8/20 and 6/20. Original evidence files remain retained.

### Final streaming workload timing

Twenty sequential fresh-thread runs on the unchanged final Model Gateway build; no overlapping builds or other test suites during the cohort. Existing provider prompt caching was retained; no application answer cache was added.

| Metric | p50 | p95 | Maximum |
| --- | ---: | ---: | ---: |
| Acknowledgement | 0.134 s | 0.209 s | 0.219 s |
| First accepted answer text | 9.867 s | 11.277 s | 11.618 s |
| Completion | 12.436 s | 14.231 s | 14.914 s |

First accepted text arrives later than the previous unchecked first token. That cost is explicit; the first-token target is not passed by this slice. Timing does not imply factual correctness.

### Manual content review

The final direct answers still contain unsupported inferences in several samples. Examples: samples 1/11/13/15/17 infer that nobody was notified because no invitation was sent; samples 10/12/14/15/19 invent a dependency preventing budget or invitation work before technical clarification; sample 17 assigns the next decision to Amir although the brief only assigns him technical clarification. These are retained **Q13 failures**. A passed word-range receipt must never be described as source verification or full acceptance.

Final local Model Gateway image: `sha256:06af7f3d34eb813ae0a58dcd26bd396526b09a850d8820f787878069f0f5da3c`, started `2026-09-20T01:12:16Z`. No production release or recording approval. Intermediate image `1097700a…` is not the final streaming validation build.

Evidence directory: `C:/Users/ImaFernandesDaCosta/.codex/visualizations/2026/09/19/01a0b94f-be34-7aa3-8714-e90cb61181bc/q11-q12/`. Browser authentication state and raw network traces are private and are not published with this record.

## Scope still open

These are the first deployable slices of [Q11/Q12](PRODUCT_RECORDING_NEXT_STEPS_2026-09-20.md), not completion of their entire acceptance matrix.

- Extend requirements to artifact sections, per-section limits, preserved text, address/language, dates and numeric invariants. Unsupported or ambiguous requirements receive no local-pass receipt today.
- Validate claims against allowed source spans and distinguish uncertainty, recommended actions and completed actions. The known customer/campaign factual failures are not solved by a word-count receipt.
- Do not confuse an accurate version receipt with successful preservation of every requested section. Exact section-preservation enforcement and repair staging remain open.
- Model restart, fresh-context revision and reload have one successful customer observation. Replay and concurrent-revision proof for the new receipt lifecycle, plus five repeated complete journeys on one release build, remain open.
- Measure the cost of withholding unchecked text. First verified text is expected to arrive later than the old unvalidated first token. Full customer-task latency and all four journeys still need repeated measurements on an unchanged build.
- Connected-data prerequisites, audit-outbox delivery, broader recovery/accessibility and approved marketing recordings remain open as documented in Q05–Q10.

Next implementation slice: Q13 source-backed claim checks and exact preservation of sections the user asked to keep, followed by Q14 phase timing and latency work. Keep publication blocked until the complete journeys meet their acceptance gates.
