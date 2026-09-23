# Recording pass 3: source-review and repair corrections

**Campaign now has a successful complete rehearsal and a private preview film. Project still times out.** This pass continues [pass 2](PRODUCT_RECORDING_CAPTURE_PASS2_2026-09-22.md). Public release remains blocked. All live inference in this pass uses **gpt-5.6-terra through openai-codex-subscription**. No Claude, Balanced or API fallback is used.

## What changed

- A conditional earliest-possible date is explicitly distinguished from approval, permission and a guaranteed execution date. The original failing label is retained, with paired valid/invalid controls in another domain.
- Long semantic reviews are divided into two to four bounded reporting groups, weighted by text size and segment count. Every group still receives all source lines and the complete document. Cross-group references and conflicts must be checked; all pending groups are cancelled on timeout. Reasoning remains high (4,096 tokens), with the same 90-second artifact and 150-second turn deadlines.
- An initial unsupported verdict may include minimal private text patches. Patches must identify unique, non-overlapping spans in failed segments, cover every rejected segment, and preserve all other bytes. They consume the existing repair budget and never count as acceptance. The complete edited document must pass local checks and a separate semantic review. Subsequent reviews request verdicts only, separating repair suggestions from acceptance.
- Markdown table data rows now receive individual semantic verdicts and repair targets. Headers and all other rows remain in context. Local arithmetic, schedule and weekday checks retain complete table blocks. Tests cover CRLF offsets, repairs following expanded tables, preservation of untouched bytes and the existing segment limit.
- Reviewers must identify every unsupported assertion in a segment rather than stop after the first. Campaign edits receive exact section-body word allowances. Author instructions distinguish task ownership from approval authority, unknown status from non-occurrence, and task completion criteria from later release gates. Campaign guidance keeps copy inside its body-length range and requires sourced features or explicitly hypothetical situations.
- Context-repair controls now require exact expected text on positive cases and explicit rejection of the unchanged dependent claim on negative cases. Two additional controls exercise real subscription review suggestions and independent rechecks; the existing eight controls retain scripted initial failures/repairs.

## Diagnosed failures and retained attempts

The project trace showed a nine-row table receiving one verdict. Its first review caught an invented approval role but missed a separate rule incorrectly narrowed from **all technical faults** to **blocking faults**. The next review detected that defect, requiring another repair after most of the deadline had elapsed. Another unsupported statement treated undocumented approval as an explicit decision not to approve. Row-level reporting and author instructions address these concrete defects.

The campaign trace showed an unsupported claim about external publication history and a feature description expanded into an unstated functional capability. A suggested correction also shortened one post below its minimum length, requiring another generation before the full recheck. Word allowances now accompany suggested corrections; the existing exact local word-count gate remains enforced.

Candidate history is preserved rather than replaced by later results:

| Candidate | Component evidence | Recording outcome |
| --- | --- | --- |
| v12 | 15/15 targeted semantic cases; 8/8 context controls | Project and campaign timed out before an accepted initial result. |
| v13 | A new real inline repair passed, but the context set scored 8/9: a changed antecedent's stale dependent date was falsely accepted. | No product recordings were attempted on this candidate. |
| v14 | 10/10 strengthened context/inline controls; 12 actual response schemas valid | Project and campaign still timed out. Campaign initially met body limits, but its post-repair full review exceeded the remaining time. |
| v15 | 3/3 targeted cases, including four-group dependency controls; 9 actual response schemas valid | Project and campaign failed. Project completed a second review that found two further unsupported segments. |

Separate v15 diagnostic captures are not qualifying product runs. The first project diagnostic failed because a temporary local forwarding proxy mishandled an unbounded gRPC deadline; that transport defect was corrected and the failed attempt retained. The later project and campaign diagnostics exposed the content defects above. The proxy forwarded the same subscription model and recorded only allowlisted fictional fixture/review content. It is stopped, and the original Compose file and direct Inference Core route have been restored. The ordinary v15 campaign's detailed gateway stage log was lost during the diagnostic restart; its browser report and raw recording remain, and no detailed stage timing is inferred for it.

## Final candidate verification

Checker **attachment-source-review-v16** is built and running locally. Gateway image: `sha256:bbd439960ca93e14c49c0020a791a465cf946d658a3f2195dbd67a164340daa2`.

