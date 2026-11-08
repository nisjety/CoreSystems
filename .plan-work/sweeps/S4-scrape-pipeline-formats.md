# S4 scrape-pipeline-formats

Scope: Quarry `/v1/scrape` path plus its Model Plane and gateway callers, for F-t2-005, F-t2-009, F-t2-019, F-t2-023, F-t2-037, F-t2-050, F-t2-048. Read-only. Only this file was written.

Read from the LIVE tree `C:/dev/CoresSystem` (HEAD afb83928, plus uncommitted work). Read the "Surprises" before relying on any citation: several QB-T17 "gate" pieces the audit cites exist only as uncommitted or untracked files.

Tags: [C] committed and unchanged; [M] committed but modified in live tree; [U] untracked (exists only in working tree); [NEW] would be created by the plan; [UNVERIFIED] named but not opened. Tags come from `tools/provenance.sh`; files not run individually were checked against `live-uncommitted.tsv` (absent = [C]).

Path legend (all relative to `C:/dev/CoresSystem/`):
- `Q/` = `apps/Ingestion Plane/Quarry-v2/crates/`
- `QR/` = `apps/Ingestion Plane/Quarry-v2/`
- `MP/` = `apps/Model Plane/rust/services/`
- `GW/` = `apps/Frontend Plane/verevonv3/apps/gateway/src/`
- `IC/` = `apps/Ingestion Plane/imports-core/app/`
- `DP/` = `apps/Data Plane v2/services/`

## Cross-item overlaps (merge-conflict hotspots)
1. `Q/quarry-edge/src/routes.rs` [M] (1564 lines): F-005 ScrapeRequest/scrape 346-530; F-023 render and actions; F-048 cache at 432-487, internal_run_page 1029-1046, scrape_stream 1085-1141; F-050 persistence.
2. `Q/quarry-runtime/src/pipeline.rs` [C] (1503): F-005 clean stage 506-508 and formats 697-705; F-009 sniff 421-476; F-023 screenshot/pdf None 702-703; F-048 fingerprint 524; F-050 redaction before persist 601-683.
3. `Q/quarry-core/src/output.rs` [C] (243): F-005 OutputFormats 129-144; F-023 screenshot/pdf refs 139-141; F-048 cached value shape; F-009 content-kind metadata (PageMetadata 184+).
4. `Q/quarry-runtime/src/browser_driver.rs` [C] (581), `driver.rs` [C], `driver_plan.rs` [C] (95): F-005 signals; F-023 do_fetch hook; F-009 byte reuse.
5. `Q/quarry-runtime/src/answer.rs` [C] (2007): F-009 is_pdf 745-753 (lift); F-037 PDF branch 835-839.
6. `Q/quarry-edge/src/extract_routes.rs` [M] (639): F-019 runner gate 205-208 and json_for_org 286; F-005 schema/format contract; F-050 redaction before model.
7. `Q/quarry-runtime/src/mp_client.rs` [C] (715), `QR/crates/quarry-runtime/contracts/cross-plane-token-requests.json` [M], `Q/quarry-edge/src/config.rs` [M] (501): F-019, F-037 OCR hop and F-050 PII hop all wait on the same model-gateway grant.
8. `MP/model-gateway/src/quarry.rs` [M] (2515), `grpc.rs` [M] (4611), `http_routes.rs` [M] (11681): F-005 web_fetch inline (quarry.rs 761-778); F-019 ExtractStructured/invoke (grpc.rs 2516; http_routes 6693, 8908-9003); F-037 documents/layout (http_routes 434); F-050 PII route (http_routes 442, 1947, 4732).
9. `QR/docs/openapi.yaml` [M], `QR/docs/SELF_HOST.md` [M], `QR/docs/CROSS_PLANE_INTEGRATION.md` [M], `QR/docs/OSS_PARITY_BACKLOG.md` [C]: stale or false claims for F-005/009/019/023/037/050; the QB-T17 docs exist only uncommitted.
10. `GW/domains/search.rs` [C] (1078), `GW/domains/knowledge/quarry.rs` [C] (1158), `GW/cache.rs` [C] (201), `GW/domains/browser.rs` [C]: F-005 formats/maxPages caller contract; F-048 URL-only preview cache; F-023 stale tenant comment at browser.rs 1461-1463.

---

## F-t2-005 One strict ScrapeOptions contract: formats[], inline return (the ZDR path) and main-content controls

### 1. CITATION CHECK
24 load-bearing citations checked (card plus verification.evidence). Skipped: Firecrawl reference lines in `C:/dev/firecrawl` (the three cited files exist; line numbers not re-read) and duplicate entries.

| Cited | Tag | Verdict | Live finding |
|---|---|---|---|
| `Q/quarry-edge/src/routes.rs:345-375` ScrapeRequest | [M] | OK (346-375) | No `deny_unknown_fields`. Fields: url, prev_fingerprint, cache, zdr, privacy, signals, ingest, org_id, render (364-365), prefer_http3 (373-374). |
| routes.rs:131 only POST routed | [M] | OK | Line 131 `post(scrape)`; 132 `/v1/scrape/stream`. No GET /v1/scrape/:id. |
| routes.rs:398-530 scrape handler | [M] | OK | |
| routes.rs:1067-1110 scrape_stream | [M] | OK (runs to ~1150) | Cache read 1086; put_with_ttl 1141. |
| routes.rs:432-440, 476-492 cache read/write | [M] | OK (read 432-438, write 480-487) | Read needs prev_fingerprint and `!zdr.is_active()`. |
| routes.rs:596, 687 crawl/batch refused under ZDR | [M] | OK | `refuse_durable_job_under_zdr` at 563. |
| routes.rs:626 crawl handed to orchestrator | [M] | DRIFTED: 626 comment, 627 call | |
| `QR/quarry-edge/src/auth.rs:504-545` scopes | [C] | OK | `require_service_route_scope` 504; `/v1/artifacts/` arm 527-531. |
| `Q/quarry-core/src/output.rs:129-150`, `:131-147` | [C] | OK | OutputFormats 129-144 (html 131, markdown 133, screenshot 139, pdf 141); FormatRef 146-150. |
| output.rs:94 "excerpt is the only inline text" | [C] | DRIFTED/UNVERIFIED | Line 94 is `PageExtraction.excerpt` (struct 86-94), not an OutputFormats field. |
| `Q/quarry-runtime/src/pipeline.rs:597-690` ZDR skips artifacts | [C] | OK | zdr::guard 601; None arm 681-683. |
| pipeline.rs:696-706 formats | [C] | OK | Literal 697-705; raw/screenshot/pdf/extract None (700, 702-704). |
| pipeline.rs:476-506 charset then readability | [C] | DRIFTED: 444-476 and 506 | content-type 444-448; PageFetched 452-463; charset 473-476; readability 506. |
| pipeline.rs:505-516 md/links/meta | [C] | DRIFTED: 506-508 | Language detect 514-518. |
| pipeline.rs:421-440 16 MB cap | [C] | OK (421-443) | Applies before any sniff. |
| `Q/quarry-transform/src/readability.rs:235-240` whole-HTML fallback | [C] | OK | `None` arm 238 is the only fallback. |
| `Q/quarry-transform/src/images.rs:27-121` | [C] | OK | is_placeholder 27, resolve_lazy_images 59, resolve_image_links 89, strip_base64_images 121. No parser. |
| `MP/model-gateway/src/quarry.rs:145-152` ScrapeOptions | [M] | OK (145-150) | Fields render, signals, prefer_http3, zdr. Uncommitted hunks are in `impl Client` search code. |
| quarry.rs:755-775 "Quarry does not inline page text" | [M] | DRIFTED: 761-778 | Comment 761-769; ZDR-gated second call 770-774; referenced_markdown 780. |
| `GW/domains/search.rs:112-140` formats/maxPages | [C] | OK | maxPages 118/134; formats ["markdown"] 119/135. |
| `GW/domains/knowledge/quarry.rs:28-60` | [C] | OK | scrape 28 normalises body.url, then forwards the rest of the body. |
| `QR/docs/OSS_PARITY_BACKLOG.md:18-34`, `:22` | [C] | Citation OK; CLAIM FALSE | Row 22 claims "scrape -> markdown/json/screenshot/links/html/summary | full". No formats field exists. |
| `Q/quarry-core/src/cache.rs:5-14` CachePolicy.max_age_s | [C] | OK | CachePolicy 6-14 (max_age_s 9). |
| `QR/docs/openapi.yaml` /v1/scrape 46, ScrapeRequest 4331 | [M] | OK | Properties url, prev_fingerprint, cache, zdr, signals, ingest, org_id. No formats, no timeout. |
| `QR/docs/CONTRACTS.md:568` additive v1 policy | [C] | OK | |

