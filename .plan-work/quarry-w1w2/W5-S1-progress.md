# W5-S1 progress: 071a built, 069 step 1 not built

Date: 2026-11-07. Branch `quarryv2-next` (local only). Commit `79843694c`.

## Built (committed)

- 071a: deleted `WebPageViewer` from `apps/Frontend Plane/verevonv3/src/features/dashboard/home/DashboardSearchResults.tsx` (it rendered an iframe with `allow-scripts allow-same-origin`) and its `.dashboard-webview*` rules from `apps/Frontend Plane/verevonv3/src/styles/global.css` (115 lines). No importer existed. No behaviour change.

## Not built

- 069 step 1 (allow `pdf` and `screenshot{full_page}` in the gateway `sanitize_action`, `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/browser.rs` line 4466). Not done, for a reason the spec does not cover. A `pdf` action would produce a PDF artifact, and the browser artifact route serves it through `safe_browser_artifact_content_type` (browser.rs 5312-5326). That function has no `application/pdf` arm and falls back to `image/png`. Fixing it needs a decision the spec does not make: serve the PDF inline on the app origin, or as an attachment with `nosniff`. Inline on the app origin is the risk. Owner call needed before this step.

## Ran (this session)

- `eslint src/features/dashboard/home/DashboardSearchResults.tsx`: exit 0, no problems reported.
- `vitest run src/features/dashboard/home --maxWorkers=2`: 55 files, 665 tests passed, exit 0.
- `pnpm run typecheck` (`tsc -b` after the build steps): 22 `error TS` lines, 21 in `src/shared/api/quarry-client.ts` and 1 in `src/shared/api/schedule-contract.test.ts`. None is in a file this change touched. The documented baseline (33 errors, 12 of them in `RemoteSupportPage.tsx`) does not match this fresh install, so I make no exact before and after claim.

## Read only

- Gateway tests: not run. No gateway file changed, and the gateway has no build target yet (a cold build is required).
- `BrowserArtifactMediaKind` already includes `'pdf'` (`browser-session.ts`, per W5-ORCH section 3). Read, not changed.

## Environment note (not in any commit)

- The committed `pnpm-lock.yaml` on this branch records `@quarry/client` as `file:../Ingestion Plane/...` (one `..` short, lines 27 and 552-553). Main's lockfile has `../../`. `pnpm install --frozen-lockfile` therefore fails with ENOENT on this branch. For these checks I patched the lockfile in place, installed, and restored it byte for byte (SHA-256 compared). The lockfile is unchanged in git. The fix belongs in its own commit.

## Proven

- Nothing is proven live. Tests only.
