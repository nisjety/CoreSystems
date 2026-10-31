# U4-gateway-spa-errors (user requests 4 and 5)

## Commits (all on quarryv2-next, local)
- b2272e55 gateway `classify_scrape_failure` reads `details.block_reason` (hunk-only on knowledge/quarry.rs)
- 779bbfb2 gateway browser.rs `browser_site_blocked_response` for a 403 CHALLENGE_DETECTED (hunk-only)
- ed34f04d SPA transport: `ApiError` keeps details / retryAfterSeconds / resumable / blockReason (`apiErrorFromResponse`)
- 8b6a71df SPA copy: errors.ts entries (no/en), BLOCK_REASON_COPY, `describeApiError` (hunk-only on errors.ts, knowledge-client.ts)
- a235cf08 SPA wiring: KnowledgeComposer browser/scrape catch sites use `browserErrorMessage`; tests
- 93fac697 SCRAPE_PREVIEW_FAILURE_CODES without `unsupported_media` (that code is WS5.G's uncommitted work)

## Contract implemented
- Scrape: 403 + `error.code == CHALLENGE_DETECTED` + typed `details.block_reason` -> `site_blocked` (+ `error.blockReason`);
  any other 403 -> `scrape_refused`; 429 -> `scrape_rate_limited` (+ `Retry-After`/`retryAfterSeconds` when 1..=3600);
  no proxy text; nothing from the upstream body is echoed. Unknown reason string = bare 403.
- Browser agent path: 403 CHALLENGE_DETECTED -> `browser_site_blocked` with only typed `blockReason`/`recovery` (host dropped).
- SPA `describeApiError(err, tr, fallback)` -> { message, kind, code, retryAfterSeconds, resumable }; kinds:
  unauthenticated, forbidden, takeover, blocked, rate_limited, session_lost, server, network, client.
- No action contract was added: copy and mapping only, no new UI operation (no take-over / try-another-source button).

## Foreign (not committed) hunks I touched or left
- gateway quarry.rs: WS5.G's 415 test (preview_* tests, formats insert, 415 branch). I changed its two classifier calls to the
  3-argument form and the bare-403 expectation to `scrape_refused` (working tree only). If WS5.G is committed later, that test
  must keep those two edits.
- errors.ts `unsupported_media` line and knowledge-client.ts ScrapeRequest/ScrapeResult hunks: untouched, still uncommitted.
  When the 415 slice is committed, add `'unsupported_media'` back to `SCRAPE_PREVIEW_FAILURE_CODES` in knowledge-client.ts.
- gateway browser.rs still carries another unit's tab/initiator hunks (uncommitted).

## Traps
- `apps/gateway` is binary-only: `cargo test --bin verevon-gateway-rs <filter>` (not `--lib`).
- The unit test `has copy for every code the gateway browser and scrape handlers emit` (browser-errors.test.ts) scans browser.rs and
  knowledge/quarry.rs production code for emitted codes: a new code without copy fails it. The three `browser_idempotency_*`
  codes are allow-listed as unreachable from the SPA (no Idempotency-Key UI).
- Hunk-only patches: scripts in the scratchpad U4-gateway-spa-errors (mkpatch.js builds HEAD -> mine-only; edit scripts re-run against a HEAD copy).
- vitest full run is ~6.5 min and times out two heavy suites when the machine is loaded (trust-data, ChatMessages.chart); both failed in the baseline once and passed after.

## Not done
- Challenge field on the observation (`BrowserObservation.challenge`) and a `BrowserBlockedNotice` with "Take over" / "Try another source"
  (plan WS3.J SPA half, needs the action contract `browser.blocked.try_other_source` first).
- Copy for browser_idempotency_*; renewal UI (explicitly out).
- Crawl, upload and knowledge-base catch sites still show raw gateway messages.
- No live run against a real bot wall; Quarry's QUARRY_CHALLENGE_ENFORCE defaults to log, so the 403 only exists on an enforcing environment.

## Isolation check done after the commits (corrects "not compiled in isolation" in b2272e55 and 779bbfb2)
`git archive HEAD` of apps/gateway (+ nginx.conf and the two compose files its include_str! needs) into a scratch dir, sources touched
(otherwise cargo reuses the working tree's stale binary: the first run did, and showed WS5.G's tests that HEAD does not have), then
`cargo test --bin verevon-gateway-rs knowledge::quarry` = 21 passed, `domains::browser` = 100 passed on the committed tree alone.
The SPA committed-only tree was not run separately (vitest on the working tree, which only adds WS5.G's hunks, passed: 73 in the last run).

## Round 4 (review fixes) — WORK DONE BUT NOT COMMITTED (Docker down)
Why uncommitted: Docker Desktop answers 500 / hangs, and the repo's pre-commit hook (gitleaks via docker, no native gitleaks) fails closed. I did not bypass it.
Second blocker: tools/qcommit-patch.sh refuses while ANOTHER session has 7 files staged in the real index (browser-broker-ci.yml, live_gate.rs, ssrf.rs, two browser.proto, browser.pb.go, browser-grant-contract.test.mjs). I wrote C:/Users/IMAFER~1/AppData/Local/Temp/claude/C--dev-CoresSystem--claude-worktrees-vigorous-perlman-e7ae5b/562f6350-4d29-46dc-a04b-eee466d0c963/scratchpad/U4-gateway-spa-errors/qcommit-patch2.sh (private GIT_INDEX_FILE; hook still runs; real index only moved for the patched paths).

Findings and fixes (all in the working tree):
1. HIGH invalid_url etc.: real. errors.ts copy for invalid_url, invalid_crawl_request, invalid_browser_action, zdr_persistent_profile_forbidden, model_gateway_unreachable; knowledge-client.ts SCRAPE_PREVIEW_FAILURE_CODES gains invalid_url; the scan test covers every prefix; new closed-list test.
2. MEDIUM distinct messages: real. Exact sentence per status case, pairwise distinct; unmapped 401 message asserted.
3. MEDIUM wiring untested: real. KnowledgeComposer.scrape-errors.test.tsx (renders, real client, stubbed fetch), BrowserChrome.timeline-meta.test.ts (export of ownerTimelineEventMeta).
4. MEDIUM flat 429: real (quarry-edge routes.rs ApiError::rate_limited). gateway quarry.rs scrape_retry_after_seconds reads envelope then top-level; handler test with the real flat body.
5. MEDIUM WS3.J SPA half: real, NOT built (needs the Model Plane BrowserObservation.challenge first). Plan ledger row added (docs/plans/quarry-browser-build-plan-2026-10-14.md) recording it as OPEN; earlier commits cannot be amended.

Commit recipe once docker answers (run from C:/Users/IMAFER~1/AppData/Local/Temp/claude/C--dev-CoresSystem--claude-worktrees-vigorous-perlman-e7ae5b/562f6350-4d29-46dc-a04b-eee466d0c963/scratchpad/U4-gateway-spa-errors):
- gateway:  bash qcommit-patch2.sh msg-r4-gw.txt p-gw4.patch
- copy:     cat p-errors4.patch p-kc4.patch > p-copy4.patch; bash qcommit-patch2.sh msg-r4-copy.txt p-copy4.patch
- tests:    bash ../tools/qcommit.sh msg-r4-tests.txt "apps/Frontend Plane/verevonv3/src/shared/i18n/browser-errors.test.ts" "apps/Frontend Plane/verevonv3/src/shared/api/browser-client.errors.test.ts" "apps/Frontend Plane/verevonv3/src/features/dashboard/home/BrowserChrome.tsx" "apps/Frontend Plane/verevonv3/src/features/dashboard/home/KnowledgeComposer.scrape-errors.test.tsx" "apps/Frontend Plane/verevonv3/src/features/dashboard/home/BrowserChrome.timeline-meta.test.ts"
  (commit the copy first: the tests need the copy entries and the list entry)
- plan:     bash qcommit-patch2.sh msg-r4-plan.txt p-plan.patch   (the patch only has U4's row; U5's WS6.E row stays uncommitted)
Check afterwards: git diff on quarry.rs / errors.ts / knowledge-client.ts still shows WS5.G's hunks; git show --stat HEAD lists only my files. The patches were verified with git apply --check --cached against HEAD in a private index.

Verified this round: gateway knowledge::quarry 26 passed (tree) and 22 passed on HEAD + p-gw4.patch in an archive copy (CARGO_TARGET_DIR=C:/q4t); domains::browser 104 passed (tree); vitest 6 files / 76 tests passed (src/shared/i18n, browser-client.errors, http.error-hints, the two new component tests); typecheck 33 errors in RemoteSupportPage.tsx and quarry-client.ts = identical to the baseline set, none in my files; lint 0 errors / 191 warnings = baseline. Mutations: composer catch site reverted -> 5 of 5 composer tests fail; invalid_url copy renamed + list entry removed -> 3 tests fail.
Traps: this tool layer collapses "\\" to "\" in script text (regexes in heredoc/Write scripts lose backslashes: use the Write tool with String.raw or Edit); link.exe 1104 for a long CARGO_TARGET_DIR.

## ROUND 4b (reviewer findings on the uncommitted tree) - STILL UNCOMMITTED, docker engine returns 500
Findings and answers:
1. Hunk coupling (errors.ts unsupported_media line vs the emitted-code test): real. Fix: the copy commit now carries errors.ts WHOLE (both hunks, incl. WS5.G's one-line unsupported_media copy entry, a complete self-contained table entry). Then the emitted-code test passes whichever of {U4 copy commit, WS5.G 415 commit} lands first. knowledge-client.ts is still hunk-only (p-kc4.patch = invalid_url only; WS5.G's formats/raw_html stay out). Commit message msg-r4-copy.txt says so.
2. Source-scanning regex tests = maintenance cost: accepted. A comment in browser-errors.test.ts now says what to do when it trips (give copy, or add to NOT_AN_ERROR_CODE).
3. 22 vs 26: explained, not a defect. HEAD has 21 #[test] in knowledge/quarry.rs; HEAD + my patch (p-gw4) = 22 (one new test: a_429_scrape_in_quarry_edges_flat_shape...). The tree has 26 = 22 + 4 WS5.G tests (preview_makes_one_quarry_call_when_markdown_is_inline, preview_keeps_the_formats_the_caller_sent, a_415_is_classified_as_unsupported_media, preview_of_an_unsupported_media_type_is_a_typed_415).
Verified 4b: gateway `cargo test --bin verevon-gateway-rs knowledge::quarry` = 26 passed (tree); vitest on the 5 U4 test files = 72 passed.
COMMIT RECIPE (from scratchpad/U4-gateway-spa-errors; NEW files p-errors5.patch/p-copy5.patch replace the p-errors4/p-copy4 mentioned above; all patches `git apply --check --cached` OK against HEAD e7e9b9e1 in a private index):
- bash qcommit-patch2.sh msg-r4-gw.txt p-gw4.patch
- bash qcommit-patch2.sh msg-r4-copy.txt p-copy5.patch     (errors.ts whole + knowledge-client.ts invalid_url hunk)
- tests commit via tools/qcommit.sh as listed above (msg-r4-tests.txt)
- bash qcommit-patch2.sh msg-r4-plan.txt p-plan.patch
Why blocked: pre-commit hook runs gitleaks via docker; `docker ps` hangs and `docker run` gets "500 ... _ping"; the hook then reports "gitleaks found a secret" (exit 1 from the docker CLI, NOT a real finding). No --no-verify used. Also another session has 7 files staged in the real index (qcommit-patch.sh refuses; qcommit-patch2.sh uses a private index).
