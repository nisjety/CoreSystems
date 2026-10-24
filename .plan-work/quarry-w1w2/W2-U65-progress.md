# W2-U65 progress: WS14.S6 (CrawlRequest v2, scoping vocabulary on the crawl request)

Unit W2-U65, gated group G2-c, wave 2 (build first, depth later). Branch `quarryv2-next`, worktree `vigorous-perlman-e7ae5b`. Built to the plan's scope for the request contract, the handoff wiring, the Model surface and the SPA options. Tests written; compile-checked (edge and gateway) and one single-file vitest run. The Rust unit tests and the gateway unit tests were NOT run (wave rule).

## What is built

- Edge `crates/quarry-edge/src/crawl_request.rs` (NEW, LF): `CrawlRequestSettings` (`QUARRY_EDGE__CRAWL_REQUEST_VALIDATION=lenient|strict`, default lenient; an unknown value is logged without its value and treated as lenient), `FIELDS` (every option with its spellings), `RESERVED_KEYS` (refused in both modes), `parse_body` (the whole request checked, all problems reported together), `Refusal` (400 Invalid / 403 PolicyDenied), `ParsedCrawl` (url, max_pages, max_depth, ingest, zdr, the scope, warnings), `fields_json`.
- Edge `routes.rs` `crawl_handoff`: takes `Bytes`, parses with `crawl_request::parse_body`, maps refusals through `crawl_request_refusal` (400 `BAD_REQUEST` with `details.fields`, 403 `FORBIDDEN` with `details.reason = policy_denied`). The untyped `CrawlRequest` struct is removed (its only user was this handler). The scope it stamps carries the requested patterns, the domain switch and the extension list (`HandoffScope::with_request_scope`). `HandoffAck.effective` gains `request_warnings` (`{code, field}` for ignored keys).
- Edge `crawl_policy.rs`: `HandoffScope::with_request_scope` and the scope notes `patterns_reported_not_enforced` (observe), `scope_not_applied_legacy` (off), `same_directory_only`.
- `lib.rs` and `main.rs`: one `crawl_request` module line each.
- Gateway `domains/knowledge/quarry.rs`: `Surface { Ui, Model }`; `normalize_crawl_body` keeps its signature (UI); new `normalize_crawl_body_for(input, surface)`. Model drops `allow_external_links`. Adds `maxDepth` (clamp 10, zero takes the default), `includePaths`/`excludePaths` (same caps as the edge, 100 patterns, 512 characters, 8192 in total). `pattern_list` helper. Tests: `model_surface_drops_external_and_robots_keys`, `ui_surface_forwards_scope_fields`, `crawl_site_action_goes_through_the_ssrf_normalizer`, `max_pages_is_clamped_on_the_action_path`, `pattern_caps_match_the_edge_boundaries`.
- Gateway `domains/actions/dispatchers.rs`: `dispatch_crawl_site` uses `normalize_crawl_body_for(.., Surface::Model)`.
- SPA `src/shared/api/knowledge-client.ts`: `CrawlRequest` gains maxDepth, includePaths, excludePaths, allowSubdomains, allowExternalLinks; `startCrawl` forwards them when set; new type `CrawlEffectiveScope` (not yet read by `normalizeCrawlJob`).
- SPA `src/shared/actions/action-registry.ts`: `crawlSiteInput` gains maxDepth (max 10), includePaths, excludePaths (max 100 entries, 512 characters), allowSubdomains. No action id added (H13 rule kept).
- SPA `src/shared/actions/crawl-site-scope.test.ts` (NEW): asserts the schema has no robots or external key and carries the caps. Run: 3 of 3 passed.
- openapi `docs/openapi.yaml` `CrawlRequest`: include_paths, exclude_paths, deny_extensions, crawl_entire_domain, sitemap, ignore_query_parameters, scrape_options, ignore_robots_txt (additive).
- `docs/CRAWL_SCOPE_ROBOTS.md` section 12: the request table, the refusals, the ack, the shared cap table, the Model boundary.
- `docs/RUNBOOKS.md`: section "Crawl request contract (WS14.S6, unit W2-U65, built dark, NOT run)" (append only).
- Orchestrator: no change. `params.scope` already carries include/exclude/deny/crawl_entire_domain (W2-U60 `CrawlScope` in Go), so the plan's `dispatcher.go`/`workflows.go` row is satisfied without a new edit.