`struct ScrapeOptions` does not exist in any Q crate (grep). The card's "define ScrapeOptions once in quarry-core" is a [NEW] type. No `clean_html`, `onlyMainContent`, selector, extractor, timeout or scrape-id symbol exists in Q (grep returns only unrelated comments).

### 2. AFFECTED FILES
- `Q/quarry-edge/src/routes.rs` [M], 1564 lines. ScrapeRequest 346-375; scrape 398-530; cache read 432-438, write 480-487; internal_run_page 898 (cache 1029-1046); scrape_stream 1067 (cache 1086, 1141). Callers of ScrapeRequest: routes.rs:401, 1070; tests 1481-1482. Route table 131-134: add `GET /v1/scrape/:id` [NEW handler].
- `Q/quarry-core/src/output.rs` [C], 243 lines. OutputFormats 129-144; NormalizedOutput 10-~27. Consumers: pipeline.rs:697-705 (constructor); `MP/model-gateway/src/quarry.rs` [M] artifact_ref 1232, referenced_markdown 780-809.
- `Q/quarry-core/src/cache.rs` [C] (size not measured). CachePolicy 6-14, CacheMode 18-22, Default 26-31.
- `Q/quarry-runtime/src/pipeline.rs` [C], 1503 lines. Insertion point for a new clean/extract stage between 506 and 524. Artifact writes 601-683; NormalizedOutput literal 687-705.
- `Q/quarry-runtime/src/driver_plan.rs` [C], 95 lines. DriverSignals 13-21; consumer routes.rs:440.
- `Q/quarry-transform/src/lib.rs` [C] (module list 5-27; add `clean_html` module [NEW]); `links.rs` [C] (extract 8, caller pipeline.rs:507); `metadata.rs` [C] (extract 12, caller pipeline.rs:508); `markdown.rs` [C] (html_to_markdown 3); `readability.rs` [C] (235-240; caller pipeline.rs:506); `structured.rs` [C] (extract 343, caller extract_routes.rs:161); `images.rs` [C] 196 lines (no callers); `attributes.rs` [C] 459 lines.
- `Q/quarry-edge/src/cache.rs` [C], 236 lines. PageCache key 31-32 (`quarry:page:{fingerprint}`); put_with_ttl 59.
- `Q/quarry-edge/src/auth.rs` [C]. Scope map 514-532; add a `/v1/scrape/` arm if GET is added.
- `MP/model-gateway/src/quarry.rs` [M], 2515 lines. ScrapeOptions 145-150; scrape_with_options 661 (zdr 703); referenced_markdown 780-809; artifact_text 817. Inline return removes the 761-778 second call.
- `MP/execution-core/src/web_tools.rs` [C] (WebToolsClient; web_fetch consumer; health_attest.rs:55 imports it; not opened).
- `GW/domains/search.rs` [C], 1078 lines (118-119, 134-135). `GW/domains/knowledge/quarry.rs` [C], 1158 lines (scrape 28). `GW/domains/ingestions/runs.rs` [C] (create_scrape 336; not read).
- `QR/docs/openapi.yaml` [M] (/v1/scrape 46-95; ScrapeRequest 4331; add GET /v1/scrape/{id}).
- Migrations (only if scrape-id access rows are persisted): `QR/crates/quarry-runtime/migrations/` next free `0010_*.sql` (last committed 0009_browser_timeline_events.sql [C]); control-side alternative `QR/services/quarry-control/internal/store/pg/migrations/` next `015_*.sql` (last 014_fleets.sql [C]). Inline-only needs none.
- SDK regeneration (`sdks/python`, `sdks/typescript`, per card): not located [UNVERIFIED].

### 3. REUSABLE BUT UNWIRED
- `Q/quarry-transform/src/images.rs` [C] (resolve_lazy_images 59, resolve_image_links 89, strip_base64_images 121). Proof: `grep -rn "images::" Q --include=*.rs | grep -v quarry-transform/src/images.rs` returns nothing.
- `Q/quarry-transform/src/attributes.rs` [C] (strip_scripts 23, extract_hreflang_variants 130, extract_dublin_core 178, normalize_tables 262, resolve_anchor_hrefs 329). Only non-module hit is a doc comment at `Q/quarry-runtime/src/crawl_frontier.rs:331`. Proof: `grep -rn "attributes::" Q | grep -v attributes.rs`.
- `Q/quarry-edge/src/cache.rs` [C] `fingerprint()` 84-98 (`#[allow(dead_code)]`): only the definition.
- `Q/quarry-core/src/cache.rs` [C] `stale_while_revalidate_s` (13): no consumer; presets.rs:112 sets it.
- WIRED, not unwired: zdr::guard (zdr.rs:56; pipeline.rs:601); structured.rs (extract_routes.rs:161); links/metadata/readability (pipeline.rs:506-508).

### 4. TESTS
- Workspace: `cd "apps/Ingestion Plane/Quarry-v2" && cargo test --workspace` (`.github/workflows/quarry-v2-ci.yml:205`, job at 146; llvm-cov 80% floor at 304).
- `cargo test -p quarry-edge routes::` (routes.rs tests: call_scrape 1469, request_for 1481, rate_limited_scrape_yields_api_error_envelope 1497, bad_url_scrape_keeps_envelope_error_shape 1536).
- `cargo test -p quarry-edge --test cache_policy` (cache_read_write_serves_from_cache_on_second_call 78; needs Redis, try_redis 24).
- `cargo test -p quarry-runtime zdr` (pipeline.rs: zdr_event_tests 1133, zdr_run_keeps_page_extracted_off_the_durable_channel 1247, zdr_scrape_indexes_nothing 1476).
- Model Plane: `cd "apps/Model Plane/rust" && cargo test --workspace --no-fail-fast` (`.github/workflows/model-plane-rust-ci.yml:119` [M]). quarry.rs tests: scrape_retries_exactly_once_after_unauthorized 1327, scrape_forwards_zero_data_retention 1349.
- Gateway: `cargo test` (`.github/workflows/verevonv3-ci.yml:141` [M]; working directory [UNVERIFIED]). knowledge/quarry.rs test helper state_with_org_zdr 789.
- No formats golden tests exist. The "10 HTML fixtures" in `QR/docs/gap-quarry.md` [C] were not located [UNVERIFIED].

### 5. FEATURE FLAGS / ENV
- No switch gates formats. Request-level: `prefer_http3` (routes.rs:373-374) with `QUARRY_EDGE__HTTP3_ENABLED` (comment 366-368).
- `fetch_timeout_s` (`Q/quarry-edge/src/config.rs` [M] 17; default_timeout 304; assigned 352; applied process-wide at `main.rs:267`). Not per request.
- `cache_ttl_secs` (config.rs:33; default 319-320 = DEFAULT_TTL_SECS 3600 in cache.rs:15). Env name [UNVERIFIED].
- ZDR: body `zdr` is floored by the token claim (routes.rs:416 `claims.effective_zdr_mode(req.zdr)`).

### 6. SURPRISES
- No route-coverage CI exists. `quarry-docs-contract.yml:53` [U, untracked] runs `docs-contract.test.mjs`, which checks named operations (for example the /v1/search 400 docs) and never compares the router's `.route(` list with `openapi.yaml`. The card's "CI route-coverage check from F-t2-064" is not in the tree.
- The docs-contract CI workflow is itself untracked, so the gate exists only in the working tree.
- The audit's "attributes.rs (selector+attribute pairs)" is false. The module has no selector+attribute extractor.
- Gateway already sends `formats: ["markdown"]` and `maxPages: 1` (search.rs:118-119, 134-135). Quarry ignores them silently. A strict parser must ship with a gateway change in the same change.
- The second artifact GET is a latency and ZDR problem, not a permission one: auth.rs:514-532 already grants scrape:read for /v1/artifacts/, and resource_routes.rs:157 binds the read to the claim org.
- `QR/quarry-edge` has no `timeout` on ScrapeRequest; the audit's "timeout" is process-wide config.
- `MP/go/pkg/quarry/client.go` [C] header says model-gateway uses it for Fetch and ExtractStructured. No importer exists (only client_test.go). model-gateway's Rust quarry.rs is the real client. Stale doc.
- Uncommitted touch on this path is minimal: routes.rs +1 line (test fixture at 1329). The scrape handler is committed-stable.

