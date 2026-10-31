# W2-U20 progress: WS14.S2 (crawl filter and run_page crawl context)

Unit W2-U20, batch 4, wave 2 (build first, depth later). Branch `quarryv2-next`, worktree `vigorous-perlman-e7ae5b`. Built to scope, inert (no caller sends `crawl` until WS14.S3). Tests written and compile-checked; NOT run (final test phase).

## Committed (local, not pushed)

| Commit | Subject | Files |
|---|---|---|
| 9bee0f4d5 | feat(quarry-core): crawl scope contract and robots_unreachable denial | `quarry-core/src/crawl_scope.rs` (new), `crawl_denial.rs`, `lib.rs` |
| 1b4ff9384 | style(quarry-core): rustfmt the crawl scope module | `crawl_scope.rs` |
| af75d8b2a | feat(quarry-runtime): crawl scope evaluation for one page | `quarry-runtime/src/crawl_filter.rs` (new), `crawl_frontier.rs`, `lib.rs`, `Cargo.toml` (regex) |
| 5f5622d7f | feat(quarry-edge): crawl scope on POST /v1/internal/run_page | `quarry-edge/src/routes.rs`, `tests/crawl_scope_robots.rs` (new) |
| fd2060174 | docs(quarry-v2): openapi crawl scope on run_page and the crawl report | `docs/openapi.yaml` (crawl hunks only) |
| d0d575755 | docs(quarry-v2): RUNBOOKS section for the run_page crawl scope | `docs/RUNBOOKS.md` (see incident) |
| 32d954c74 | fix(quarry-v2): keep the CRLF lines of the WS9.C RUNBOOKS section | superseded, see incident |
| ae4c9aa61 | fix(quarry-v2): restore the RUNBOOKS sections dropped by 32d954c74 | superseded, see incident |
| 1e2362019 | fix(quarry-v2): RUNBOOKS, keep both WS13.1 and W2-U21 sections | `docs/RUNBOOKS.md` (final state) |

Plan-work notes (depth bullets, progress, hand-over) are committed in the last commit of this unit (see `git log --oneline` for `docs(plan-work): W2-U20`).

## Built (what the code does)

- `quarry-core::crawl_scope`: wire types `CrawlContext`, `CrawlScope`, `CrawlKind`, `ScopeMode`, `RobotsMode`, `CrawlReport`, `DenialCounts`, `DenialSample`, `RobotsDenial`, `CrawlFieldError`; caps (`MAX_PATTERNS`=100, `MAX_PATTERN_CHARS`=512, `MAX_PATTERN_TOTAL_CHARS`=8192, `MAX_LINKS_EVALUATED`=5000, `MAX_SEED_HOSTS`=100, `MAX_EXTENSIONS`=100, samples 3 per code and 20 per page, robots list 50); `CrawlContext::from_json` and `validate` (field paths such as `crawl.scope.include_paths[3]`).
- `quarry-core::crawl_denial`: `RobotsUnreachable { host }` (code `robots_unreachable`), and `remedy()`.
- `quarry-runtime::crawl_filter`: `CompiledScope::compile` (regex size 256 KiB, DFA 1 MiB), `CompiledScope::evaluate` (fixed check order, enforce and observe, batch), `host_matches` (the shared host rule), `DEFAULT_DENY_EXTENSIONS`, `RobotsPeek`/`RobotsLookup`.
- `quarry-runtime::crawl_frontier`: `host_in_scope` now calls `host_matches(.., true)`; `normalize_url` and `glob_match` are `pub(crate)`.
- `quarry-edge` `POST /v1/internal/run_page`: optional `crawl` (kept as `serde_json::Value`, compiled before the fetch; malformed is a 400 with `details.fields[] {path, code, message}`); `links` and `crawl` report attached at both return sites through `with_crawl`; no-`crawl` responses are unchanged (the `crawl` field is skipped when absent).
- Flag: none (inert). Migration: none (U20 owns none). Golden: none recorded (see depth).

## Tests (written, compile-checked with `cargo check --tests`, NOT run)

- `quarry-core` `crawl_scope::tests` (11): defaults, empty object, unknown key refused, version, seed hosts, seed URL, pattern count and length and total caps, extension entries, sample caps, robots caps, robots unreachable.
- `quarry-core` `crawl_denial::tests`: unique codes (list extended), robots_unreachable code and message, remedy presence.
- `quarry-runtime` `crawl_filter::tests` (about 40): same host and www both ways, trailing dot and case, IDN, userinfo trick, port, subdomains on and off, external on and off, depth boundary, non-web schemes, fragment-only and fragment collapse, extension denylist, explicit extension list, directory prefix, include miss, exclude hit, exclude beats include, grammar prefixes, duplicate collapse, robots caps and unreachable and ignore, 6000-link cap, invalid regex field, oversize regex (weak), catastrophic regex under 250 ms (debug), redirect host at depth 0 only, observe, batch, pattern list refusal, non-host inputs; proptest `frontier_enqueue_and_host_matches_agree`.
- `quarry-edge` `tests/crawl_scope_robots.rs` (6): no crawl, enforce on the standard fixture, observe, malformed 400, bad regex index, batch. Uses the signed run binding.
- Named by the queue row U20: `crawl_filter` unit tests (same host, www both ways, IDN, userinfo) and `crawl_denial` unique-code tests: written.

