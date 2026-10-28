# D2-B-x10-robots: hand-over (decision 3.1, X-10, list B)

Status: list B prepared and tested, flags off or observe. The enforce flip, the stack proof and the soak are NOT done here and are not this unit's to do (the owner's, after the soak).

## What is committed (branch quarryv2-next, local, not pushed)

| Commit | Subject (short) | What |
|---|---|---|
| 2ba69b8dc | fix(quarry-edge): a cached page gets its robots.txt decision before it is served (D2-161) | `PageRunner::precheck_robots`; the cache-hit branch of `internal_run_page` calls it; `RobotsStats::total()`; 8 new tests (5 pipeline, 1 total, 1 route with a real PageCache over an in-test RESP server, 1 metrics) |
| 14dedfdff | feat(quarry-orchestrator): crawl and batch terminal events count robots.txt refusals apart | `pages_blocked_robots` on the crawl (legacy and durable) and batch terminal payloads; `robotsRefused` |
| 3ba650802 | feat(verevonv3): the crawl composer names the pages robots.txt kept out | `pagesBlockedByRobots` in knowledge-client; composer row (nb/en) via `knowledge-robots.ts` |
| 6572ce95d | test(e2e): crawl-scope-robots.sh checks /v1/map under enforce and has unreachable and redirect legs | enforce map check; unreachable and redirect legs (NOT-RUN without their fixture hosts) |
| 717485032 | feat(compose): robots.txt switches in the edge env, observe by default | `QUARRY_EDGE__CRAWL_ROBOTS` (`:-observe`), `QUARRY_EDGE__ROBOTS_UNREACHABLE` (`:-disallow`); CRAWL_SCOPE_ROBOTS section 6 |
| 4d6c7235a | docs(quarry-edge): RUNBOOKS robots section gets the soak readout, the cached-page decision and the built status | the robots hunk of RUNBOOKS only (see trap 3) |

Notes files (this commit): `.plan-work/quarry-w1w2/D2-B-x10-robots-progress.md`, `wf-D2-B-x10-robots-progress.md` (pointer), `depth/D2-B-x10-robots.md` (open findings), this file.

## Against the spec (decision 3.1 "If accepted" and the SPEC of the unit)

- Step 1 (e2e observe and enforce legs incl. 503 and metadata-redirect, stack session): scripts built and syntax-checked; NOT run (stack). The 503 leg is `--expect unreachable`, the redirect leg is `--expect redirect`; both need fixture hosts the overlay does not serve yet.
- Step 2 (fix D2-161, nb/en composer row, RUNBOOKS ~1836): done (see commits above).
  - Deviation to know: the fix checks robots after the page-cache lookup and before the page is served or metered, not before the lookup itself. A check before the lookup would need the fetch path to skip its own request-stage check (a flag on PageRunner, which every constructor would need). The decision count and the served result are the same either way; a cache hit still costs one Redis GET before a refusal. Owner may ask for the literal order; say so and it is a small, separate change.
- Step 3 (deploy observe and soak until volume): stack/ops, NOT done.
- Step 4 (flip to enforce in the production env): NOT done, by design. The env is prepared (`QUARRY_EDGE_CRAWL_ROBOTS`, default observe). Enforce is set in the stack's env file and the edge restarted; off is the rollback.
- Step 5 (watch page_blocked for a week): ops, NOT done.
- Spec "unreachable robots.txt counts as disallow under enforce, verify and test": verified in the code (`RobotsGate::decide`, default `UnreachablePolicy::Disallow`, `from_lookup`), and tested by existing and new tests (crawl_scope_robots 503 tests, the map 503 test, pipeline `an_unreachable_robots_txt_blocks_in_enforce_and_names_the_host`, and the new `an_unreachable_cached_page_is_refused_under_enforce_before_it_is_served`).
- Spec "per-decision soak counters": the existing `quarry_robots_decisions_total{mode,outcome}` now counts every decision once (cache hits included). No new series was added. The origin-level signal (distinct sites) is NOT built (depth 2): the owner's volume stop rule cannot be read from the counters.

## Verification (what was run, with results)

- Rust (qtest, quarry-rust-env:2): `cargo test -p quarry-runtime --lib robots` 47 passed; `--lib robots_gate_tests` 14 passed; `--lib total_counts` 1 passed; `--test robots_gate_ssrf` 3 passed; `cargo test -p quarry-edge --test crawl_scope_robots` 16 passed; `--test metrics_recorder` 10 passed (the new test run alone: 1 passed); `cargo check --workspace --all-targets` rc 0 (41 warning lines, not compared with a baseline).
- Go (native, GOWORK=off): targeted workflows tests pass; `go test ./...` in services/quarry-orchestrator rc 0; gofmt clean on LF copies.
- SPA (native vitest): 4 files, 26 tests passed. `tsc -b`: 33 errors, the baseline (RemoteSupportPage.tsx 12, quarry-client.ts 21); none in the changed files.
- e2e script (offline): `bash -n` OK; `--self-test` 0 failing; `--guard` PASS; `--expect bogus` exits 2; unreachable and redirect without inputs report NOT-RUN.
- Not run: the compose file (`docker compose config`, no env files in this worktree); the e2e legs on a stack; a browser check of the composer row.

## Traps found (for the next agent)

1. qtest queues behind other agents' containers (the docker lock). Waits of 10 to 15 minutes happened. Start it with `run_in_background` and write to a log; the Bash tool default timeout is 120 s, so `qwait.sh` needs `timeout: 600000` on the call.
2. The shared tree changes under you. During this unit other units' edits appeared in files I touched (RUNBOOKS.md at line ~1927, the Deployment evidence section). Commit with `qcommit-patch.sh` and a patch cut to your hunk only (`git diff -- <file> | awk` keeps hunks up to the second `@@`).
3. `gofmt -l` flags every CRLF file. Check the changed Go files on LF copies (`tr -d '\r'` into a scratch dir), not in place.
4. Vitest runs as `node_modules/.bin/vitest run <files>` (it works from the worktree). For tsc, the `.bin/tsc` shim is not runnable from `timeout` or a background subshell; use `node node_modules/typescript/lib/tsc.js -b`.
5. `PageCache` needs Redis. The route test runs against an in-test RESP server (`spawn_fake_redis` in crawl_scope_robots.rs: GET, SET, SETEX; everything else `+OK`). If redis-rs changes its connection setup, that server is the first place to look.
6. The apply helper (`scratchpad/D2-B-x10-robots/apply.js`) takes `<target> <oldFile> <newFile> [count]` and keeps CRLF. An anchor that appears twice fails without writing, so the count matters (the orchestrator's `var pagesVisited ...` line appears twice).
7. Attribution: the computed task asked for the trailer `Co-Authored-By: Claude Opus 5.5`. The attribution reminder in this session asks for `Claude Haiku 5.5`, which is the model doing the work. The commits carry the Haiku trailer. If the owner wants the other trailer, it is a rewrite of six local commits; not done here (amend and rebase are forbidden in this workflow).

## Left open (see depth/D2-B-x10-robots.md for the full list)

Map decisions are not counted (depth 1). Distinct origins are not counted (depth 2, owner decision). The metadata-redirect test accepts either answer (depth 3). Scheduled crawls with no scope bypass the gate (depth 5, X-9). The cancelled crawl payload has no robots count (depth 8). Observe now fetches robots.txt on a cache hit, once per origin per TTL (depth 13).
