# Recording pass 1: real customer preview and preserved supporting attempts

**One private customer preview is ready to inspect. Public recording release remains blocked.** The customer and sales journeys passed; campaign and project both failed with `response_validation_timeout` before an accepted initial result. This recording pass follows [product pass 13](PRODUCT_RECORDING_Q13_Q15_PASS13_2026-09-22.md); it does not resolve that pass's semantic, latency or recovery blockers.

All live generation, review and repair used **`gpt-5.6-terra` through `openai-codex-subscription`**. There was no other provider or paid API fallback. No model runtime, provider deadline or business source fixture was changed. All attempts, including failures and the first poorly framed customer take, remain private evidence.

## Recording fixes

- Added explicit **private rehearsal** and **qualified release** modes. The old capture flag retains release gating. Rehearsals never grant publication approval. Both modes require private output outside the repository; the maintained runner resolves filesystem ancestors, rejects reused output/report paths and disables automatic retries.
- Added a customer filming sequence using the unchanged prompt, attachment and exact REGI follow-up. It checks fixture hashes, facts, word limits, the same artifact's version change, unchanged internal source notes, bound receipts, authoritative saved content and reload after clearing local transcript caches. It records image identity, route, timestamps and editorial markers.
- Corrected result framing: the first take passed its content checks but left the document mostly below the viewport. The second take uses the real workspace divider and scrolls the document into view. Reading holds make the instruction, source, first answer, revision and restored answer inspectable. Holds before submission are excluded from measured task latency. No answer or screen content is fabricated.
- Added a customer export command with a SHA-256-bound edit plan, chronological cuts, separate Norwegian action-description captions for short/full versions, an actual result poster and visible rehearsal/time-cut disclosure. It preserves the raw file byte for byte, decodes both outputs and verifies media duration. The final caption is bounded by encoded duration, including frame rounding.
- Fixed the public media approval contract to require an integer pass count of at least five. Missing values, numeric strings, fractions and non-finite values no longer pass through JavaScript comparison coercion.
- Updated the canonical manifest, README and [recording workflow](../RECORDING-WORKFLOW.md) to the September 22 evidence. Historical runs remain historical; all public media paths remain null.

## Captured attempts

| Attempt | Functional result | Recorded observations |
| --- | --- | --- |
| Customer 01 | Passed | Initial 40.128 s; revision 54.018 s; browser test 139.201 s. Editorially rejected because the result framing was poor. Preserved. |
| Customer 02 | Passed | Initial 38.245 s; revision 41.289 s; browser test 121.725 s. Readable first/revised result, source preview and reload. Used for the private short film. |
| Sales 01 | Passed | Initial 106.233 s; follow-up 40.244 s; browser test 154.752 s. Full diagnostic raw capture saved. It still needs dedicated filming/editing. |
| Campaign 01 | Failed | Initial request 120.049 s; browser test 123.305 s. Validation timeout; no accepted initial artifact, follow-up or completed reload journey. Failure footage preserved. |
| Project 01 | Failed | Initial request 133.074 s; browser test 136.441 s. Validation timeout; no accepted initial artifact, follow-up or completed reload journey. Failure footage preserved. |

These are individual diagnostic observations, not a five-run qualification or a latency percentile. Live scenarios ran serially with no other live inference or concurrent build. The repeated customer capture was justified by the observed framing defect; campaign/project were each attempted once in this pass, with no retries to replace their failures.

**Manual content review:** the customer draft keeps 9 of 12 lamps packed, three awaiting replenishment, provisional warehouse arrival distinct from customer delivery, pickup uncertainty, proposed logistics confirmation and the next-working-day update. Its revision preserves the internal source section exactly. The sales report's totals and weighted margins match the supplied CSV, and the concise follow-up retains the measured changes and first proposed investigation. Quotations remain distinct from invoiced sales; the initial report identifies causal uncertainty. No customer message, booking or public campaign was sent.

## Customer media and playback