- Gateway unit tests: **1,279 passed, one intentionally ignored**.
- Recording-policy/publication-contract tests: **34 passed**.
- Scoped strict frontend TypeScript: **passed**.
- Complete semantic matrix: **104/104 correct**, with zero false acceptances, false rejections, missing verdicts or incomplete required table-row rejections. This includes all original cases, the original conditional-date failure and the two traced project documents. The sum of case times was 1,092.792 seconds (complete native test 1,142.03 seconds). All 122 completed responses used the required model/provider and 4,096-token review budget; no experiment overrides were applied.
- An independent JSON Schema validator accepted all **122 actual responses** against their requested schemas, with **12/12** positive/negative protocol controls passing. These checks validate report structure, not factual correctness by themselves.
- Final context/repair controls: **10/10 passed** in 102.73 seconds. Eight use fault injection before a live independent recheck; two exercise a real initial review, inline suggestion and independent acceptance recheck. These are component controls, not ten complete authoring journeys.
- All **12 actual repair-control responses** passed independent schema validation; **16/16** format controls passed, including the optional initial-review edit branches.

## Final-build recording results

| Take | Result | Initial request | Revision request | Complete browser test |
| --- | --- | --- | --- | --- |
| Project v16-01 | Validation timeout | 138.286 s | Not reached | 149.171 s |
| Campaign v16-01 | Passed | 70.010 s | 62.341 s | 201.077 s |

Project's first four-group review took 57.623 seconds and rejected three segments. The fallback author repair took 12.220 seconds. Its full recheck began but only one group finished before the artifact deadline. No accepted initial artifact, completed revision or successful reload is claimed. The original failed take is retained. These timings identify the remaining repair/recheck budget problem, not the exact content of this latest candidate's rejected claims; the detailed content diagnosis above concerns the separately identified v15 traces.

Campaign passed the maintained source, body-length, date, revision-preservation and server-reload assertions. Its initial and revised source reviews took 26.251 and 32.015 seconds respectively, with no private repair cycle. The REGI follow-up changed the 23 September post for a shared office while preserving the other two posts and the email. All output remained draft/proposed content; the test did not publish, schedule or send a business communication. This is one successful rehearsal, not five consecutive qualified runs or a latency percentile.

Inference Core remains `sha256:4a91fd8dd9f529313518ec8470ea3424756900d98ed99a7c42ec235578f3f8d3`; Integration API remains `sha256:4ac588775c24a2f659dbadc2aec2f0a205788327e080c3bc813da9d6939b8d0b`. Source fingerprints record every candidate. Build and live inference work run separately; live scenarios are serial. All business fixtures and expected labels remain unchanged; new cases are additions.

Private evidence lives in `C:\Users\ImaFernandesDaCosta\.codex\visualizations\2026\09\19\01a0b94f-be34-7aa3-8714-e90cb61181bc\recordings-pass3`. Raw attempts, original failures, native reports, actual requested schemas and stage timing records remain there. Authentication state and private configuration are not marketing assets.

## Campaign film and playback

`campaign-preview-01/` contains a **30.32-second** H.264 short film, **199.36-second** uninterrupted VP8 recording, a real result-frame poster, seven short-film caption cues, ten full-recording cues and private provenance. Both videos are silent and 25 fps. The raw is 1440 × 900; the edit is 1440 × 960 with the rehearsal/time-cut disclosure. Its raw SHA-256 is `69020d58d667b0de3f6838e80ad3b282b2a8a15ce26ef9b55fc03ef0cf0a93cb`. Both files fully decoded in FFmpeg, and the raw copy is byte-identical.

The edit uses reviewed chronological footage of the request, source file, publication overview, LinkedIn copy, email, exact follow-up, revised post and server-restored post. Waiting is removed only from the short edit and disclosed; the uninterrupted recording retains it. The opening composer shows the end of the longer prompt, while the subsequent conversation/source shot shows the complete request. No result text or frames were replaced.

The private loopback preview at [127.0.0.1:5292](http://127.0.0.1:5292/) combines the existing customer and sales films with this campaign film using the actual production `ProductRecordingPlayer`. Playback checks passed for three-task and short/full switching, pausing the replaced video, no autoplay or initial video fetch, keyboard playback, Norwegian captions, seeking/range responses, 390-pixel layout and reduced-motion preference. No browser errors occurred. Private metadata/authentication paths return 404. Desktop and mobile screenshots were reviewed; reading the desktop document on a small phone still requires full-screen viewing. The previous previews remain available. This isolated review page is not a public product-section deployment.

## Release status

The existing customer preview is from v8, sales from v11 and campaign from v16. Their runtimes are not a controlled comparison. All 13 fixture hashes remain unchanged; the 20 public media fields remain null and qualification counts remain zero. Eleven raw attempts from this pass are retained, including the diagnostic transport failure.

Next priorities:

1. Resolve the project first-draft defects and the repair/recheck budget problem. Diagnose the exact new failed segments and why an initial inline proposal was unavailable or unusable; do not infer those reasons from counts or extend deadlines to claim success.
2. Capture a successful complete project journey. The three available films remain private editorial rehearsals.
3. Establish five consecutive qualified complete runs per scenario on one selected build, then finish performance, recovery, durability and final editorial/device review before publication. The 104-case semantic result does not establish universal correctness or flawless operation.
