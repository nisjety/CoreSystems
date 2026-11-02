# W2-U65-51 status: crawl options UI and scrape captures in the composer (D2-068, D2-092)

Branch w23/lane-b. Date 2026-11-02.

## Built
- Gap build only. Already existed and not rebuilt: the action contract (`crawlSiteInput` in src/shared/actions/action-registry.ts already has maxDepth, includePaths, excludePaths, allowSubdomains with the edge caps 10 / 100 / 512; `scrapeUrlInput` already has `capture`), `CrawlRequest` and `startCrawl` already sent the scope, `ScrapeCaptureRequest`, `scrapeRequestBody`, `readScrapeCaptures` and `ScrapeCapturesSection`. The TS18048 at crawl-site-scope.test.ts:33 was already fixed in 3f3446fe7 (`match[1] ?? ''`). So no action-contract change was needed (contract first = confirmed, nothing to add).
- knowledge-client.ts: `parseCrawlPatterns` (one per line or comma separated, brace commas kept, blanks/over-long/repeats dropped, capped at 100 x 512), `buildCrawlScopeOptions` (only options the user set; depth clamped 1..10; subdomains only when ticked), `buildCaptureRequest` (true only when chosen, never false), `knowledgeScrapeOptionsEnabled` (build flag), exported cap constants.
- KnowledgeComposer.tsx (real path src/features/dashboard/home/, not features/knowledge/components): crawl form gets depth, include paths, exclude paths and include-subdomains; both the plain crawl and the agent-run (`knowledge.crawl_site`) path send the scope. Link form gets "screenshot" and "PDF" toggles. The preview scrape signals (the old lines 798-799 sent false) now send `screenshot`/`pdf` true only when ticked. Because the preview route drops capture formats (gateway strip_preview_only_fields), a ticked capture makes one extra non-ingesting `/scrape` call with `capture`, and its artifact references are merged into the preview's `captures` (the existing ScrapeCapturesSection renders them). A failed capture call shows an error and keeps the preview.
- Not covered: the "Velg sider" page-picker crawl (`crawlSelectedPages`) takes an explicit URL list, so the scope options do not apply to it.

## Commits
- 0600a4d70 feat(knowledge-client): crawl scope parsing and capture toggles helpers behind a build flag
- 7b320394d feat(knowledge): crawl scope options and scrape capture toggles in the composer, flag off
- plus the docs commit for this file and the depth rows.

## Tests (host, vitest in apps/Frontend Plane/verevonv3)
- `vitest run knowledge-client knowledge-scrape-capture crawl-site-scope KnowledgeComposer`: 6 files, 42 passed, 0 failed (before: 3 files, 14 tests for the three named; the two existing composer files 16 tests also still pass).
- New: knowledge-client.test.ts +4 (patterns split/caps, scope builder, startCrawl body), knowledge-scrape-capture.test.ts +2 (toggle request, formats), KnowledgeComposer.scrape-options.test.tsx 6 (flag off: no controls, signals false, no /scrape; flag on untouched: same; screenshot ticked: signals + `/scrape` formats [markdown, screenshot]; crawl scope typed; crawl untouched sends no scope keys; crawl flag off hides controls).
- tsc6 -p tsconfig.app.json --noEmit: 1 error, the pre-existing schedule-contract.test.ts TS2352; nothing new. (The queue baseline of 33 does not hold in this lane: the SDK resolves, so the 21 quarry-client errors are absent.)

## Flag and default
`VITE_KNOWLEDGE_SCRAPE_OPTIONS_ENABLED` (build flag, read once when the composer mounts). Default off: the controls are hidden and the composer sends exactly what it sent before (signals screenshot/pdf false, no /scrape call, no scope keys).

## Assumed decisions
- One flag covers both crawl scope and captures (same surface, same release).
- Captures through one extra `/scrape` call after the preview, because the preview route strips capture formats by design; no ingest on that call.
- Pattern box accepts newline or comma separation; a comma inside `{a,b}` stays in the glob.
- `allowExternalLinks` is not exposed (UI-only on the client type, but the plan keeps external links off the Model surface and no owner decision asked for it in the form).
- Depth blank = not sent (edge default); depth is clamped to 1..10.
- Over-long (>512) patterns are dropped silently and the list is capped at 100 (same bounds as the edge); no inline warning copy in this unit.

## Model Plane asks
None.

## Not verified
- No browser/visual check: the new inputs reuse the `crawl-options` class; the text inputs have no dedicated CSS (global.css is outside this unit's files), so they may look unstyled next to the number input.
- The gateway side was not re-run: the crawl scope and `/scrape` capture handling are assumed per existing gateway tests; no live crawl or capture was run.
- ZDR inline screenshots go through the same merge but were not exercised end to end in the composer.