The selected raw take is **121.08 seconds, VP8, 1440 × 900, 25 fps, silent**. Its SHA-256 is `ef55e08f06e2cf2430ee02dae2644093000cc74d8eca078db9b882e215e5b371`. The short film is **45.64 seconds, H.264, 1440 × 960, 25 fps**, with a 60-pixel disclosure strip. It contains real chronological footage and disclosed time cuts, without speed-up, substituted responses or a runtime claim.

The final private export is `recordings-pass1/customer-preview-02/`:

- `short.mp4`, `raw.webm`, `poster.png`
- `captions.vtt` (seven cues), `raw-captions.vtt` (ten cues)
- `media-review.json` with provenance, edit plan and asset hashes

Representative raw/edit frames, the poster/result, caption timing and the original/final text were inspected. The second export corrects the final caption endpoint after encoded-frame rounding; the first export is retained. An initial edit plan shorter than 45 seconds was rejected before export and corrected by keeping more actual footage.

A private loopback preview at `http://127.0.0.1:5290` uses the **actual production `ProductRecordingPlayer` component**, bundled in an isolated page with review styling and a single real customer recording. It is not a public homepage deployment. Its server exposes only the preview bundle and the five media assets, supports byte-range seeking and does not serve private provenance, logs or authentication state.

Browser checks passed for no autoplay, no initial video prefetch, keyboard play, loaded Norwegian captions, pause when switching short/full videos, desktop, 390-pixel mobile width, reduced-motion preference and seeking. There were zero browser errors in this playback check. Desktop document text is readable at the reviewed size; a desktop capture's document is too small to read in a narrow mobile embed without full-screen viewing. That limitation still needs editorial/device review for final marketing use. The attachment preview also remains above the document in the actual app; it was not painted out of the recording.

## Verification

- **34 unit checks passed** across recording policy and public media contract tests.
- **11 CLI refusal/preservation checks passed**, including blocked release, unsafe output paths, existing-take/export preservation, mismatched raw-file hash and empty full-recording captions.
- Scoped strict TypeScript and ESLint checks cover the capture configuration, helpers, specs, scripts and modified web contract. The first explicit TypeScript invocation required the compiler's `--ignoreConfig` flag; the corrected invocation passed.
- FFmpeg fully decoded both customer media files; ffprobe confirmed formats and actual durations. The copied raw file retained its original SHA-256. Browser caption tracks loaded all seven/ten cues.
- Public status remains `blocked`, all 20 public media fields are null, and no public recording is approved. Five-run qualification counts remain zero because these rehearsals do not establish the required complete release qualification.

Runtime remains gateway `sha256:472e143629bce267b2ded5c27a24c83d6d48f0452a176a9321abedd6c3593648`, Inference Core `sha256:4a91fd8dd9f529313518ec8470ea3424756900d98ed99a7c42ec235578f3f8d3`, and Integration API `sha256:4ac588775c24a2f659dbadc2aec2f0a205788327e080c3bc813da9d6939b8d0b`. Capture manifests also record the frontend image; it uses the existing source-mounted app. No runtime image was rebuilt in this pass.

Private evidence root: `C:\Users\ImaFernandesDaCosta\.codex\visualizations\2026\09\19\01a0b94f-be34-7aa3-8714-e90cb61181bc\recordings-pass1`. The private `preview.mjs` restarts the review server, and `playback-qa.cjs`, `cli-check.json`, `scenario-results.json`, `artifact-review.md` and capture reports retain the checks. No credentials or auth state were copied into the export or preview server.

## Remaining work

1. Resolve the campaign/project validation timeouts and the semantic errors documented in pass 13; current failure footage is not suitable for a success film.
2. Add dedicated readable result shots, source/revision holds and timed captions for the three supporting films. The sales diagnostic pass supplies functional evidence, not a finished 25–40 second supporting film.
3. Qualify five consecutive complete runs for every scenario on the selected build and finish the outstanding correctness, performance, recovery and device requirements. Keep all failed attempts.
4. Review all four final media packs, including small-screen legibility, before enabling them in the public product section. The current customer preview is useful for review, but the product is not established as flawless or ready for marketing release.
