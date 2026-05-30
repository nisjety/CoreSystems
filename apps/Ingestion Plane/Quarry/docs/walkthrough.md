## Quarry Worker Debugging Walkthrough

Summary:
- Added trace logging to `internal/scraper/browser_pool.go`, `internal/scraper/scraper.go`, and `internal/temporal/activities.go` to surface page creation, navigation, and activity lifecycle.
- Made `MaxConcurrentPages` and `BrowserPoolSize` configurable via `MAX_CONCURRENT_PAGES` and `BROWSER_POOL_SIZE` environment variables in `internal/config/config.go`.
- Enabled `ZEROLOG_LEVEL=debug` for the worker and toggled zerolog debug in `cmd/worker/main.go`.
- Rebuilt and started `quarry-worker` with debug logging and triggered a scheduled crawl for `https://triodelab.no`.

Key observed logs (relevant sequence):

- `BrowserPool: launcher launched URL=...` — browser launched successfully.
- `BrowserPool: rod connected to browser` — rod connected to browser.
- `FetchPageActivity: starting fetch for https://triodelab.no` — worker activity started.
- `FetchPageActivity: fetched html length=46698 for https://triodelab.no` — page fetched.
- `AnalyzePageActivity: invoking module.Run for https://triodelab.no (maxDepth=1, enrich=true)` — analysis started.
- `GetPage: browser.Page starting` / `GetPage: browser.Page created` / `GetPage: stealth injected` — page tabs created and stealth injected.
- `scrapePage: navigating to https://triodelab.no` / `scrapePage: page loaded https://triodelab.no` — navigation succeeded.
- `enrichProduct: calling extractor for https://triodelab.no/kontakt (html_len=26607)` — enrichment attempted.
- `ai extraction failed, falling back to heuristic` with error `extract_data failed: Failed to decode TOON: a bytes-like object is required, not 'str'` — AI-Core TOON decoding error observed during `ExtractData` call.

Findings:
- The quarry-worker and browser pool are functioning: pages are created, navigations complete, and HTML extraction works.
- The primary failure during enrichment is an `extract_data` decoding error originating from AI-Core (TOON conversion). The worker falls back to heuristic extraction after the AI error.

Next recommended steps:
1. Fix the TOON decode error in AI-Core's `ToonConverter` (likely a bytes vs string handling bug). Once fixed, re-run the scheduled crawl to verify `extract_data` succeeds.
2. If deeper rod internals are needed, enable rod's debug logger in `browser_pool.go` and add page-level screenshots for failing pages to aid reproduction.
3. Commit/log these changes and include log excerpts in the final project `walkthrough.md` record.

Logs captured during this run are available via `docker-compose logs quarry-worker`.
