# T3 category 2: Multimodal and creation (cross-source comparison)

107 reference records from 11 sources were grouped into 40 capability clusters. Every record is in a cluster; none were excluded. Verevon was compared against 41 baseline records that were checked in code, plus the baseline narratives. Full detail is in `02-multimodal-creation.clusters.json`.

Provenance tags: SRC = read in source, DOC = documentation, INF = inference. Hermes has no records in this category.

## Shared baseline (table-stakes)
- **File attachments (C-001).** Multi-format upload with published limits, processed outside the context window.
- **Dictation (C-008).** Speech-to-text into the composer, streamed live on the best sources.
- **Image generation (C-013).** Includes a choice of model and typed results.
- **Office files (C-020).** Creating docx, xlsx, pptx and pdf through skills in a sandbox.
- **Data analysis (C-027).** Sandboxed analysis of files the user uploaded. The sources here are documentation only.
- **Common but below table-stakes:**
  - turn-based voice mode (C-010)
  - image editing (C-014)
  - slide decks (C-021)
  - video generation (C-016)
  - meeting minutes (C-019)
  - web and site builders (C-034)

## Best of breed
| Capability | Best source | Why |
|---|---|---|
| Attachments and limits (C-001) | claude.ai, plus the Claude desktop document funnel | Clear PDF tiering: visual pages up to 100, text-only to 1,000. Files are converted in an isolated scratch area (SRC + DOC). |
| Attachment store (C-002) | DeepSeek harness, plus T3 Code on the client | Content-addressed store with variants per model route; events carry only references. Upload queue, HEIC conversion and compression in the client. |
| Capability gating (C-003) | DeepSeek `read_image` and the Antigravity MIME table | Fails closed before any I/O. The send button refuses media the selected model cannot take (SRC). |
| Dictation (C-008) | Claude desktop and Antigravity | One streaming state machine with a short-lived credential and a CSP lock. 16 kHz PCM sent in 40 ms chunks, with a cleanup pass (SRC). |
| Realtime voice (C-012) | Codex | The voice model only delegates to the governed agent, through one tool plus a silence tool, over WebRTC (SRC). |
| Image generation (C-013) | Codex skill, plus Cursor's guards | Typed result item and no silent model fallback. Cursor adds a concurrency limit of 2 and an abuse screen (SRC). |
| Image editing (C-014) | Manus canvas; ChatGPT sets the UX bar | Mark tool with an instruction per region, plus upscale and background-removal jobs (SRC). ChatGPT Images is the reference experience (DOC). |
| Office files (C-020) | Claude skills, plus DeepSeek's checker | Skills for each file type in a sandbox. After writing, a structural check, a LibreOffice render and formula recalculation run before delivery. |
| Slide decks (C-021) | Manus Slides | Manifest owned by the tool, a state per slide, and a gate that blocks delivery until every slide validates. Edit per page, a presenter view, and charts backed by data. |
| Templates (C-022) | Codex template picker | A template is a skill plus a reference file plus a preview. Users pick from thumbnails or save their own upload as a template (SRC). |
| Collaborative documents (C-024) | Claude | Rust yrs CRDT and pulldown-cmark compiled to WASM. @AI in a comment edits the document with the requester's permissions. |
| Previews (C-025) | DeepSeek | Renderer registry keyed by file type, with explicit loading modes and caps that refuse oversized files instead of truncating them. |
| Annotation (C-032) | T3 Code | A typed payload (elements, regions, strokes, style changes) is sent to the agent as a prompt together with a screenshot (SRC). |
| Agent session recording (C-033) | T3 Code, plus Cursor's habit | Records the canvas at 12 fps with MediaRecorder. Agents start and stop recording as tools and attach the video as proof. |

## Differentiators worth noting
- Content credentials (C2PA) on generated files (C-015, Claude).
- Widgets inside the chat that send a follow-up prompt back (C-031).
- A voice front end that only delegates to the agent (C-012).
- Recorded walkthroughs as proof of work (C-033).
- Fully local speech-to-text (C-009, DeepSeek).
- Dashboards that show their queries (C-030, claude.ai, DOC only).

## Verevon's biggest gaps (tiers proposed)
1. **Office files (C-020): priority, significance 5, effort L.** Today they come only from ad-hoc `code_interpreter` scripts. There are no skills for each file type, no check or render-and-look step, and no LibreOffice. Attachments are not copied into the sandbox, and the time limit is about 30 s.
2. **Slide decks (C-021): priority, significance 4, effort L.** There is no deck model, no per-slide editing and no present mode. Office exports contain no charts.
3. **Attachments (C-001): priority, significance 4, effort M.** Limits are 1 MB per file and 60k characters. Files are reduced to text only. The durable upload route exists but nothing calls it.
4. **Capability gating (C-003): priority, significance 4, effort S.** The ChatGPT subscription route strips images and attachments without telling the user. Vision is a one-off call that replaces the turn.
5. **Data analysis (C-027): priority, significance 4, effort M.** The sandbox cannot see the user's uploads and keeps no state between calls.
6. **Recommended:**
   - image editing (C-014, absent)
   - live-proving image generation and adding a second provider (C-013)
   - editable documents, starting with an editable artifact phase (C-024)
   - previews (C-025)
   - templates (C-022)
   - widgets that send prompts (C-031)
   - annotation (C-032)
   - session recordings, produced by Quarry, which runs the browser (C-033)
   - C2PA (C-015)
   - streaming dictation (C-008)
   - the attachment store (C-002)
   - research-to-deliverable chaining (C-035)
7. **Skip:**
   - video timelines (C-017)
   - generated audio (C-018)
   - a site builder or hosting (C-034); only its approval pattern and its access-controlled publishing are worth reusing
   - telephony (C-040), which belongs to the future Channel Plane

## Verevon strengths (keep)
- **Charts (C-029).** Accessible SVG charts (role=img, a data table, keyboard navigation, never colour alone) and table-to-chart. Ahead of the references on accessibility.
- **Export (C-026).** Word, Excel and PowerPoint are built in the browser with no network call, which works well under ZDR. PDF goes through the print path.
- **Voice (C-010, C-011).** Push-to-talk voice and read-aloud run through the governed loop with a Norwegian default voice. Voice is blocked in temporary chat, as ZDR requires.
- **Screenshots (C-006).** One-shot screenshot attach.
- **Governance.** Verevon has a hermetic bwrap sandbox, typed refusal codes and checks of drafts against their sources. These give it the governance foundation that most references lack.

## Caveats
- **Same-vendor sources.** The 11 sources include two pairs from the same vendor, Claude desktop with claude.ai and Manus desktop with manus.im web. Their prevalence counts overstate how many vendors have a feature; each affected cluster's note says so.
- **Documentation-only clusters.** C-004, C-017, C-027, C-030, C-031, C-035 and C-039 rest only on documentation (DOC), so confidence was not raised for them. Table-stakes was assigned by adjustment for C-001, C-013, C-020 and C-027, as explained in the JSON.
- **Statuses still to verify.** Two statuses are "unknown" (C-004 audio ingestion, C-030 dashboards) and need a check in code. Absence of C2PA, templates and meeting capture was confirmed by grep on 2026-10-13. The image-generation guards (C-013) and the translate action (C-037) also still need a code check.
- **Stale baseline record.** v-planned-decisions-025 marks charts as partial on the basis of a stale document. Code shows `ChatChart.tsx` is shipped.
- **No live proof.** The baselines record no live proof for Verevon's image, vision, voice or Office outputs, so "shipped" means shipped in code only.