---

## F-t2-009 Route PDFs and Office files to the right parser in /v1/scrape (Content-Type plus magic bytes)

### 1. CITATION CHECK
All card and evidence citations checked (16).

| Cited | Tag | Verdict | Live finding |
|---|---|---|---|
| `Q/quarry-runtime/src/pipeline.rs:440-480` content-type read | [C] | DRIFTED: 444-476 | content-type 444-448; PageFetched 452-463; charset 473-476. |
| pipeline.rs:441-506 readability over every body | [C] | DRIFTED: 444-508 | Readability 506. |
| pipeline.rs:421-440 16 MB body cap | [C] | OK (421-443) | Applies before sniffing. |
| `Q/quarry-runtime/src/answer.rs:839` sole pdf::extract_text caller | [C] | OK | |
| answer.rs:679-753 is_pdf | [C] | DRIFTED: 742-753 | `fn is_pdf` 745-753 (doc 742-744). Private fn. |
| answer.rs:800-850 PDF vs readability branch | [C] | OK | PDF 835-839; readability 848-858. |
| answer.rs:1251-1265 is_pdf tests | [C] | OK | Tests 1251, 1269, 1291. |
| `Q/quarry-transform/src/pdf.rs:40` MAX_PDF_BYTES | [C] | OK | 25 MiB. |
| pdf.rs:69-85 extract_text; :202-213 size and load | [C] | OK | Size check 203; `Document::load_mem` 209. |
| `Q/quarry-transform/src/docx.rs:1-40` extract_text | [C] | OK | MAX_DOCX_BYTES 15; extract_text 18. |
| docx.rs:15-45 compressed cap, read_to_string | [C] | OK | `read_to_string` 39, unbounded per audit. |
| `Q/quarry-transform/src/images.rs:1-40` | [C] | OK | Lazy-load, base64 strip, srcset only. |
| `Q/quarry-transform/src/lib.rs:12` | [C] | OK | `pub mod docx;` 12. |
| `QR/docs/OSS_PARITY_BACKLOG.md:29` | [C] | OK | Media row: pdf.rs text partial; DOCX P2. |
| `QR/docs/GAP.md:85` | [C] | CLAIM FALSE | "PDF, text, and binary source routing is implemented in the pipeline." No routing exists. |
| `QR/docs/PARITY_IMPLEMENTATION_PLAN.md:129` | [C] | CLAIM FALSE | "3C DOCX parsing ... SHIPPED". No non-test caller. |

### 2. AFFECTED FILES
- `Q/quarry-runtime/src/pipeline.rs` [C], 1503 lines. Sniff insertion point: after the body cap (421-443) and before charset decode (473-476). PageFetched content_type 452-463.
- `Q/quarry-runtime/src/answer.rs` [C], 2007 lines. `is_pdf` 745-753 must become `pub(crate)` or move. PDF branch 835-839.
- `Q/quarry-transform/src/pdf.rs` [C], 279 lines. extract_text 69; extract_headings 89 (tests only); size check 203; load 209.
- `Q/quarry-transform/src/docx.rs` [C], 156 lines. extract_text 18; read_to_string 39.
- `Q/quarry-transform/src/lib.rs` [C] (docx 12, pdf 20). A `sniff` module would be [NEW].
- `Q/quarry-transform/Cargo.toml` [C] (lopdf 28; zip deflate-only 29-31).
- `Q/quarry-core/src/output.rs` [C], 243 lines. PageMetadata 184+ (content_type); record detected kind as a [NEW] field.
- Typed `unsupported_media` skip reason: [NEW] (no existing enum found).
- No Model Plane, migration or proto change.

### 3. REUSABLE BUT UNWIRED
- `docx::extract_text` (`Q/quarry-transform/src/docx.rs` [C] 18): no production caller. Proof: `grep -rn "docx::" Q --include=*.rs` returns module internals and tests only.
- `pdf::extract_headings` (`pdf.rs` [C] 89): tests only (274-276).
- Image leg: no parser. `images.rs` [C] is HTML helpers only. Cargo.toml [C] lines 28-31 show lopdf and zip only [UNVERIFIED for the full file].
- WIRED, not unwired: is_pdf (answer.rs:835); zdr::guard; charset::decode.

### 4. TESTS
- `cargo test -p quarry-runtime pdf_` (answer.rs: pdf_branch_selected_by_content_type_or_magic_bytes 1251, pdf_content_type_routes_to_pdf_extraction_not_readability 1269, pdf_magic_bytes_route_to_pdf_extraction_despite_html_content_type 1291).
- `cargo test -p quarry-transform pdf` (pdf.rs: oversized_pdf_is_rejected_before_parsing 260, malformed_pdf_returns_driver_failed 267, extract_headings_on_malformed_returns_error 274).
- `cargo test -p quarry-transform docx` (docx.rs: strips_runs_and_paragraphs 109, unescapes_entities 121, br_becomes_newline 127, extract_text_roundtrip_from_zip 133, non_docx_bytes_error 152).
- No pipeline-level test for non-HTML bodies. CI: `.github/workflows/quarry-v2-ci.yml:205`.

### 5. FEATURE FLAGS / ENV
- None. Hard caps: MAX_HTML_BYTES 16 MiB (pipeline.rs:421); MAX_PDF_BYTES 25 MiB (pdf.rs:40); MAX_DOCX_BYTES 25 MiB (docx.rs:15).
- `DriverSignals.url_type` (driver_plan.rs:20, default 31): UrlType Json/PlainText has no setter or reader outside the default, so it cannot carry a content kind.

### 6. SURPRISES
- The scrape path effectively caps PDFs at 16 MiB (pipeline.rs:421-443 rejects larger bodies before any sniff). The 25 MiB parser cap is unreachable from /v1/scrape.
- /v1/answer and /v1/scrape treat the same PDF URL differently (answer.rs:835 vs pipeline.rs:506).
- `docx.rs` reads only `word/document.xml` (grep); no `[Content_Types].xml` check found [UNVERIFIED beyond line 39].
- The audit's "reuse" of is_pdf requires a visibility change (private fn at answer.rs:745).
- Two stale docs (GAP.md:85, PARITY_IMPLEMENTATION_PLAN.md:129) are false against the code.

---

## F-t2-019 Structured extraction that works in production: json, summary, question and highlights formats over a real Model Plane structured-output grant

### 1. CITATION CHECK
22 load-bearing citations checked. Skipped: Firecrawl reference lines (external repo).

