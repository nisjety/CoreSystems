# Q01–Q04 execution record

Date: 2026-09-19. Scope: the first four tasks in [the readiness plan](PRODUCT_RECORDING_READINESS_2026-09-19.md). These are engineering and acceptance results for the local stack, not production certification or approval to publish recordings.

**Result: Q01–Q04 implemented and locally verified.** The final maintained browser run passed **6/6 cases**. Scenario 01 produced and revised the same document, reopened its source and both document versions from the server, and a separate gateway-restart check revised an existing v2 to v3 without tool failures. Q05–Q09 remain open.

## Changes

| Task | Delivered |
| --- | --- |
| Q01 — acceptance baseline | Authenticated setup verifies active organization, completes onboarding through the owning API, checks HTTP readiness and opens the actual chat. Image identities and source hashes are recorded separately. Vite dependency discovery is restricted to the SPA entry and its watcher excludes unrelated apps/caches. Docker health now checks HTTP through the BFF instead of accepting an open but stalled TCP socket. |
| Q02 — attachment handoff | File preparation completes before inference or draft reset. Unreadable files, failed extraction and browser-storage exhaustion retain the draft/files with a visible error. PDF/DOCX parsing uses an authenticated, ephemeral Ingestion endpoint; DOCX tables retain their position among paragraphs. CSV/MD text is sent in the same turn. Normal pending launches are tab-local; temporary launches stay in memory. Sending a chat file no longer silently promotes it into organizational knowledge. |
| Q03 — compaction | Local recovery remains available for Haiku, unknown models, native-off and provider fallback. Native support is limited to verified Sonnet/Opus families. A typed checkpoint travels through inference, is stored with the assistant turn under retention policy, and is restored for the next request. Pins are protected in system context. Checkpoints never become visible answer text or OpenAI-compatible message fields. Usage includes compaction iterations once. Tool decision rounds do not create checkpoints that cannot be persisted. |
| Q04 — customer draft | A maintained browser case uses the unchanged scenario 01 prompt/source and exact REGI follow-up. It checks artifact identity/version, word limits, source facts, tool failures and server-backed reload. Fixes restore readable attachment sources after reload, render tables within quoted Markdown, include exact artifact IDs in tool receipts and later-turn context, distinguish a fictional sender from workspace identity, and provide computed calendar facts for dates in the brief. The artifact viewer now merges revisions from separate server turns while retaining existing snapshot history. |

Relevant implementation entry points: `tests/e2e/product-readiness.spec.ts`, `tests/e2e/local-auth.setup.ts`, `src/features/chat/lib/prepare-chat-attachments.ts`, `src/features/chat/lib/pending-chat-launch.ts`, `src/features/chat/components/chat-normalizers.ts`, `apps/gateway/src/domains/chat.rs`, Ingestion `imports-core/app/chat_extract.py`, Model `inference-core/src/provider/anthropic.rs`, `model-gateway/src/compaction.rs`, `session_flow.rs`, `calendar_context.rs` and `proto/model_plane/v1/inference.proto`.

## Verification

- Frontend attachment and handoff regression group: **17 passed**. Markdown, restored sources and controller regression group: **91 passed**. Final artifact history and panel group: **59 passed**, including out-of-order server revisions and retained snapshot history. TypeScript compilation and the final production build passed.
- Focused ESLint checks passed for the attachment/handoff files, artifact helper/tests, panel test and maintained browser spec. The broader changed-file check still reports the pre-existing KaTeX `innerHTML` lint error and four Solid reactivity warnings in `chat-media-markdown.tsx`; it is not a clean repository-wide lint result. Production build also retains the large-chunk warning, for the Q08 performance review.
- Frontend gateway: **529 passed, 1 ignored** across library and integration checks. Action registry/dispatcher contract passed with **168 actions**. Product truth contract: **2 passed**.
- Ingestion extraction: **7 passed** in a temporary container using the service dependency environment; live browser cases additionally exercised a real PDF and a DOCX table.
- Model: **286 inference-core tests and 1,183 model-gateway tests passed, 1 ignored**. Workspace library compilation and generated Go contract package checks passed. The later exact-artifact-ID receipt and recovered artifact-inventory regressions passed; **5 computed-calendar tests passed**.
- Linux release images for Frontend gateway, Ingestion imports, Model inference and Model gateway were built and deployed through the canonical plane environment/overlay resolution. The existing Windows-only compilation failure in inference-core's Unix-signal binary path was not changed; library checks and Linux release builds passed.

### Live native compaction and restart

A separate direct-provider contract probe first crossed 165,019 counted input tokens and replayed the returned native checkpoint successfully. That result alone was not treated as application evidence.

The application test then used authenticated same-origin chat requests, the normal Model services and Session Core, with fictional repeated background material and a real durable message pin. No direct database writes were used. Six bounded input turns grew the prompt to **159,062 billed input tokens** on the compaction turn. After recreating both inference-core and model-gateway, the follow-up used **1,911 input tokens** and retained FF-1042, 9 packed / 3 pending, K3, the QA marker and the distinction between warehouse arrival and customer delivery. The checkpoint was absent from visible answer text. Follow-up wall time was **7.94 seconds**; compaction-turn wall time was **17.72 seconds**.

This proves one live Sonnet route and this recovery case. Haiku/native-off/unknown/fallback behavior is covered by contract tests, not a claim that every provider has been exercised live. ZDR persistence behavior is unit-tested; the current catalog does not attest an available ZDR model, so the UI correctly disables temporary chat.