## Verification (run, honest)

1. `cargo check -p quarry-edge --tests` via qtest: rc 0 (two runs; the first retry ran after the sibling's quarry-runtime `robots_cache` edit settled). Warnings in the log are in `fleet_routes.rs` and `queue_routes.rs` (line 183), not in this unit's files.
2. `cargo check --tests` in `apps/gateway` (native host): rc 0. One warning (unused import `ResponseTemplate` in a test module of browser.rs), pre-existing and not in this unit's files.
3. `npx vitest --run src/shared/actions/crawl-site-scope.test.ts` (single file): 3 passed.
4. Not run: `cargo test -p quarry-edge crawl_request`, gateway `normalize_crawl` tests, the SPA knowledge-client test (not written), any router-level test.

## Not done (in scope, with reason)

- KnowledgeComposer options UI ("Stay on this site", "Include subdomains", "Follow links to other sites", include and exclude lists, nb and en copy). The plan lists it under S6, but the queue row for U65 does not own `KnowledgeComposer.tsx` (U60 owned its summary row, not built). Left for the next batch.
- `knowledge-client.ts` `normalizeCrawlJob` does not yet read the echoed `effective` scope into `CrawlEffectiveScope`. The type exists; the reader does not.
- Router-level test "end-to-end through the router with the orchestrator double (stamped params contain the full scope)": not written.
- The plan's `map_routes.rs` soft row (`MapRequest` scope fields): skipped (soft, only if F-t2-055 has not rewritten the file; not checked).
- Deployment evidence (a) to (h): stack lane, needs the rebuilt edge, orchestrator and gateway. Evidence (a) and (b) need `QUARRY_EDGE__CRAWL_SCOPE_MODE=enforce`; under the default `observe` the patterns are reported, not applied (see depth, high).
- `ignore_robots_txt` for service principals: every `true` is refused at the edge now, so the second line is in force, but the separate S7 override branch is not built (by design).
- `deny_extensions` is accepted at the edge and the Model path does not forward it (the Model surface does not carry it); the UI does not offer it.

## Shared files: what this unit touched and what was left alone

- Hot files, exact-anchor edits only: `routes.rs` (crawl_handoff region, struct removal, the BatchRequest doc link), `main.rs` (one mod line), `lib.rs` (one mod line), `quarry.rs` crawl normaliser region, `dispatchers.rs` (one call), `action-registry.ts` (`crawlSiteInput` only), `openapi.yaml` (CrawlRequest only), `RUNBOOKS.md` (append only).
- Other units' uncommitted hunks left alone: `routes.rs` (WS17.P0 profile routes and body limit), `main.rs` (ZDR profile warning), `quarry.rs` and `openapi.yaml` (other sections), `RUNBOOKS.md` (other units' sections, which are reordered in the working copy, the content is intact in the working copy and HEAD), `profile_routes.rs`, quarry-runtime files (W2-U63 in progress).
- Commit method: patches built from HEAD plus this unit's edits only, applied with `qcommit-patch.sh` (the working tree is not touched).

## Notes for the next batch

- `crates/quarry-edge/src/crawl_request.rs` was created by another agent a few minutes before this unit's write (see the hand-over). The write replaced that file. Whoever owns WS14.S5 (`validate_batch_urls`, W2-U64) must re-add it as an append-only region of this module (or as its own module) and must not overwrite it.
- Keep `CRAWL_MAX_*` (gateway) and `MAX_*` (quarry-core) in step; the doc table is the reference.
- The commit attribution line: the system attribution instruction names Claude Haiku 5.5, the queue text names Sonnet 5.5. This unit follows the system instruction (same choice as W2-U60); flagged for the owner.