| Cited | Tag | Verdict | Live finding |
|---|---|---|---|
| `Q/quarry-runtime/src/ai_formats.rs:60-300` AiFormatRunner | [C] | OK | Struct 28; summary 75-125; json 127-188; query 190-234. |
| ai_formats.rs:129-283 | [C] | OK (widen to 75-281) | |
| ai_formats.rs:1-40 module doc | [C] | OK | |
| ai_formats.rs:302-310 zdr discarded | [C] | OK | validate_input 302-309; `let _ = zdr;` 308. |
| `Q/quarry-edge/src/extract_routes.rs:195-300` json path | [M] | DRIFTED (live) | extract 184; json_for_org 286 (committed 284). |
| extract_routes.rs:207-232 runner setup | [M] | OK in live; gate UNCOMMITTED | Gate 205-208 and 501 branch ~226-232 exist only uncommitted. Committed 205-206 has no gate. |
| extract_routes.rs:37-58 prompt dead_code, MAX_MAX_URLS 25, 20k cap | [M] | DRIFTED | ExtractRequest 39-54; prompt 48-49; MAX_MAX_URLS 32; MARKDOWN_CAP 33 (20_000). |
| `Q/quarry-runtime/src/mp_client.rs:31-40` ModelPlaneInvokeRequest | [C] | OK (32-39) | Fields content, model, session_key, thread_id. No zdr, no structured_output_schema. |
| mp_client.rs:150-200 invoke | [C] | DRIFTED: invoke_with_org 116-166 | `/v1/invoke` URL 166. |
| `Q/quarry-runtime/src/structured_extract.rs:1-60` | [C] | OK | Client 25-66; route `/v1/structured/extract` 85. |
| `Q/quarry-core/src/contracts.rs:941-957` | [C] | OK (941-953) | StructuredExtractRequest; `zdr: ZdrMode` required. |
| `QR/crates/quarry-runtime/contracts/cross-plane-token-requests.json:36-65, :41-65, :1-70` | [M] | OK | model-invoke 40-45 (granted false at 45); model-stream 57-62 (62); model-audio 68-73 (73). Uncommitted diff touches only rerank rows. |
| `Q/quarry-edge/src/model_plane_gate.rs:1-100` | [U] | OK content; file UNTRACKED | 401 lines; FEATURES 53; manifest_granted 205-209. |
| `QR/docs/SELF_HOST.md:100-125`, `:112-122` | [M] | MISSING in committed | QB-T17 section 112-127 exists only uncommitted. Committed row 101 says PAGE_TITLE_ENRICH default `true`; live row (~107) says `false`. |
| `GW/domains/ingestions/runs.rs:190-300` create_extract | [C] | OK (268-300) | Forwards urls and prompt (283-285); no schema. |
| `MP/model-gateway/src/grpc.rs:2464-2660` Fetch/ExtractStructured | [M] | OK | fetch 2474-2515 (ZDR 2480); extract_structured 2516 (ZDR 2522-2528, uncommitted); MAX_EXTRACT_CONTENT_CHARS 320. |
| `MP/model-gateway/src/http_routes.rs:6670-6700` | [M] | OK | InvokeRequest.structured_output_schema 6693. |
| http_routes.rs:8960-9000 | [M] | OK | invoke 8908-9003; ZDR edits in hunks 8924-9003; schema 8987-8988 inside the span [UNVERIFIED touched]. |
| `MP/inference-core/src/provider/openai.rs:455-490` | [C] | OK (463-488) | response_format json_schema; strict left unset. |
| `MP/inference-core/src/provider/anthropic.rs:725-740` | [C] | OK (727-737) | Schema NOT forwarded; prompt-only. |
| `MP/go/pkg/quarry/client.go:1-20` | [C] | CLAIM FALSE | Says model-gateway uses it; no importer. |
| `QR/scripts/tests/docs-contract.test.mjs:213-227, 258-267` | [C] | OK | Asserts switches default 0 and committed registry has no model-gateway audience. |

### 2. AFFECTED FILES
- `Q/quarry-runtime/src/ai_formats.rs` [C], 472 lines. summary 75-125; json 127-188; query 190-234; validate_input 302-309. Callers: extract_routes.rs:217 (runner), `Q/quarry-edge/src/main.rs` [M] 1205, `Q/quarry-edge/tests/zdr_claim_floor.rs` [M] 315, `Q/quarry-runtime/src/answer.rs` [C] 933, 1037, 1074, 1543.
- `Q/quarry-runtime/src/mp_client.rs` [C], 715 lines. Add `structured_output_schema` and `zdr` to ModelPlaneInvokeRequest (31-40). invoke 101, invoke_with_org 116-166.
- `Q/quarry-runtime/src/structured_extract.rs` [C], 277 lines (no production caller).
- `Q/quarry-core/src/contracts.rs` [C] StructuredExtractRequest 941-953.
- `Q/quarry-edge/src/extract_routes.rs` [M], 639 lines. ExtractRequest 39-54; gate 205-208; json_for_org 286.
- `Q/quarry-edge/src/answer_routes.rs` [M]. query_for_org 256.
- `Q/quarry-edge/src/model_plane_gate.rs` [U], 401 lines. FEATURES 53; decide 140; manifest_granted 205-209.
- `Q/quarry-edge/src/config.rs` [M], 501 lines. model_plane_url 103; model_plane_token 106; ai_formats 161-163 (default false 391); test env 480.
- `Q/quarry-edge/src/main.rs` [M], 1569 lines. Gate decide 246-251; AiFormatRunner::new 1205; CROSS_PLANE_AUTH_DEV_BYPASS refusal 176.
- `Q/quarry-runtime/src/token_request_contract.rs` [M]. Model-row tests 209, 267, 778.
- `QR/crates/quarry-runtime/contracts/cross-plane-token-requests.json` [M] (rows 40-73) and `.json.sha256` [M] (pin 40bd3586...). Both must change together.
- `Q/quarry-runtime/src/service_tokens.rs` [C]: `EXPECTED_REQUESTS` 718 (6 rows) must match the manifest.
- `scripts/tests/zdr-contract-sync.test.sh` [M] (repo root; GOLDENS comments 32-62) must register the new digest.
- `apps/Control Plane/auth-core/src/auth/fixtures/cross-plane-token-requests.json` [M] (rows 40-73) must be re-vendored.
- `apps/Control Plane/config/plane-service-principals.json` [C]: quarry-edge principal 380 (no model-gateway audience); model-gateway principal 289, consumerEnv 295.
- `MP/model-gateway/src/http_routes.rs` [M], 11681 lines: `/v1/invoke` 136; InvokeRequest 6693; invoke 8908-9003.
- `MP/model-gateway/src/grpc.rs` [M], 4611 lines: fetch 2474; extract_structured 2516-~2655; MAX_EXTRACT_CONTENT_CHARS 320.
- `MP/model-gateway/src/zdr_posture.rs` [U], 1259 lines: decide_for, Route, decide_content_route.
- `MP/inference-core/src/provider/openai.rs` [C] (463-488); `anthropic.rs` [C] (727-737).
- `MP/proto/model_plane/v1/gateway.proto` [C] (Fetch 27; ExtractStructured 34; ExtractStructuredRequest 438-453; schema_json 453); `inference.proto` [C] (structured_output_schema 123).
- `GW/domains/ingestions/runs.rs` [C] create_extract 268-300 (add schema forwarding).
- Docs: `QR/docs/SELF_HOST.md` [M], `QR/docs/CROSS_PLANE_INTEGRATION.md` [M], `QR/docs/CROSS_PLANE_AUTH.md` [M].
- Migrations: none.

### 3. REUSABLE BUT UNWIRED
- `StructuredExtractClient` (`structured_extract.rs` [C] 25-66): no production caller. Proof: `grep -rn "structured_extract::" Q --include=*.rs` returns only the lib.rs:178 re-export and a comment in ai_formats.rs:128. Its route `/v1/structured/extract` is not served by any MP code (`grep -rn "v1/structured" "apps/Model Plane"` is empty).
- `AiFormatRunner::summary` / `summary_for_org` (`ai_formats.rs` [C] 75-91): no caller. Proof: `grep -rn "summary_for_org\|\.summary(" Q` returns none.
- `quarry_core::json_schema::validate` (`Q/quarry-core/src/json_schema.rs` [C]): WIRED via ai_formats.
- Structured harvest (`structured.rs` [C] 343): WIRED (extract_routes.rs:161); the model-free first pass.
- Model Plane `ExtractStructured` RPC (grpc.rs 2516): served by model-gateway, but no Verevon or Quarry caller found (grep shows only generated code, docs, and a tool-list comment at `MP/model-gateway/src/coordinator.rs:9`). Whether agent tool dispatch calls it is [UNVERIFIED].
- `structured_output_schema` to OpenAI (openai.rs 463-488): WIRED; Anthropic drops it (anthropic.rs 727-737).

### 4. TESTS
- `cargo test -p quarry-runtime ai_formats` (summary_returns_expected_result 344, summary_caps_output_length 363, json_extracts_and_validates 387, json_invalid_when_required_missing 415, json_returns_error_when_model_returns_non_json 432, query_returns_grounded_answer 449).
- `cargo test -p quarry-runtime structured_extract` (tests 164-244, including extract_rejects_zdr_without_inline_markdown 216).
- `cargo test -p quarry-runtime token_request_contract` ([M]; a_model_invoke_asks_auth_core_for_exactly_the_invoke_row 778; rerank_never_uses_the_model_plane 337).
- `cargo test -p quarry-edge structured_extraction_makes_no_model_plane_call_while_its_switch_is_off` (extract_routes.rs 515; UNCOMMITTED).
- `cargo test -p quarry-edge --test zdr_claim_floor` ([M]; AiFormatRunner 315; model_plane features 920, 980).
- Model Plane: `cd "apps/Model Plane/rust" && cargo test --workspace --no-fail-fast` (`.github/workflows/model-plane-rust-ci.yml:119` [M]).
- Docs and manifest contract: `node --test "apps/Ingestion Plane/Quarry-v2/scripts/tests/docs-contract.test.mjs"` (`.github/workflows/quarry-docs-contract.yml:53`, which is [U]).
- Quarry CI: `.github/workflows/quarry-v2-ci.yml:205`.

