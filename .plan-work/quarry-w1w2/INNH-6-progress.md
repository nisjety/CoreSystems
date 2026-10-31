# INNH-6 progress

2026-10-31 (lane C, branch innh/lane-c, worktree innh-lane-c)
- U1 done: edge flags (`CrawlFeatures`), `crawl_scrape.rs` (validated subset, canonical form), `crawl_request.rs` (sitemap enum, scrape_options, flag-off 400 kept), handoff stamping and `effective` echo, scope-off refusal for sitemap.
- U2 done: `crawl_sitemap.rs` (discovery route, collect/select, guarded fetcher), route registered, ZDR matrix row, web-policy manifest + sweep entry.
- U3 done: `run_page` honours `scrape` (cleaning, timeout, cache bypass, `scrape_applied` echo).
- U4 done and committed (788e98d90): Go `SitemapSeeds` activity, `CrawlJobWF` marker `crawl-sitemap-seeds-v1`, scrape options on every page, input builders. `go test ./...` green.
- U5 done and committed (c38304854 edge, 93e62474f tests): lib 694, integration crawl_sitemap 7, crawl_options_on 7, off 3, scope_off 1, web_policy_routes 11, zdr_feature_matrix 14, golden regenerated and re-checked.
- U6: docs (openapi, openapi-internal, CONTRACTS, RUNBOOKS, ERROR_REASONS, CRAWL_SCOPE_ROBOTS, SITEMAP_PARSING), route count 108 to 109.
- 2026-10-31 end: ALL DONE in tests. Left: docs commit; owed to a stack session: side-by-side evidence; owed: replay of an old history, Redis cache bypass end to end.
