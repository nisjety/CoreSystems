# Recording pass 2: supporting captures and source-review corrections

**A private sales film now joins the customer preview. Campaign and project still time out; public release remains blocked.** This pass follows [recording pass 1](PRODUCT_RECORDING_CAPTURE_PASS1_2026-09-22.md). All live inference used `gpt-5.6-terra` through `openai-codex-subscription`, with no other model/provider or API fallback.

## Changes and retained evidence

- Subscription source-review reports now use compact object keys while retaining the same verdicts, complete segment coverage, evidence line references and rejection reasons. The parser normalizes these reports into the existing strict validation contract. Missing/duplicate/unknown fields, invalid evidence and ineligible confirmations remain rejected. Full document context, reasoning effort and deadlines are unchanged.
- The first compact-report candidate, checker v9, passed 13 targeted source checks. Its context-repair control run exposed a real false rejection: an unambiguous pronoun in one paragraph was treated as unresolved despite its antecedent in the preceding paragraph. The original run is retained. Three negative controls were also misreported by the test diagnostic because it recognized only the old long field names. Offline interpretation of those same responses gives 5/6, not a new inference pass.
- Checker v10 explicitly resolves references across consecutive paragraphs/headings of the current document, including changed antecedents. The diagnostic accepts both report formats without weakening the required rejection of the unchanged dependent claim. Two additional exhibition-date controls test preserved and changed references in a second domain.
- Supporting recordings now include source/result/revision/reload shots, readable result framing, editorial holds, source hashes, image identity and timestamp markers. Acceptance is set only after the original scenario assertions and reload checks finish. Failed takes remain private evidence.
- The exporter accepts all four maintained scenarios, requires a successful subscription-only capture and raw-file hash-bound edit plan, and keeps the 45–60 second customer / 25–40 second supporting duration requirements. It preserves the uninterrupted raw recording, uses chronological cuts and discloses the private rehearsal and removed waiting time.
- The v10 project attempt exposed a supported verdict with no source span. Its correction round then timed out. Checker v11 adds separate schema branches for supported/non-factual/unsupported verdicts: supported requires evidence, non-factual requires empty evidence, and unsupported requires a reason. The local parser still validates source IDs, line IDs, complete coverage and semantic verdicts. Nested `anyOf` is a documented [Structured Outputs schema feature](https://developers.openai.com/api/docs/guides/structured-outputs); the actual subscription route is verified separately by the live tests.
- The first sales take passed functionally but exposed a real layout defect: a vertical artifact list consumed too much of the result pane, while fixed table columns broke labels and figures into tiny lines. Document tabs now remain above the report until a 1,024-pixel pane is available; artifact tables use content-based column sizing and horizontal scrolling. The camera uses the existing keyboard resize control and holds both sides of wide tables. A read-only check of the saved sales artifact passed on desktop/mobile, with zero generation requests or page errors. Desktop report width was 741.625 pixels. The local Vite container required a restart to serve the changed CSS.

## Verification so far

- Gateway unit tests: **1,274 passed, one intentionally ignored** on the final v11 candidate. The compact-report coverage includes 33 source-validation tests, including equivalence and malformed-report cases; the protocol diagnostic regression also passed.
- Scoped strict frontend TypeScript and ESLint checks passed for the final capture changes, checker expectations, configuration and exporter.
- v9 targeted live source checks: **13/13 passed**. Reports became smaller, but individual timings varied; this does not establish an overall latency improvement or repeatability.
- v10 context-repair controls: **8/8 passed** in 70.73 seconds. The six original cases and two added date-reference cases preserve valid claims and reject invalidated dependent claims. These are fault-injection rechecks: the initial review and repair are scripted; only the subsequent recheck uses live inference. They are not eight complete authoring journeys. The diagnostic's local regression test also passed.
- v10 campaign rehearsal: **failed** with `response_validation_timeout` on the initial result (141.258 seconds; complete browser test 152.501 seconds). A local body-length failure required a 22.017-second repair; the subsequent two-batch review of 25 segments did not finish within the remaining artifact deadline. No accepted artifact, revision or completed reload is claimed. The failed raw recording is preserved.
- v10 project rehearsal: **failed** with `response_validation_timeout` on the initial result (131.272 seconds; complete browser test 138.406 seconds). The second reporting batch omitted evidence for a supported verdict; its protocol correction did not complete within the artifact deadline. The failed take is retained.
- v10 sales rehearsal: **passed** its source/numeric/length/revision/reload assertions. The first framing is editorially unsuitable because of the observed narrow report and table layout; it remains evidence rather than a final film.
- v11 context-repair controls: **8/8 passed** in 73.79 seconds. The same original/additional labels and unchanged evidence requirements were retained.
- Complete v11 source-review matrix: **93/94 correct** in 975.23 seconds, with **one false rejection, zero false acceptances and zero missing verdicts** in this set. All 96 completed responses used the required Terra subscription and requested reasoning budget; no experiment overrides were applied. The failed test is retained, not rerun into a passing result.
- The remaining false rejection is `project-conditional-pass8`: a conditional earliest-possible invitation date was treated as an assertion that approval had been granted, even though the candidate explicitly said no date was approved. This separates a computed lower bound from permission to execute. It remains a semantic blocker; passing the other cases does not prove the checker is universally correct.
- An independent JSON Schema validator accepted all 96 recorded responses against their actual requested schemas. **12 positive/negative format controls passed**, including supported-without-evidence, non-factual-with-evidence, unsupported-without-reason, unknown fields and empty evidence groups. These are protocol checks, not additional factual approvals.
- **34 recording-policy/publication-contract tests passed**. Four current CLI checks confirmed failed-capture rejection, blocked release, existing-export preservation and existing-take preservation without inference. All 13 business-fixture hashes remain unchanged, and all six raw takes from this pass are retained.

The final v11 gateway is `sha256:448d5768f79d0d018a01227b7a07c89300ea46d47e0166281f881ae39ccc7ae2`. The v10 attempts used `sha256:5055fae0bde2df1604a90e9f3afa3262fdea67f1c6a94bc529337c94811bfc5b`; v9 used `sha256:16ee0d3e0bce51fcda2fa74168f54666bbb3dc1da0bfbedfab869e4c6fcc9241`. Inference Core and Integration API are unchanged from pass 1. The frontend uses its existing source mount and image. Source fingerprints identify each candidate; no failed attempt was replaced or relabelled as a pass.

## Final-build supporting attempts

| Take | Result | Initial request | Follow-up request | Complete browser test |
| --- | --- | --- | --- | --- |
| Sales 02 | Passed | 108.118 s | 63.420 s | 226.561 s |
| Project 02 | Validation timeout | 135.979 s | Not reached | 142.529 s |
| Campaign 02 | Validation timeout | 117.386 s | Not reached | 123.711 s |

These are serial, single rehearsals with no concurrent live task or build. They do not establish a latency percentile, overall speed improvement or five consecutive passes. Sales 01 also passed on v10 (101.323 s initial, 35.776 s revision), but its framing was editorially rejected. The second take was justified by the layout/camera fix and changed runtime; failed campaign/project attempts remain preserved.

The project initially required an 18.812-second repair. Its subsequent review took 67.955 seconds and found two more unsupported passages, leaving too little time for another repair. Campaign's first review took 72.105 seconds and found three unsupported passages; its repair took 16.563 seconds, leaving about one second for the required whole-document recheck. Neither final-build failure was a report-format rejection. No unchecked candidate was exposed as an accepted artifact.

## Sales film and playback

`sales-preview-01/` contains a **35.04-second** H.264 short film, **224.48-second** uninterrupted VP8 raw recording, a real report poster, eight short-film caption cues, ten full-recording cues, and private provenance. Both videos are silent and 25 fps. The raw is 1440 × 900; the short is 1440 × 960 with the fixed rehearsal/time-cut disclosure. Its raw SHA-256 is `327e9c3e7a6812f2f0bad9169dd55942cc69ae9ec8dc637e8f3b3e9ecf367a1f`. FFmpeg fully decoded both outputs and the raw copy remained byte-identical.

The edit uses real chronological footage: instruction, source file, summary, both sides of the report table, the exact REGI follow-up, revised note and server-restored note. Source/result/revision frames and actual output text were reviewed. The report preserves weighted margins, the distinction between offers and invoiced sales, and uncertainty about causes. The concise note preserves those qualifications and proposes a first follow-up without claiming it was sent or executed.

The private loopback preview at `http://127.0.0.1:5291/` uses the actual production `ProductRecordingPlayer` with the earlier customer and new sales media. Playback checks passed for task and short/full switching, pause of the replaced video, no autoplay or initial video fetch, keyboard play, Norwegian caption loading, seeking/range responses, 390-pixel mobile layout and reduced-motion preference. There were no browser errors. The server rejects requests for provenance, capture metadata, edit plans and authentication state. This is an isolated review page, not a public homepage deployment.

The customer film remains from v8; sales is from v11. Their runtimes must not be compared as a controlled speed benchmark. Full-screen review is still needed for reading a desktop capture on a small phone. The actual app's source preview remains above short revised documents; it was not removed from the footage.

Private evidence: `C:\Users\ImaFernandesDaCosta\.codex\visualizations\2026\09\19\01a0b94f-be34-7aa3-8714-e90cb61181bc\recordings-pass2`. Evidence includes build/unit logs, `review-focused-01.json`, the original `recheck-controls-01.json`, its separately labelled offline format audit, and source fingerprints. No authentication state or credentials belong in media exports or the public product section.

## Release status

All public media fields remain null and qualification counts remain zero. Private rehearsals are not five consecutive qualified release runs. Campaign/project deadline handling and the remaining semantic error must be resolved; repeatability, recovery, performance and final four-film editorial/device review also remain open. The earlier customer preview remains a v8 recording; it must not be presented as evidence of the newer runtime.

Next priorities:

1. Correct the distinction between a conditional earliest date and actual approval, with paired valid/invalid controls and full-document checks. Keep the current failing case and label intact.
2. Reduce the campaign/project cycle of rejected drafts, repairs and full rechecks within the existing deadlines. Both authoring and review already receive computed source facts and conditional schedules; adding those again is not a missing integration fix. Preserve source coverage and factual checks while profiling the slow stages.
3. Capture successful campaign and project journeys only after those defects are resolved. The successful sales preview is ready for private editorial review; failed raw takes are diagnostic evidence.
4. Run five consecutive complete scenarios on one selected build and finish recovery, performance and final media/device review before public approval. No result in this pass establishes flawless operation.