### 5. FEATURE FLAGS / ENV
- `QUARRY_EDGE__AI_FORMATS`: config.rs:161-163 (live only). Committed afb83928 has 0 hits. Default false at 391.
- `QUARRY_EDGE__MODEL_PLANE_URL`: config.rs:103; main.rs gate 246-251. Committed extract (205-206) enables the runner when this alone is set.
- `QUARRY_EDGE__MODEL_PLANE_TOKEN`: config.rs:106.
- `QUARRY_EDGE__ANSWER_SYNTHESIS` (config.rs:156) gates the answer route.
- `QUARRY_EDGE__PAGE_TITLE_ENRICH`: committed SELF_HOST default `true`; live `false`.
- `QUARRY_EDGE__CROSS_PLANE_AUTH_DEV_BYPASS` (main.rs:176 refuses in production). Per the card, extraction works today only under this bypass.
- `MODEL_GATEWAY_SERVICE_API_KEY` (plane-service-principals.json:295).
- Model Plane ZDR comes from token identity (`identity.zdr_for`, grpc.rs 2480, 2522). Uncommitted zdr_posture.rs [U].

### 6. SURPRISES
- The audit's "switch stays off by default" describes UNCOMMITTED code. Committed extract_routes.rs:205-206 has no gate: setting MODEL_PLANE_URL alone enables the runner, which Auth Core then refuses with 403 under the manifest.
- `model_plane_gate.rs` [U], zdr_posture.rs [U] and the `ai_formats` config are not in the committed tree. A plan that cites them must commit them first.
- Gateway `create_extract` forwards prompt only (runs.rs 283-285); the schema is dropped.
- There is no json repair pass, no normalisation and no provider fallback in Quarry: ai_formats uses only `json_schema::validate` after the call (validate, not repair).
- Flipping the grant requires coordinated edits to the manifest, its sha256 pin, zdr-contract-sync goldens, vendored copies, `EXPECTED_REQUESTS` and two docs-contract assertions (213-227 and 258-267). The docs-contract test currently asserts every model row is `granted: false`.
- The Go package header (`go/pkg/quarry/client.go`) is stale (see table).

---

## F-t2-023 Scrape-time page actions plus screenshot and PDF formats, under one bounded, grant-gated action policy

### 1. CITATION CHECK
20 citations checked. Skipped: Firecrawl reference lines (external repo).

| Cited | Tag | Verdict | Live finding |
|---|---|---|---|
| `Q/quarry-edge/src/routes.rs:346-373` render hints only | [M] | OK (346-375) | render 364-365; prefer_http3 373-374. |
| `Q/quarry-runtime/src/pipeline.rs:702-703` screenshot/pdf None | [C] | OK | Lines 702 and 703. |
| `Q/quarry-runtime/src/driver_plan.rs:13-20` signals hints | [C] | OK (13-21) | actions Vec<String> 15; screenshot 16; pdf 17. |
| `Q/quarry-browser/src/chromiumoxide.rs:2392-2445` | [C] | OK | screenshot 2392-2398 (capture_beyond_viewport 2396); pdf 2438-2441 (PrintToPdfParams 2440). |
| `Q/quarry-runtime/src/action_runtime.rs:62-110` ActionRuntime | [C] | OK | Struct 62; new/default 80-86. |
| action_runtime.rs:380-392 Evaluate direct | [C] | OK (380-394) | `driver.evaluate` 384; no policy. |
| action_runtime.rs:164-200 run | [C] | OK | run 164. |
| `Q/quarry-browser/src/actions.rs:1-80` | [C] | OK | Action 5-54 (Screenshot 35, Pdf 38, Evaluate 39, Navigate 42); ActionScript 64-69; OnError 71-78. |
| `Q/quarry-runtime/src/browser_driver.rs:128-240` do_fetch | [C] | OK (131-~250) | acquire 173; egress 192; goto 203; wait_for 219; content 226. |
| `Q/quarry-runtime/src/driver.rs:31-79` hints | [C] | OK (approx.) | FetchHints ~28-40; RenderHints ~44-56. |
| `Q/quarry-runtime/src/observation.rs:525-545` put_artifact_if_allowed | [C] | MISLABELED | Helper defined at 1428-1446. 516-541 are the Screenshot and Pdf call sites. |
| `Q/quarry-edge/src/resource_routes.rs:157` tenant-bound read | [C] | OK | `get_with_key(&claims.org_id, …)`. |
| `Q/quarry-edge/src/resource_routes.rs:190-215` | [C] | DRIFTED: 196-215 is `get_page_image` | Not the artifact read. |
| `Q/quarry-runtime/src/vision.rs:124-144` | [C] | OK | Preprocess tiles (not capture). |
| `QR/docs/OSS_PARITY_BACKLOG.md:21-23` | [C] | CLAIM FALSE | Row 22 claims screenshot parity; row 23 claims actions "before extract". Neither is wired on scrape. |
| `Q/quarry-edge/src/agent_routes.rs:140-290` approvals | [C] | OK | sensitive_approval_for_action 144; RespondDialog 149; `_ => Ok(None)` 280. |
| `MP/execution-core/src/quarry_agent.rs:161` Evaluate | [C] | OK | `Evaluate { script }` 161. |
| `GW/domains/browser.rs:3751` evaluate denied | [C] | OK | Denied at 3751. |

