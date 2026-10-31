# D2-B-x10-robots: progress (decision 3.1, X-10, list B)

Status: COMMITTED (list B prepared and tested, flags off or observe). The stack steps (e2e runs, observe soak, enforce flip, one-week watch) are not done here, by design. Hand-over: wf-D2-B-x10-robots.md.

## Commits (local, quarryv2-next; not pushed)

- 2ba69b8dc fix(quarry-edge): a cached page gets its robots.txt decision before it is served (D2-161, X-10)
- 14dedfdff feat(quarry-orchestrator): crawl and batch terminal events count robots.txt refusals apart (WS14.S4, X-10)
- 3ba650802 feat(verevonv3): the crawl composer names the pages robots.txt kept out (WS14.S4, X-10)
- 6572ce95d test(e2e): crawl-scope-robots.sh checks /v1/map under enforce and has unreachable and redirect legs (WS14.S4, X-10)
- 717485032 feat(compose): robots.txt switches in the edge env, observe by default (WS14.S4, X-10 flip prepared)
- 4d6c7235a docs(quarry-edge): RUNBOOKS robots section gets the soak readout, the cached-page decision and the built status (WS14.S4, X-10)

Notes (committed with this file): wf-D2-B-x10-robots.md (hand-over), depth/D2-B-x10-robots.md (open findings), wf-D2-B-x10-robots-progress.md (continuation pointer).

## Verified (each run in this session)

- Rust, qtest: `cargo test -p quarry-runtime --lib robots` 47 passed; `--lib robots_gate_tests` 14 passed (5 new); `--lib total_counts` 1 passed; `--test robots_gate_ssrf` 3 passed; `cargo test -p quarry-edge --test crawl_scope_robots` 16 passed (new: a_cached_page_is_refused_under_enforce_and_counted_once_under_observe); `--test metrics_recorder` 10 passed (new: sampler_exports_each_robots_decision_under_its_mode_and_outcome); `cargo check --workspace --all-targets` rc 0.
- Go, native: targeted workflows tests pass; `go test ./...` in services/quarry-orchestrator rc 0.
- SPA, native vitest: 4 files, 26 tests passed. tsc -b: 33 errors (the baseline; none in changed files).
- e2e script, offline: bash -n OK; --self-test 0 failing; --guard PASS; NOT-RUN for unreachable and redirect without inputs.

## Left (not this unit's to do, or not possible here)

- Stack session: the e2e observe and enforce legs (and unreachable, redirect once their fixture hosts exist); `docker compose config` for the compose env; a browser check of the composer row.
- Owner: the volume-based observe soak, then the enforce flip (set QUARRY_EDGE_CRAWL_ROBOTS=enforce in the stack env file; off is the rollback).
- Open findings: depth/D2-B-x10-robots.md items 1 to 13.