### Browser attempts and defects retained

| Attempt | Outcome |
| --- | --- |
| Preflight | Onboarding and actual chat passed. |
| live-01 | Frontend HTTP stalled after a rebuild; authentication timed out. |
| live-02 | All four file formats reached the model from chat and dashboard. Scenario 01 revised the same artifact and survived server reload. One failure assertion used the wrong UI selector. Visual review found raw source text in the reloaded user bubble and a quoted Markdown table rendered as pipes. |
| live-03 | HTTP readiness exposed the recurring Vite startup stall before any scenario ran. |
| live-04 | Setup and all attachment cases passed. Scenario 01 exposed an incorrect weekday, wrong fictional-company signature and an artifact-read call using an invented ID. |
| live-05 | Setup and attachment cases passed; signature and artifact ID issues were corrected, but the weekday was still wrong. Prompt guidance alone was insufficient. The order-reference assertion was also corrected to allow the document subject/title to carry the reference. |
| live-06 | Setup and attachment cases passed. The weekday was correct, but the draft calculated its next update from the real clock rather than the dated fictional brief. Added an explicit computed scenario clock; strengthened deadline assertions. Also fixed the test to distinguish a subject/header separator from the internal-notes boundary. |
| live-07 | Setup and attachment cases passed; the initial customer draft passed factual/calendar checks. The revision invented an artifact ID because earlier tool receipts were absent from subsequent-turn history. Added a bounded authoritative artifact inventory, rebuilt and deployed Model gateway. |
| live-08 | **6/6 passed**, with no failed tool steps. An independent server-only reopen then found the version selector had lost v1 even though the server retained it. Fixed aggregation across turns and added unit/browser regression coverage. |
| live-09 | Setup and attachment cases passed. A test assertion rejected a valid layout: the internal source note followed the customer document in the assistant message instead of inside the document. The assertion now accepts either clearly separated location; factual and length requirements remain. |
| live-10 | **6/6 passed in 2.0 minutes**, including source reopening and v1/v2 navigation after clearing both browser transcript stores. No page errors or failed tool results in the scenario. |

The original scenario files and control notes were not rewritten to make the test pass. The existing search toggle is explicitly off for these file-based acceptance cases. This is not a deterministic attachment-only source-isolation guarantee; that remains Q07.

### Final customer journey and recovery

The final run used the ordinary `verevon-balance` profile, resolved to **claude-sonnet-4-6**. Thread `01M2WW8AFDSJYZDJ3K23ZSHK5C` created artifact `svar-ff-1042` v1 and updated it to v2. End-to-end wall times were **42.97 s** for the draft and **28.95 s** for the revision. The tool sequence was `create_artifact`, then `read_artifact` / `update_artifact`; no send or order mutation occurred. Manual review against the control notes confirmed 9 packed / 3 pending, 17 September as an unconfirmed warehouse estimate, no confirmed customer delivery, next update on Tuesday 15 September, K3 as current and K4 as expired. Internal notes retained the logistics and cost questions.

After forcibly recreating Model gateway from the same image, an independent browser context reopened the earlier live-08 thread `01M2WVPWGVAWJEN69NG3CXWCZ0`, navigated its v1/v2 history, and submitted the same revision instruction. Artifact `ff-1042-svarutkast` advanced **v2 → v3**, survived another server-only reload and retained version navigation. This check took **24.49 s** including reload, with zero tool failures and zero page errors. It proves recovery from the gateway's empty in-memory artifact store, not just reuse of the active process.

Screenshots were inspected for the actual draft, source text, internal notes and version controls. The narrow results pane, floating feedback control, mixed-language metadata and broader responsive presentation still need the Q08/Q09 finishing review; these screenshots are acceptance evidence, not finished marketing assets.

## Evidence and remaining gates

Evidence is under `C:/Users/ImaFernandesDaCosta/.codex/visualizations/2026/09/19/01a0b94f-be34-7aa3-8714-e90cb61181bc/q01-q04/`: baseline and final runtime JSON, build/test logs, direct-provider and application compaction probes, per-attempt screenshots and browser traces, plus `artifact-restart.json`. Final runtime metadata records six healthy participating services and image IDs separately from 209 modified source/config hashes and 13 fixture hashes. The source snapshot includes pre-existing user work; it is not a clean release commit or an authorship list. Raw traces remain local because they may include session headers; use the selected fictional output and screenshots for review, not raw traces as marketing assets.

Repeat the maintained acceptance suite from the Verevon v3 root with `pnpm exec playwright test --project=product-readiness --trace on --output <evidence-directory>`. The local fixture and owning services must be running; do not rebuild/restart them during a run. Preserve each failed attempt rather than overwriting its output directory.

Current attachment limits are **1 MB per file**, **60,000 extracted characters per file**, **120,000 total extracted characters**, and a bounded pending-launch payload. Scanned PDFs without readable text are rejected. Durable PDF/DOCX recovery supplies an honestly named extracted text copy; it does not claim to retain the original binary. File sending does not create a knowledge-base document; that remains a separate explicit action.

Q05–Q09 remain: scenarios 02–04, five consecutive fresh-thread passes per scenario on the chosen build, memory/connected-source isolation, broader recovery/performance measurements, then recording and product-section publishing. The readiness plan's latency budgets are still targets; these small samples do not establish p50/p95 or production reliability.