### 2. AFFECTED FILES
- `Q/quarry-edge/src/routes.rs` [M], 1564 lines. ScrapeRequest 346-375 (add `actions`, `screenshot`, `pdf` [NEW]); scrape 398-530.
- `Q/quarry-runtime/src/driver_plan.rs` [C], 95 lines. DriverSignals 13-21; `plan_from_signals` consumes actions/screenshot/pdf.
- `Q/quarry-runtime/src/driver.rs` [C]. FetchHints ~28-40; RenderHints ~44-56 (wait_for only); Driver trait 82-95.
- `Q/quarry-runtime/src/browser_driver.rs` [C], 581 lines. BrowserDriverAdapter 75; do_fetch 131-~250. Primary hook for capture and actions.
- `Q/quarry-runtime/src/action_runtime.rs` [C], 541 lines. ActionRuntime 62; run 164; Screenshot 228; Pdf 255; Evaluate 380-394. Must accept an existing session [NEW].
- `Q/quarry-browser/src/actions.rs` [C]. Action enum 5-54; ActionScript 64-69; OnError 71-78.
- `Q/quarry-browser/src/lib.rs` [C] (trait BrowserDriver 408; configure_egress_policy 445; screenshot 489; pdf 501).
- `Q/quarry-browser/src/chromiumoxide.rs` [C], 3551 lines (screenshot 2392; pdf 2438).
- Other driver impls [C]: `browserless.rs` (screenshot 289, pdf 302), `browserbase.rs` (305, 317), `kernel.rs` (611, 622), `pool.rs` (177, 184).
- `Q/quarry-runtime/src/pipeline.rs` [C] 1503 lines: formats 697-705; page_renderer clone 1000; render_and_emit 1020-1027.
- `Q/quarry-runtime/src/page_renderer.rs` [C], 240 lines (zdr guard 29-30; render_and_emit 116; screenshot 166).
- `Q/quarry-runtime/src/observation.rs` [C], 3214 lines (agent arms 516-541; put_artifact_if_allowed 1428-1446).
- `Q/quarry-core/src/output.rs` [C] (screenshot/pdf fields 139-141).
- `Q/quarry-edge/src/resource_routes.rs` [C], 1252 lines (artifact read 157; get_page_image 196-215).
- `Q/quarry-edge/src/agent_routes.rs` [C] (approvals 144-290; Evaluate mapping 694, 751).
- `Q/quarry-edge/src/main.rs` [M] (page renderer wiring 1351-1387) and `Q/quarry-edge/src/config.rs` [M] (vision and browser fields 132, 273-291).
- `Q/quarry-edge/Cargo.toml` [C] (default features 73-74).
- `MP/execution-core/src/quarry_agent.rs` [C] (Evaluate 161; the Model's action contract).
- Migrations: none for capture. Artifact content types via `content_type_for_key` (not located).

### 3. REUSABLE BUT UNWIRED
- `ActionRuntime` (action_runtime.rs 62-166): no production caller. Proof: `grep -rn "ActionRuntime" Q --include=*.rs` returns its definition, the lib.rs:99 re-export, and tests 445-520 only.
- `ActionScript`/`OnError` (actions.rs 64-78): used only by ActionRuntime.
- `OutputFormats.screenshot` / `.pdf` (output.rs 139-141): never assigned (pipeline.rs 702-703 None).
- WIRED on other paths: BrowserDriver screenshot/pdf (observation.rs 517, 531; page_renderer.rs 166); put_artifact_if_allowed (observation.rs 520, 533); get_page_image (resource_routes.rs 196-215, route registration [UNVERIFIED]).

### 4. TESTS
- `cargo test -p quarry-runtime action_runtime` (empty_script_returns_empty_results 445; all_actions_succeed 453; screenshot_produces_output_payload 499; pdf_produces_output_payload 509; results_carry_correct_indices 519). Mock drivers only.
- `cargo test -p quarry-runtime agent_loop` (screenshot_action_refused_when_driver_claims_no_visual_fidelity 811).
- `cargo test -p quarry-runtime browser_driver` (fetch_calls_in_order 459; fetch_conditional_invokes_wait_for_when_selector_set 495; fetch_conditional_uses_default_timeout_when_unset 524).
- `cargo test -p quarry-browser --features chromiumoxide` (`quarry-v2-ci.yml:211`); browserless.rs screenshot_success 428, pdf_success 444.
- No e2e browser test for scrape screenshots found [UNVERIFIED: not searched outside Quarry].

### 5. FEATURE FLAGS / ENV
- Cargo feature `browser-agent`, default ON (`Q/quarry-edge/Cargo.toml` [C] 73-74): enables chromiumoxide and the page renderer.
- PageRenderer is online only when `cas_bucket`, `dataplane_nats_url` and `edge_internal_base_url` are set (main.rs 1352-1358 [M]). `page_image_cleanup_enabled` (config.rs 283) controls whether the visual processor is passed (main.rs 1366-1370).
- Vision switches (config.rs [M] 273-291): vision_enabled 273, vision_backend 275, vision_sidecar_url 277, visual_diff_enabled 279, screenshot_preprocessing_enabled 281, visual_tiles_enabled 285, ocr_preconditioning_enabled 287.
- Browser: require_browser_grants (config.rs 132; default false at 385); browser_grant_validator_url 124; browser_grant_validator_grpc_url 129; BROWSER_PROVIDER [UNVERIFIED].
- ZDR: `zdr::guard(…, WriteKind::Artifact)` (`Q/quarry-core/src/zdr.rs` [C] 56) gates writes at observation.rs 1436 and page_renderer.rs 29.

### 6. SURPRISES
- The card's "screenshot is never captured on scrape" is only partly true. With `ingest` on, a successful DP ingest and the browser-agent feature with CAS and NATS configured, the scrape path calls `PageRenderer::render_and_emit` (pipeline.rs 994-1027; page_renderer.rs 166). It captures a full-page screenshot for visual RAG, not returned as `formats.screenshot`, and skipped under ZDR (page_renderer.rs 29-30).
- Page images already have an endpoint (`get_page_image`, resource_routes.rs 196-215). The audit does not list it as reuse.
- `observation.rs:525-545` is not the helper (see table).
- `ActionRuntime` `Evaluate` runs `driver.evaluate` with no size, time, approval or hash (action_runtime.rs 380-394). This is the F-t2-002 gap, reachable only if ActionRuntime gets a caller.
- `UrlType` on DriverSignals is never set from requests (driver_plan.rs 20, 31).
- Stale tenant comment: browser.rs 1461-1463 says Quarry's artifact GET applies no tenant scoping; resource_routes.rs:157 binds it to the claim org.
- `signals.actions` is `Vec<String>` (driver_plan.rs 15), not a typed action payload. There is no typed action list on ScrapeRequest today.

---

## F-t2-037 Document parsing depth: PDF classification and Model Plane OCR routing, Office and e-book conversion, bounded parsing and a per-org parse cache

### 1. CITATION CHECK
18 citations checked.

| Cited | Tag | Verdict | Live finding |
|---|---|---|---|
| `Q/quarry-transform/src/pdf.rs:1-40` | [C] | OK | MAX_PDF_BYTES 40. |
| `Q/quarry-runtime/src/answer.rs:745` is_pdf | [C] | OK | 745-753. |
| answer.rs:835-839 only pdf::extract_text caller | [C] | OK | 839. |
| `MP/inference-core/src/provider/doc_intel.rs:1-20` | [C] | OK | Default model `prebuilt-layout` 18; v4 GA note 15-16. |
| `Q/quarry-runtime/src/vision.rs:35-59` | [C] | OK (approx.) | VisualPreprocessResult 30-38; VisualProcessorOptions 41+. Sidecar preprocessing, not OCR. |
| `QR/docs/GAP.md:85` | [C] | CLAIM FALSE | Claims PDF routing in the pipeline. |
| `Q/quarry-transform/src/docx.rs:1-30` | [C] | OK | |
| `Q/quarry-transform/src/lib.rs:12` and 12-40 | [C] | OK | docx 12, pdf 20. |
| pipeline.rs:430-500 no content-type router | [C] | OK (444-508) | Charset and readability for every body. |
| `QR/docs/PARITY_IMPLEMENTATION_PLAN.md:129` | [C] | CLAIM FALSE | DOCX "SHIPPED". |
| `QR/docs/OSS_PARITY_BACKLOG.md:29` | [C] | OK | |
| `Q/quarry-transform/src/pdf.rs:40, 69-85, 89` | [C] | OK | |
| `Q/quarry-transform/src/docx.rs:15-45` | [C] | OK | Cap 15; read_to_string 39. |
| `Q/quarry-transform/Cargo.toml:28-31` | [C] | OK | lopdf 28; zip 29-31. |
| `IC/parsers.py:30-60, 104-112, 140-215, 1085-1130` | [C] | OK for 34, 42, 103-112, 138-165, 1090, 1121 | `_parse_pdf` 34; `_parse_docx` 42; zip caps 103-112; deadline 138-145; errors 160-165; `_parse_local` 1090; `parse_uploaded_file` 1121. |
| `IC/main.py:204-212, 271-285` | [C] | OK | /api/v1/import/extract 204-212 has no zdr parameter; zdr refusal 221-224 is on /jobs/upload; durable refusal near 279-282. |
| `IC/chat_extract.py:1-40` | [C] | OK | 60k cap 12; 1 MB input cap 30. |
| `MP/model-gateway/src/http_routes.rs:434, 4566-4650` | [M] | OK (434); handler span approx. | Layout route 434; decide_content_route at 4609. |
| `Q/quarry-runtime/src/page_extract.rs:332-343` model_hop_allowed | [C] | OK (335-343) | Returns false under ZDR (336-338). |
| `Q/quarry-core/src/privacy.rs:65-80` | [C] | OK | guard_third_party_processing 65. |
| `CP/auth-core/src/auth/fixtures/cross-plane-token-requests.json:40-73` | [M] | OK | granted false at 45, 62, 73. |
| `Q/quarry-edge/src/model_plane_gate.rs:1-14` | [U] | OK content; untracked | Model rows off by default. |

### 2. AFFECTED FILES
- `Q/quarry-runtime/src/pipeline.rs` [C] (1503): router between 421-476 and charset 473-476; readability 506; add document branch [NEW].
- `Q/quarry-runtime/src/answer.rs` [C] (2007): is_pdf 745-753; PDF branch 835-839.
- `Q/quarry-runtime/src/page_extract.rs` [C]: model_hop_allowed 335-343 (refuses under ZDR).
- `Q/quarry-runtime/src/vision.rs` [C]: sidecar preprocessing (not OCR).
- `Q/quarry-transform/src/pdf.rs` [C] (279), `docx.rs` [C] (156), `lib.rs` [C], `Cargo.toml` [C].
- Office/EPUB modules [NEW] (XLSX, PPTX, ODF, RTF, CSV, EPUB per audit).
- Model Plane: `MP/model-gateway/src/http_routes.rs` [M] (layout route 434; pages 1942; decide_content_route 4609); `MP/inference-core/src/provider/doc_intel.rs` [C] (18); `MP/model-gateway/src/zdr_posture.rs` [U] (1259 lines).
- Model Plane OCR client in Quarry [NEW] (no Quarry caller of `/v1/ai/documents/layout` found by grep).
- Control Plane fixture [M] (rows 40-73) and `Q/quarry-runtime/src/token_request_contract.rs` [M].
- `IC/parsers.py` [C], `IC/main.py` [C], `IC/chat_extract.py` [C] (reuse candidates).
- `services/quarry-vision-sidecar` [C] (directory exists; not read).
- Migrations: parse cache, if persisted: `QR/crates/quarry-runtime/migrations/0010_*.sql` [NEW].

### 3. REUSABLE BUT UNWIRED
- `docx::extract_text` (docx.rs 18): no caller outside the module. Proof: `grep -rn "docx::" Q --include=*.rs`.
- `pdf::extract_headings` (pdf.rs 89): tests only.
- `imports-core` parsers (`IC/parsers.py` [C] 34-215, 1090-1130): built and in production for that service, but not reachable from Quarry.
- Model Plane `doc_intel` / `/v1/ai/documents/layout` (http_routes.rs 434): served; no Quarry caller (grep `documents/layout` in Q: none).
- WIRED: is_pdf; model_hop_allowed (page_extract.rs 335).

### 4. TESTS
- `cargo test -p quarry-transform pdf` (oversized_pdf_is_rejected_before_parsing 260; malformed_pdf_returns_driver_failed 267; extract_headings_on_malformed_returns_error 274).
- `cargo test -p quarry-transform docx` (extract_text_roundtrip_from_zip 133; non_docx_bytes_error 152).
- `cargo test -p quarry-runtime pdf_` (answer.rs 1251, 1269, 1291).
- imports-core tests: [UNVERIFIED path and command].
- CI: `.github/workflows/quarry-v2-ci.yml:205`.

### 5. FEATURE FLAGS / ENV
- No switch for PDF or Office parsing. Caps are hard-coded: body 16 MiB (pipeline.rs 421), PDF 25 MiB (pdf.rs 40), DOCX 25 MiB (docx.rs 15).
- Vision (config.rs [M] 273-287): vision_enabled, vision_sidecar_url, ocr_preconditioning_enabled. Env names [UNVERIFIED].
- Model Plane OCR: no Quarry-side switch; any future switch needs a manifest row (`model-audio` pattern, config.rs 173).
- imports-core: `office_read_deadline` config key (parsers.py 145) [UNVERIFIED env name].

### 6. SURPRISES
- The card says the item is about routing; the code's bigger gap is that `imports-core` already has budgeted Office parsers that Quarry cannot reach. ADR candidate per audit.
- `imports-core` `/api/v1/import/extract` is ephemeral with no zdr parameter (main.py 204-212), which matches the audit. Its durable jobs endpoint refuses ZDR (221-224).
- `QR/docs/GAP.md:85` and `PARITY_IMPLEMENTATION_PLAN.md:129` are false (see table).
- `page_extract.rs` `model_hop_allowed` refuses every Model hop under ZDR (336-338), so OCR under ZDR needs a new exception design, as the audit says.

---

## F-t2-050 Fail-closed PII redaction stage for scraped and crawled content

### 1. CITATION CHECK
16 citations checked.

| Cited | Tag | Verdict | Live finding |
|---|---|---|---|
| `Q/quarry-runtime/src/pipeline.rs:505-516` | [C] | DRIFTED: 506-518 | md 506; links 507; metadata 508; lang 514-518. No redaction. |
| pipeline.rs:525 fingerprint over raw bytes | [C] | DRIFTED: 524 | `content_fingerprint(&resp.body)` 524; text_fp 525. |
| pipeline.rs:604-660 html/markdown/meta persisted | [C] | OK (604-661) | |
| `Q/quarry-runtime/src/agent_memory.rs:49-67, 96-105` | [C] | OK (52-67; digit rule 102) | Whitespace tokenising; 13-19 digit run at 102. |
| agent_memory.rs:52 | [C] | OK | |
| `Q/quarry-runtime/src/lib.rs:101` re-export | [M] | OK | `pub use agent_memory::{redact_sensitive, AgentScratchpad};` at 101. |
| `MP/model-gateway/src/moderation.rs:1-20, :83-106, :83-135` | [C] | OK | Header 1-20 (owner note at 3); pii_filter check 109; enforcement 83-135. |
| `MP/model-gateway/src/http_routes.rs:442` route | [M] | OK | |
| http_routes.rs:1947-1957 AiLanguageRequest | [M] | OK (1947) | No entity or replace-style params. |
| http_routes.rs:4732-4739 ai_language_pii | [M] | OK (4732) | |
| `MP/inference-core/src/provider/language.rs:1-25, :25, :46, :517-530` | [C] | OK | Pii 25/46; PiiEntityRecognition 517; piiCategories All and redactionCharacter `*` 529. |
| `QR/crates/quarry-runtime/contracts/cross-plane-token-requests.json:40-73` | [M] | OK | granted false at 45, 62, 73. |
| `QR/docs/CROSS_PLANE_INTEGRATION.md:70-80` | [M] | OK (~66-75) | Says quarry-edge holds only the data-plane audience. |
| `QR/docs/SOVEREIGN_SCRAPING_RELIABILITY.md:97, :218` | [C] | OK | `raw_artifact_policy: redact_before_persist` 97; "PII Redactor" stage 218. |
| `Q/quarry-core/src/privacy.rs:65` | [C] | OK | guard_third_party_processing 65. |
| `DP/retrieval-engine-rs/src/redact.rs:1-30` | [M] | OK (regexes 25-40) | EMAIL_RE 25; CARD_RE 30; PHONE_RE 37. Uncommitted +100 lines (redact_query 45, query_for_log 92, correlation_hash 115). |

### 2. AFFECTED FILES
- `Q/quarry-runtime/src/pipeline.rs` [C] (1503): redaction stage between 506-518 and fingerprint 524; artifact writes 601-683; local index 186-194; ingest 855-1030; NormalizedOutput 687-705.
- `Q/quarry-runtime/src/agent_memory.rs` [C] (157): redact_sensitive 52-67; AgentScratchpad::note 28-33 calls it at 32.
- `Q/quarry-runtime/src/lib.rs` [M]: re-export 101.
- `Q/quarry-transform/src/` [NEW] redact module (deterministic: email, phone, IBAN, Luhn, Norwegian fodselsnummer). Register in `lib.rs` [C] (mod list 5-27).
- `Q/quarry-core/src/privacy.rs` [C]: PrivacyPolicy; guard_third_party_processing 65.
- `Q/quarry-core/src/zdr.rs` [C]: WriteKind 36; guard 56.
- `Q/quarry-runtime/src/ingest_client.rs` [C]: zdr::guard(WriteKind::Ingest) 199.
- `Q/quarry-runtime/src/page_renderer.rs` [C] (page images are not text; redaction cannot cover them).
- `Q/quarry-edge/src/routes.rs` [M] and `extract_routes.rs` [M] (json format input, 286).
- `MP/model-gateway/src/http_routes.rs` [M] (PII route 442, 1947, 4732), `moderation.rs` [C].
- `MP/inference-core/src/provider/language.rs` [C] (Pii op; categories hard-coded at 529).
- Policy owner: `MP/go/services/capability-core` [C] (safety_policies; moderation.rs 3-5 says capability-core owns pii_filter, not the Control Plane).
- `DP/retrieval-engine-rs/src/redact.rs` [M] (shared regex set candidate).
- Migrations: none unless a redaction audit table is added [NEW].

### 3. REUSABLE BUT UNWIRED
- `redact_sensitive` (agent_memory.rs 52): reached only through `AgentScratchpad::note` (32). `AgentScratchpad` has no production caller: `grep -rn "AgentScratchpad" Q --include=*.rs` returns the lib.rs:101 re-export only.
- Model Plane `/v1/ai/language/pii` (http_routes.rs 442): served; no Quarry caller (grep `language/pii` in Q: none).
- `moderation.rs` `redact_pii` (deterministic pii_filter): WIRED inside model-gateway for model input only.
- DP `redact.rs` (EMAIL_RE/CARD_RE/PHONE_RE): used for logs (DP scope); no Quarry use.

### 4. TESTS
- `cargo test -p quarry-runtime agent_memory` (redacts_email 121, redacts_long_token 129, redacts_card_number 136).
- `cargo test -p quarry-runtime zdr` (pipeline.rs zdr tests 1133-1137, 1247, 1476).
- Model Plane: `cd "apps/Model Plane/rust" && cargo test --workspace --no-fail-fast` (`model-plane-rust-ci.yml:119` [M]); moderation.rs test names [UNVERIFIED].
- Data Plane tests for redact.rs [UNVERIFIED].
- No Norwegian or non-English accuracy test exists (grep not run) [UNVERIFIED].

### 5. FEATURE FLAGS / ENV
- None exists for Quarry PII redaction.
- Model Plane `pii_filter` is a capability-core `safety_policies` kind (moderation.rs 109). Read from capability-core with a bearer and URL (moderation.rs 137-166); env names [UNVERIFIED].
- The Model Plane PII tier needs the model-gateway grant (manifest rows 40-73, all granted false).

### 6. SURPRISES
- Persisted representations beyond markdown: raw html artifact and meta.json (pipeline.rs 604-661), the local index (186-194), page images (page_renderer.rs), NormalizedOutput metadata (title, description, author; output.rs 184-220), and Data Plane ingest (pipeline.rs ~855-1000). All must be covered, not only markdown.
- The audit's "run before hashing for change tracking" conflicts with the raw-byte fingerprint at pipeline.rs:524. Fingerprints stay raw; the change decision is pending.
- Policy ownership is misassigned in the card: policy sits with capability-core (moderation.rs 3-5), not the Control Plane.
- `redact_sensitive` flattens markdown by whitespace-splitting (agent_memory.rs 53), so reuse is not safe for markdown (the audit agrees).
- DP `redact.rs` is changing in uncommitted work (+100 lines), so a shared redactor must coordinate with the Data Plane session.

---

## F-t2-048 Org-scoped URL cache with explicit freshness, and a cache-only no-egress mode

### 1. CITATION CHECK
20 citations checked.

| Cited | Tag | Verdict | Live finding |
|---|---|---|---|
| `Q/quarry-edge/src/cache.rs:1-7` | [C] | OK | Module doc; key `quarry:page:{fingerprint}` (line 3). |
| `Q/quarry-edge/src/routes.rs:430-436` read skipped under ZDR | [M] | OK (432-438) | Needs prev_fingerprint. |
| routes.rs:480 write skipped under ZDR | [M] | OK (480-487) | |
| `Q/quarry-edge/src/cache.rs:1-30` keyed by fingerprint | [C] | OK | key 31-32; get 38; put_with_ttl 59. |
| cache.rs:84-105 fingerprint(url, vary, js) dead | [C] | OK (84-98) | `#[allow(dead_code)]` 84. |
| cache.rs:108-125 SearchCache::key org-scoped | [C] | DRIFTED: 105-127 | key 121-127; doc 114-120. |
| routes.rs:56-76 should_read/should_write/effective_ttl | [M] | OK | Fns 58, 65, 72. |
| `Q/quarry-core/src/cache.rs:1-40` CachePolicy, CacheMode | [C] | OK | Struct 6-14; CacheMode 18-22; Default 26-31. |
| `Q/quarry-core/src/presets.rs:112` SWR 3600 | [C] | OK | No runtime consumer (grep). |
| `GW/cache.rs:80-125, 191-201` | [C] | OK (approx.) | lookup_within 96; store 108; store_for_secs 115; cache_key 194-201 (DefaultHasher 195). |
| `GW/domains/knowledge/quarry.rs:80-135, 196-210, 255-290` | [C] | OK (approx.) | scrape_preview 62; cache_key 104; lookup 117; store 200; CacheAdmission 213-285; from_policy 256. |
| `GW/domains/search.rs:218, 286` | [C] | OK | cache_key "search-web" 218; store 286. |
| routes.rs:476-492 write block | [M] | OK | 476-487. |

### 2. AFFECTED FILES
- `Q/quarry-edge/src/cache.rs` [C], 236 lines. PageCache 18-62 (key 31-32; put_with_ttl 59); fingerprint 84-98 (dead); SearchCache 105-127 (org key 121-127, the pattern to copy); DEFAULT_TTL_SECS 15.
- `Q/quarry-edge/src/routes.rs` [M], 1564 lines. scrape cache 432-438 and 480-487; internal_run_page 1029-1046 (cache also used here; not in audit); scrape_stream 1085-1141; should_read/should_write/effective_ttl 58-80.
- `Q/quarry-core/src/cache.rs` [C]: CachePolicy 6-14 (add minAge, store [NEW]); CacheMode 18-22.
- `Q/quarry-core/src/output.rs` [C]: NormalizedOutput gains cacheState and cachedAt [NEW] (fields absent today).
- `Q/quarry-edge/src/config.rs` [M] (cache_ttl_secs 33; default 319-320).
- `Q/quarry-edge/src/search_routes.rs` [M] (SEARCH_CACHE_DEFAULT_TTL_SECS 204; SearchCache use).
- `Q/quarry-runtime/src/artifact_store.rs` [C] (body storage for phase 2 [UNVERIFIED usage]).
- `GW/cache.rs` [C] (201 lines; FRESH_SECS 20, STORE_SECS 23) and `GW/domains/knowledge/quarry.rs` [C] (retire preview cache; CacheAdmission 213-285 is the spec for a shared should_cache).
- `GW/domains/search.rs` [C] (cache_key 218; store 286).
- Migrations: none for Redis phase 1. Phase 2 index would be `QR/crates/quarry-runtime/migrations/0010_*.sql` [NEW].
- Policy floor for no_egress: Control Plane [UNVERIFIED location].

### 3. REUSABLE BUT UNWIRED
- `cache::fingerprint()` (`Q/quarry-edge/src/cache.rs` [C] 84-98): no callers (`grep -rn "fingerprint(url" Q` finds only the definition).
- `CachePolicy.stale_while_revalidate_s` (quarry-core/src/cache.rs 13): consumer absent (grep `stale_while_revalidate` in Q finds only presets.rs 112, 168, 302 and cache.rs).
- `CachePolicy.vary_on` (quarry-core/src/cache.rs 11): consumer [UNVERIFIED].
- WIRED: should_read, should_write, effective_ttl (routes.rs 58-80); PageCache.put_with_ttl (cache.rs 59).

### 4. TESTS
- `cargo test -p quarry-edge --test cache_policy` (cache_read_write_serves_from_cache_on_second_call 78; cache_bypass_always_hits_origin 139; cache_write_only_skips_read_but_populates_cache 200; cache_max_age_zero_uses_default_ttl 265). Needs Redis; try_redis at 24 may skip [UNVERIFIED skip behaviour].
- `cargo test -p quarry-edge cache::` (key_is_org_scoped 211; key_stable_and_param_sensitive 221).
- Gateway: `cargo test` (verevonv3-ci.yml:141 [M]); knowledge/quarry.rs state_with_org_zdr 789 and the BI-2 preview test (~807).

### 5. FEATURE FLAGS / ENV
- `cache_ttl_secs` (config.rs 33) default 3600 via DEFAULT_TTL_SECS (cache.rs 15). Env name [UNVERIFIED].
- `max_age_s` default 3600 (quarry-core/src/cache.rs 29); SWR default 30 (31).
- Gateway FRESH_SECS 4 h (GW/cache.rs 20) and STORE_SECS 24 h (23); hard-coded.
- SEARCH_CACHE_DEFAULT_TTL_SECS 900 (search_routes.rs 204).
- ZDR bypass both sides (routes.rs 432 and 480; gateway CacheAdmission).

### 6. SURPRISES
- The audit says "bodies already live in the cached value". False: NormalizedOutput holds only FormatRef ids (output.rs 129-144), url, metadata and driver info (no body text). A cache hit returns artifact ids whose bodies may be gone. Phase 1 must store bodies or validate refs.
- Cross-tenant read: routes.rs 432-436 calls `cache.get(prev)` with key `quarry:page:{fingerprint}` (cache.rs 31-32) and no org check. NormalizedOutput has no org field (output.rs 10-27), so a caller who holds another tenant's body fingerprint gets that tenant's URLs, metadata and artifact ids. Exploitability depends on whether fingerprints leak [UNVERIFIED]. The fingerprint is a content hash, so identical bytes from two orgs share a key by design.
- `internal_run_page` (routes.rs 898-1046) also reads and writes this cache, which the audit does not mention.
- The gateway preview key is URL-only (`GW/domains/knowledge/quarry.rs` 104; `GW/cache.rs` 194-201), so it is not org-scoped either.
- `cache.rs` (quarry-edge) is [C] but its PageCache is keyed by content, so the URL-plus-options cache is a new key space [NEW].

<!-- COMPLETE -->
