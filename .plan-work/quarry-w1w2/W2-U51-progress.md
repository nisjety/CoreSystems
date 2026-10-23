# W2-U51 progress (WS13.7: gateway artifact route, preview admission, SPA rendering, batch 9)

Status: BUILT to the plan's scope, dark by design (no flag; the route is inert until a caller asks for a capture, and captures need `QUARRY_EDGE__SCRAPE_CAPTURE=on`). Compile-checked and two single SPA test files run; the gateway suite, clippy, fmt and the e2e spec were NOT run (build-first strategy, 2026-10-23). Nothing deployed; no stack evidence.

## Committed (local branch quarryv2-next, not pushed)
- 70e41deb0 feat(gateway): serve scrape capture artifacts and strip capture fields from the preview (WS13.7).
  `domains/knowledge/quarry.rs` (route handler, preview strip, 10 tests), `domains/browser.rs` (pub(super) visibility, new `scrape_artifact_content_type`, rewritten `get_artifact` comment), `domains/knowledge.rs` (route), `cache_admission.rs` (deny keys).
- 1b4d83606 feat(spa): scrape capture client, Skjermbilde / PDF section and PDF artifact kind (WS13.7).
  `knowledge-client.ts`, `knowledge-preview.ts`, `KnowledgeScrapePreview.tsx`, `errors.ts`, `action-registry.ts`, `browser-session.ts`, `BrowserTimelineDetail.tsx`, plus two new test files.
- (this commit) docs(plan-work): W2-U51 progress, depth and hand-over notes.

## Route contract as built
- `GET /api/v1/knowledge/scrape-artifacts/{artifact_id}` (SearchScrape rate class, session required).
- 400 `invalid_scrape_artifact` (id must be `art_` + path-safe chars); 404 `browser_artifact_withheld` for a ZDR org (posture read first); 404 `scrape_artifact_not_found` for upstream 404/410 (also another org's id); 413 `scrape_artifact_too_large`; 415 `scrape_artifact_unsupported` for any type other than png/jpeg/webp/pdf (no image/png fallback); 502 `scrape_artifact_failed`.
- 200 headers: `Content-Type` from the allowlist, `Cache-Control: private, no-store`, `X-Content-Type-Options: nosniff`; PDF adds `Content-Disposition: attachment; filename="page.pdf"` and `Content-Security-Policy: sandbox`.
- `scrape_preview` now removes `actions`, `actionsOnError`, `viewport`, `capture`, the `screenshot`/`pdf` formats and non-string formats before the cache admission and before the browser-failure retry.

## Deviations from the slice text (recorded)
- `BrowserArtifactAbsence` was not moved to `shared.rs`; it is `pub(super)` in browser.rs.
- The browser route's `safe_browser_artifact_content_type` was not changed; the new strict helper is `scrape_artifact_content_type` beside it.
- Upstream 404/410 is the contract code `scrape_artifact_not_found` (status and envelope as the browser route's NeverStored).
- Tests live in `quarry.rs` (`scrape_artifact_tests`), not `tests/scrape_artifact_route.rs`.

## Not done (and why)
- RUNBOOKS section: `apps/Ingestion Plane/Quarry-v2/docs/RUNBOOKS.md` has another unit's uncommitted hunks; to append after that unit commits. Draft text for the section is in `.plan-work/quarry-w1w2/wf-W2-U51.md`.
- Composer wiring: nothing in the SPA requests captures yet (`KnowledgeComposer.tsx` uses the text-only `scrapePreview`). The section renders only when a caller passes a capture.
- Action handler (`domains/actions/handlers.rs`, `dispatchers.rs`) does not forward `capture` yet (H13).
- Playwright spec `tests/e2e/knowledge-scrape-capture.spec.ts` not written.
- Gateway: `cargo test`, `cargo clippy -D warnings`, `cargo fmt --check` not run (the file was already not rustfmt-clean before U51; the new blocks were written by hand to match).
- SPA: `action-registry.test.ts`, `catalog-manifest.test.ts`, `knowledge-preview.test.ts` not run.

## Verified (what was actually run, 2026-10-24)
- `cargo check --tests` in `apps/Frontend Plane/verevonv3/apps/gateway` (native): rc 0. One warning, unused import `ResponseTemplate` in browser.rs test module (pre-existing, not from U51).
- `vitest run src/shared/api/knowledge-scrape-capture.test.ts`: 1 file, 5/5 passed.
- `vitest run src/features/dashboard/home/KnowledgeScrapePreview.captures.test.tsx`: 1 file, 4/4 passed (after making the error-state assertion `waitFor`-based; Solid 2 updates are async under fireEvent).
- `./node_modules/.bin/tsc -p tsconfig.app.json --noEmit`: 33 errors, all in `quarry-client.ts` (21) and `RemoteSupportPage.tsx` (12); none in U51 files.

## Next batch needs to know
- Batch 10 units that touch `knowledge-client.ts`, `KnowledgeScrapePreview.tsx` or `quarry.rs` (H11) start from these commits.
- The capture formats on `/v1/knowledge/scrape` are `screenshot` / `pdf` strings or objects; the preview never carries them.
- WS13.7b (optional PDF timeline card) can use the `pdf` media kind already added; its agent-path half is sibling (b).