## Compile checks run (each rc 0, quarry-rust-env:2, through qtest)

1. `cargo check -p quarry-core -p quarry-runtime -p quarry-edge --tests` before the rustfmt pass: rc 0 for all three.
2. Warnings grep for my files: none reported.
3. Final: `cargo check -p quarry-core --tests`, `-p quarry-runtime --tests`, `-p quarry-edge --tests` after the rustfmt pass and the edge-test hand edits: rc 0 for all three (`/tmp` logs, no errors, no crawl-related warnings).
4. rustfmt: applied in place to `crawl_scope.rs` and `crawl_filter.rs`; the edge test was formatted by hand (its `mod support` would make rustfmt reformat another unit's support files). The rustfmt run also reports formatting drift in `tests/support/fixture_site.rs` and in `routes.rs` outside my lines; not touched.
5. YAML: no YAML parser on the host (system Python has no PyYAML, no node_modules yaml). The openapi insertions were checked by reading the surrounding lines (`InternalRunPage` and `InternalRunPageResult` properties, and the new schemas before `InternalRunPageResult`). Not parsed.

## Not done (in scope, with reason)

- Test runs: all of the above, per the wave rule. Final test phase: `cd "apps/Ingestion Plane/Quarry-v2" && cargo test -p quarry-core crawl_ && cargo test -p quarry-runtime crawl_ && cargo test -p quarry-edge --test crawl_scope_robots`, plus the frontier tests (the www rule changes `host_in_scope`).
- Golden of the no-crawl body recorded from the pre-U20 handler (plan: "golden test proves no change without crawl"). Not recorded; the edge test checks absence of the key only.
- Robots peek wiring: `run_page` passes `None`; robots denials do not fire until WS14.S4 (U63).
- Pattern-level differential between `CrawlFrontier::enqueue` and `evaluate` (proptest covers hosts only).
- Deployment evidence (rebuild the edge image, overlay, signed calls, jq assertions). Stack lane.
- The orchestrator side (WS14.S3) is not started; it is the first caller of `crawl`.

## Incident: RUNBOOKS.md history (fixed by 1e2362019; disclosed)

While committing the RUNBOOKS section, my first attempt built the file from a HEAD that did not include the work of other units, and the first repair also started from an older base. Between my commits another unit (W2-U21, 2eae5f6be) committed on top of the broken state. Result of the sequence:
- `32d954c74` (my "fix") was built on `199961724` but from an older copy of RUNBOOKS, so it dropped the WS13.1 section (`a07c96b67`). Its commit message claims it only restored CRLF bytes, which is not what it did.
- `2eae5f6be` (W2-U21, parent `32d954c74`) dropped the WS13.1 section again and added its budgets section.
- `ae4c9aa61` (my repair) went back to `199961724`, which dropped the budgets section.
- `1e2362019` (final) has `199961724` plus the W2-U21 section, with the WS9.C devtools CRLF bytes restored. Verified: its diff against `2eae5f6be` adds only the WS13.1 section; against `199961724` it adds the W2-U21 section and restores 8 CR bytes.
- History was not rewritten (no amend, reset or rebase were used). The intermediate commits `32d954c74` and `ae4c9aa61` remain in the branch history and are wrong as to RUNBOOKS content. Anyone who reads RUNBOOKS at those commits will see the missing sections. A later rebase or squash by the owner would clean that up; I did not do it.
- Root cause: `git hash-object` (via the clean filter, `core.autocrlf=true`) converts CRLF to LF, which is how the WS9.C lines became LF in `d0d575755`; and my first two commit helpers took HEAD and the file at a different moment from the working copy. The helper I use now (`commit-head.sh`, held under the git lock, temporary index, `hash-object --no-filters`) avoids both.
- Also in this unit's messages: `fd2060174` says 381 insertions (the true count is 211; 381 counted another unit's devtools hunk), and `d0d575755` says 14 lines (it is 22 insertions and 8 deletions of EOL). Corrections are in the `1e2362019` message.

## Hand-over

- Other units' uncommitted files left alone: `crates/quarry-core/src/{contracts,envelope,event,policy}.rs`, `crates/quarry-core/tests/contracts.rs`, `crates/quarry-runtime/src/{browser_driver,browser_procedure,nats_event_bus,postgres_step_receipts,step_receipts}.rs`, `docs/CONTRACTS.md`, and the working-copy RUNBOOKS and openapi hunks of other units. My commits carry only my hunks (checked with `git diff --numstat` before each commit).
- Commit-message attribution: the system reminder names Claude Haiku 5.5 for the Co-Authored-By line; the workflow text asked for Claude Sonnet 5.5. I used Haiku 5.5, following the system reminder. Flag if the repo owner wants the other line.
- Next batch (WS14.S3, U-next) needs: `CrawlContext` (send `scope`, `scope_mode`, `robots`, `seed_hosts`, `seed_url`, `depth`, `max_depth` from the workflow), the `crawl` field in `activities.RunPageInput`, and the observe-first default. Read `.plan-work/quarry-w1w2/depth/W2-U20.md` for the decisions it must take (seed_url, redirect hosts, extension list, plain-prefix semantics).
