# T2 / Category 2: Output formats and content processing (cross-source comparison)

Inputs: 50 reference records (firecrawl 37, codexbrowser 8, aside 3, apify 2) and 26 Verevon baseline records. Output: 26 capability clusters, all 50 records assigned, 0 excluded. Provenance in the clusters: mostly SRC. Apify records are DOC only, and one Codex record (asset bundling, 113) is DOC. Clusters file: `02-output-formats-content-processing.clusters.json`.

## Shared baseline (table-stakes)
For web-scraping sources, "table-stakes" means the capability is in both Firecrawl and Apify. Codex and Aside are browsers, not scrape APIs.
- **Main-content extraction with selector filters** (C-002: firecrawl, apify). Verevon: **partial**. It has one fixed Readability pass, with no include/exclude selectors, no onlyMainContent toggle and no full-HTML fallback.
- **Screenshot and print-to-PDF output** (C-004: firecrawl, codexbrowser, aside). Verevon: **partial**. Capture exists only as agent actions. On scrape, the screenshot and pdf outputs are always None.
- **Office/e-book conversion and file capture during crawl** (C-017: firecrawl, apify). Verevon: **stub**. `docx.rs` has no caller, and there is no XLSX or PPTX support.
- Also baseline-level: HTML-to-Markdown (C-003), where Verevon has **shipped** it.

## Best of breed
| Capability | Best source | Why |
|---|---|---|
| Format registry and document response (C-001) | Firecrawl 038/039 | Typed `formats[]` union, each with its own options; one Document with cache, proxy and credit metadata |
| Main content (C-002) | Firecrawl 086 (+Apify 050 idea) | Streaming Rust transform: 42 chrome selectors, force-include regions, lazy-image resolution. Apify lets users choose the extractor and fall back to full HTML |
| Screenshots/PDF (C-004) | Aside 027 | Scroll-and-stitch tiles with 200 px overlap, waits for rendering between tiles, element shots, printToPDF |
| JSON extraction (C-009) | Firecrawl 090 | Strict-schema normalisation, model choice by schema complexity, trim to 80% of context, JSON repair, provider fallback |
| PDF (C-015/016) | Firecrawl 080-084 | Native classification and fast path; OCR only when needed; typed layout blocks; sha256 parse cache; async job adoption; parser semaphore |
| Change tracking (C-013) | Firecrawl 094 | Whitespace-normalised compare, git and JSON diffs, history kept per tag |
| Agent page projection (C-005) | Codex 134 | Visible interactive DOM with an attribute allowlist, capped at 200 elements / 20k chars |
| Rich chat rendering (C-008) | Aside 081 | Mermaid, Shiki, KaTeX, file previews (Verevon matches) |

## Differentiators and unique ideas worth taking
- **Session-authenticated export** (C-024, Codex + Firecrawl): Google Workspace export, transcripts, media. For Verevon, the better route for documents is OAuth provider actions in integration-corev2. Nice-to-have.
- **deterministicJson** (C-011, Firecrawl): the LLM writes an extractor once, and later scrapes reuse it. Verevon should generate its existing declarative `ExtractionProfile` rather than JS code. This matches "Model proposes, Quarry executes".
- **SmartScrape escalation** (C-012) and **goal-aware monitor judge** (C-014): both route an LLM decision into a bounded Quarry action.
- **Fail-closed PII redaction** (C-019): fits Verevon's GDPR and ZDR stance.
- **Model-facing image budget** (C-007, Codex): cuts cost in long browser runs. Verevon status is unknown.

## Verevon's biggest gaps (priority tier)
1. **C-009 JSON extraction on scrape** (sig 5). JSON is only on `/v1/extract`. There is no strict-mode request or repair. The cross-plane token manifest disables `model-invoke`, so this does not work outside dev bypass.
2. **C-001 formats registry** (sig 4). `/v1/scrape` has no `formats[]`. A ZDR scrape returns no content because outputs exist only as artifact refs that are never written under ZDR.
3. **C-015 PDF in the scrape/crawl pipeline** (sig 4). lopdf is used only by the answer fetcher. The Model Plane `doc_intel` (Azure prebuilt-layout) provider exists, but Quarry does not call it.
4. **C-013 change diffs** (sig 4). The compare is a raw-byte blake3 fingerprint, which flags noise as change. `diff.rs` (SemanticDiff) has no caller, and there is no text or JSON diff. Monitoring already ships in the UI, so users see this.
5. **C-002 main-content controls** and **C-004 screenshot/PDF formats** (sig 4). Both are cheap because the code exists in readability.rs and chromiumoxide.rs.
- Recommended next: C-010 summary/question/highlights, C-011, C-014, C-016, C-017 Office, C-019 PII, C-021 attributes/images (code already written, needs wiring), C-005 projection budget and UI surfacing, C-007 (verify first).
- Skip: OMCE learned boilerplate (C-020; experimental, needs a cross-tenant index), QR decoding (C-026; a browser-auth concern).

## Verevon strengths (tier n/a)
- Native Rust HTML-to-Markdown inside Quarry, with charset sniffing, soft-404 detection and a body cap (C-003).
- Chat rendering already has KaTeX, Mermaid (rendered only once the fence closes), code highlighting, CSV artifacts and grouped citations (C-008; verified in code in this pass).
- Product extraction with a picker UI, plus non-LLM structured harvest from JSON-LD, hydration data and tables (C-023, C-009 strengths).
- Model routing, fallback and ZDR-aware provider selection are centralised in the Model Plane (C-025), which is a stronger design than per-service routing.
- Provenance: field-level source trace on ingest, determinism/policy stamp, and title provenance (C-001 strengths).

## Caveats
- Firecrawl supplies 37 of the 50 records, so the record set is biased toward it. Many clusters are "unique" because only Firecrawl exposes a scrape API in depth. Apify's two records are DOC only, so prevalence understates Apify.
- C-001 and C-009 are classed "common" although only Firecrawl appears in this record set. The basis is inference: these are standard features of scrape APIs.
- Some Verevon statuses come from this pass and have no baseline record: C-008 shipped, C-019 partial, C-025 shipped (file presence only), C-026 absent (grep). C-007 is **unknown**. Model Plane Rust has no image budget by grep; Go services were not searched.
- Monitoring and snapshot storage conflict with ZDR. Any diff or cache feature must be hash-only or refused under ZDR.
