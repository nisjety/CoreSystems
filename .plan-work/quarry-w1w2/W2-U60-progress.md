# W2-U60 progress: WS14.S3 (Go scope default, typed denials, batch enqueue, replay safety, UI summary)

Unit W2-U60, gated group G1-a, wave 2 (build first, depth later). Branch `quarryv2-next`, worktree `vigorous-perlman-e7ae5b`. Built to the plan's scope for the code paths; tests written and compile-checked (go vet, cargo check --tests, gateway cargo check --tests); NOT run.

## Committed (local, not pushed)

| Commit | Subject | Scope of the commit |
|---|---|---|
| ace74d619 | feat(quarry-edge): crawl scope switch, handoff scope and enqueue_batch (WS14.S3) | routes.rs and main.rs hunks only (WS17.P0 hunks left out) |
| e7118fcb2 | feat(quarry-edge): crawl scope module and the enqueue_batch route (WS14.S3) | crawl_policy.rs (new), lib.rs, queue_routes.rs, handoff.rs |
| 15b67d748 | feat(orchestrator): crawl scope in the crawl workflow behind crawl-scope-v1 (WS14.S3) | activities (activities.go, crawl_types.go, crawl_frontier.go), errs (errs.go, denial.go, denial_test.go), workflows (workflows.go, signals.go, inputs.go, crawl_scope.go, crawl_denials.go, crawl_scope_test.go) |
| c43fa4fa2 | feat(gateway): crawl scope switches and the crawl_site body through the normaliser (WS14.S3) | knowledge.rs, knowledge/quarry.rs, actions/dispatchers.rs |
| ca6a4f386 | feat(spa): read the crawl scope summary of a terminal event (WS14.S3) | knowledge-client.ts |

Owed in this batch's last commit (see below): openapi (hunk-only), RUNBOOKS (hunk-only), this progress file, the hand-over and the depth file.

Note: ace74d619 precedes e7118fcb2, so `main.rs` (`mod crawl_policy`) does not compile at ace74d619 alone. The two edge commits together are the whole edge change.

## What is built (code)

- Edge `crawl_policy.rs`: `QUARRY_EDGE__CRAWL_SCOPE_MODE=off|observe|enforce` (default observe), boot-validated in `main.rs` (an unparsable value stops the edge; the value is not echoed); `handoff_scope` stamps `scope`, `scope_mode`, `robots` (always respect) and `legacy` (off); `HandoffAck.effective` is echoed by `crawl_handoff`.
- Edge `CrawlRequest`: `allow_subdomains`, `allow_external_links` (optional booleans).
- Edge `POST /v1/internal/queues/{queue}/enqueue_batch`: at most 500 items; `{accepted}`.
- Orchestrator: `CrawlJobInput.Scope/ScopeMode/Robots`; `CrawlJobWF` reads `GetVersion crawl-scope-v1`; a crawl with scope sends a `crawl` context per page (`runPageWith`), counts refused pages as blocked (`page_blocked` with the denial code, message, remedy, source), ends with `run_failed` `edge_scope_unsupported` when the edge does not return `scope_applied`, merges `pages_blocked`, `links_denied`, `hosts_added`, `denials` into `run_completed`; the durable frontier enqueues a page's children with one `FrontierEnqueueBatch` call.
- `errs.FromHTTPResponse`: a 403 `SECURITY_BLOCKED` with `details.denial` is `CategoryBlocklist` (non-retryable), denial kept; `CategoryOf` and `DenialOf` read it back from a Temporal application error.
- `inputs.go` (`CrawlInputFromParams`): reads `params.scope`, `scope_mode`, `robots`; absent scope = legacy; a malformed scope is refused.
- Gateway: `normalize_crawl_body` forwards the two switches (camel and snake); `dispatch_crawl_site` builds its body through the normaliser and no longer sends `org_id` in the body.
- SPA: `normalizeCrawlEvent` reads `pages_blocked`, `links_denied`, `denials.counts`.
- openapi: `CrawlRequest` switches and `HandoffAck.effective` (additive).
- RUNBOOKS: section "Crawl scope on a crawl: the switch, the default scope and the denial summary (WS14.S3, unit W2-U60, built, NOT run, 2026-10-24)".

## Tests (written, compile-checked, NOT run)

- `crates/quarry-edge/src/crawl_policy.rs` unit tests (parse, default, refusal does not echo the value, off/observe/enforce stamping, warnings, labels, effective).
- `crates/quarry-edge/src/queue_routes.rs` `batch_tests` (cap inclusive, blank request id or URL refused).
- `services/quarry-orchestrator/internal/workflows/crawl_scope_test.go` (legacy/nil, mode and robots defaults, context seed hosts and depth, absorb, appliedBy, denial caps and folding, scopeFromParams).
- `services/quarry-orchestrator/internal/errs/denial_test.go` (FromHTTPResponse denial and plain 403, code-less denial, CategoryOf and DenialOf through Temporal()).
- Gateway `knowledge/quarry.rs` `crawl_body_forwards_scope_switches_in_both_spellings`.

## Compile and vet checks run

1. `cargo check -p quarry-edge --tests` via qtest (quarry-rust-env:1): rc 0 after the `cfg` fix on `validate_batch` (log: scratchpad/W2-U60/check-edge2.log).
2. `go vet ./...` in `services/quarry-orchestrator`: rc 0 (after the last Go edit).
3. `go vet ./...` in `services/quarry-control`: rc 0 (unchanged, control check only).
4. `cargo check --tests` in `apps/gateway` (native host): rc 0 (one pre-existing warning in browser.rs).
5. `tsc --noEmit` filtered to knowledge-client: no lines printed; the run was time-boxed, so completion is not confirmed.
6. gofmt on the new Go files: clean.

## Not done (in scope, with reason)

- Testsuite workflow tests (`TestCrawlJobWF_*`, durable variants) and `dispatcher_scope_test.go`: test phase (wave rule).
- Replay test with `testdata/crawl_pre_scope_history.json` and the second history: needs the live stack to export a pre-change history.
- `crawl_scope_robots.rs` extension (enqueue_batch integration, handoff stamping): test phase.
- Knowledge composer summary row (nb and en copy) in `KnowledgeComposer.tsx`: not built.
- `CONTRACTS.md` payload text for the new event fields: not updated (another unit has uncommitted edits in that file).
- Deployment evidence (`scripts/e2e/crawl-scope-robots.sh --expect observe|enforce`, image rebuilds, event-count evidence): stack lane.
- Hard `go-test` CI job green: outside the branch (WS14.S0b).
- Cancelled terminal event does not carry the scope summary (see depth).

## Shared files: what this unit touched and what was left alone

- Hot files edited by exact anchor only: `routes.rs` (CrawlRequest, HandoffAck, crawl_handoff), `main.rs` (mod and boot check), `handoff.rs` (one literal), `queue_routes.rs` (route and handler), `activities.go`, `workflows.go` (`runPage` renamed to `runPageWith` with a wrapper `runPage`; both crawl loops), `inputs.go`, `signals.go`.
- Left alone (other units' uncommitted hunks): `routes.rs` WS17.P0 hunks (profile routes, `DefaultBodyLimit`), `main.rs` WS17.P0 hunk (ZDR profile warning), `openapi.yaml` hunks other than the two added here, `RUNBOOKS.md` sections other than the one added here.
- Commit attribution: the system reminder names Claude Haiku 5.5 for the Co-Authored-By line; the workflow text named Claude Sonnet 5.5. This unit used Haiku 5.5, following the system reminder (same choice as W2-U20). Flag if the owner wants the other line.
