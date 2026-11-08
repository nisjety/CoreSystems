# W23 orchestration (Quarry Wave 3, E3.1 to E3.9, and Model Plane status sync)

Written 2026-11-07 by the orchestration subagent on branch `w23/integ` (worktree `.claude/worktrees/innh-lane-integ`, HEAD `03d0e0a76` when read, `ba15d31dc` when committed, see section 0). Only this file is committed. No code was written, nothing was pushed, merged, deployed or flag-flipped.

Sources read: `docs/plans/quarry-browser-build-plan-2026-10-14.md` (status block, SC table 1.2, section 3.3 E3.1 to E3.9), `W23-STATUS.md` (all batch rows through the 2026-11-04 host test gate), `W23-FINISH-QUEUE.md` (W3-B4 to W3-C4 and the decisions list), `MP-W2W3-STATUS.md`, `MP-ASKS.md`, `stack-2026-10-28/live/{REVIEW.md, rerun-2026-10-29/*, sc1-2026-10-29/SC1-RESULTS.md}` (on main), `apps/Model Plane/docs/PLAN.md`, `apps/Model Plane/docs/decisions/ledger.md` and `docs/plans/wave3/*` on `model-plane`.

## 0. Branch facts that change the plan

| Fact | Evidence | Consequence |
|---|---|---|
| `w23/integ` is fully contained in `main` | `git rev-list --count main..w23/integ` = 0; `main..` merged via 4712243ef (main 03d0e0a76 into model-plane) and then into main | Nothing on integ is waiting to merge. W3 "completion on w23/integ" means new slices only. |
| `main` is 148 commits ahead of `w23/integ` | `git rev-list --count w23/integ..main` = 148 | main changed W3 hot files after integ: `quarry-runtime/src/observation.rs` (+222), `fetch.rs` (+152), `browser_egress_proxy.rs`, `agent_routes/{live,tab_ledger,step,router,types,ws}.rs`, new `quarry-edge/src/browser_redirect.rs`, `zdr-feature-matrix.json` (+16), broker `grant.go`, execution-core `quarry_agent.rs` (+388). A slice cut from `03d0e0a76` will conflict. |
| `w23f/lane-a..d` are 0 ahead of integ; `w23/lane-a..d` and `w23/wip-u56` are WIP snapshots | each 1 commit ahead; recorded in main with the "ours" merge 3bb5c96db | Do not merge the WIP snapshots; their work is in main in its finished form. |
| `model-plane` (HEAD 2af78fea2, 2026-11-07) is 18 ahead of main; `mp-ep14-s1-gap` is 1 ahead of its base | rev-list | Model Plane lands EP-x S1 slices daily; the `epic-shaw-4d613b` worktree has 112 uncommitted entries (an active MP session). Do not write Model Plane files from here. |
| integ working tree: one line-ending-only artifact (`quarry-core/contracts/web-policy-catalogue.v1.json.sha256`) | `git status` | Not staged, not committed (same rule as every batch row). |
| **A second writer is active in this worktree.** While this file was written, another session committed the E3.5-4 docs directly on `w23/integ` on top of `03d0e0a76`, not on main: first 67db05e01, which rewrote `docs/RUNBOOKS.md` and `CHANGE_TRACKING.md` from LF to CRLF in the blob (4829 and 310 diff lines for 21 and 4 real lines), then amended to ba15d31dc (12:0x), which is clean (49 insertions, 2 deletions, same with or without `--ignore-cr-at-eol`) | `git log`, the index lock seen twice during this pass | integ now diverges from main (1 ahead, 148 behind), so gate O0 can no longer be a fast-forward. The EOL problem is fixed in ba15d31dc. That writer owns `RUNBOOKS.md`, `CHANGE_TRACKING.md`, `openapi-internal.yaml` and `scripts/tests/docs-contract.test.mjs` until it stops; CW-9 waits for it. |

**Gate O0 (needs the parent or owner, because this subagent may not merge):** before any more W3 code, bring main into `w23/integ`. While integ was an ancestor of main this was `merge --ff-only main`; after ba15d31dc it is a real merge (`git merge --no-ff main`; conflicts possible only in the four docs files above). Then reset `w23f/lane-a..d` to the new integ tip. Writers on integ should stop committing on the old base until O0 is done. Until O0, the code-writer slices below are planned, not started.

## 1. Wave 3 build vs proven

### 1.1 Unit ledger (Quarry side, W3 queue W3-B4 to W3-C4 = 58 units, plus B1 to B3 = 15 units)

| Measure | Count | % | Notes |
|---|---|---|---|
| Units landed on integ (and so on main) | 72 of 73 | 99 % | Not built: **W3-E3.2-g1** (gateway `browser_policy.rs`, `browser.get_org_policy` / `browser.set_org_policy`; no file, no commit on any branch). Partial: **W3-E3.8.4d** (log canary, Redis behavioural tests, crawl `max_age` key missing). |
| Queue units reachable end to end with their flag on | about 17 of 58 | about 30 % (estimate) | Read from each unit's open items in W23-STATUS.md: most units are built dark with no caller, no trait method, no route or no wiring (list in 1.3). |
| Units whose code ran in a unit-test gate | about 39 of 58 | about 67 % | 2026-11-04 host gate covered the Quarry Rust crates (quarry-core 416, security 40, transform 310, browser 264, tls 26, runtime 1452/10 platform-only, edge 2858/14 platform-only, evals 92) and Go control/orchestrator. **Not run for W3:** verevonv3 vitest (only tsc), gateway `cargo test` (only `cargo check`), auth-core jest (E3.2-f, E3.7.10-CP), integration-corev2 (E3.1-g has no tests), user-core migration 044 (never applied), convex-core. |
| Units proven on a stack (L2, live dev or evidence) | 0 of 58 | 0 % | By plan decision ("the user said not to test W3"); W3 is to be user-tested after the merge. |
| Plan days (header of the build plan, W3 estimate 278 d) | B1 to B3 = 38.5 d recorded in the plan header; the header was not updated for B4 to C4 | n/a | The build plan status block still says "B4 and later not built". Stale; see 4.3. |

Model Plane halves of W3 (14 rows in MP-W2W3-STATUS.md, all on main): built and compile-checked, a subset of named tests ran; flags off; 0 stack-proven.

### 1.2 SC1 to SC7 (the W1/W2 criteria Wave 3 rests on)

Last evidence: live dev stack, 2026-10-28/29, dev stub identity unless stated, images older than every W2-finish and W3 commit. Nothing has been re-run on an image of `w23/integ` or current main. None is PASS at its L2 evidence type (production overlay).

| SC | Status | Evidence | What closes it |
|---|---|---|---|
| SC1 prod-mode browser path | PARTIAL | page legs A, A.6, B, C PASS on live dev after the `mp-browser-cap` fix (SC1-RESULTS.md, 18:47Z); host log scan PASS | wrapper legs A1-A3, A5a-c, A6; A.7 (second account); restart check (TTL >= 300 s); the run on the production overlay plus contract overlay |
| SC2a authority | PARTIAL | raw script denied PASS; takeover with a user token labelled agent only on the dev stub | the [USER] token leg |
| SC2b MP run pauses (W2) | NOT-RUN | none | a Model Plane run on the stack |
| SC3 no leaked runs | PARTIAL | caps, sweeper, idle expiry, checkpoint, close-with-viewer PASS (backstop shortened to 150 s); RSS 8 and 16 | RSS at 32 contexts with `QUARRY_BROWSER_MAX_SESSIONS=48` |
| SC4 driver stability | PASS on live dev (after R-SC4 to R4-SC4) | first run FAIL (T1, T3); re-runs PASS T1, T3a/b, T2, T5, D1-D3 (R4-SC4.md); sandbox on PASS; viewport probe PASS (frame-size claim unproven) | repeat on the production overlay with current images |
| SC5 ZDR and tenant boundary | PARTIAL | one-org ZDR scrape/stream/agent inline, 0 artifacts, 0 log content PASS | cross-org search cache and PageCache `prev_fingerprint` probes, gateway/orchestrator canary, purge/forensics |
| SC6 strict scrape contract | PARTIAL | PDF/DOCX inline, PNG/ZIP 415, `DOC_ROUTING=on` PASS | `GET /v1/scrape/{id}` from a second org |
| SC7 budgets stop work | W1 clause PASS; W2 remainder FAIL/OPEN | crawl default 1000 with `clamped_by:["default"]` PASS; X-12 overall FAIL (no `agent.budget_exhausted` on SSE, cost bound with `on`) | X-12 SSE fix, budget exhaustion on the stack, billing metric codes confirmed |

Fully proven: 0 of 7. Partly proven: 6 of 7 (SC2b not run).

### 1.3 Built but not reachable (the real W3 completion work), by hot file

| Chain | Missing piece | Severity |
|---|---|---|
| E3.6 PII | crawl does not refuse `redact_pii` (`crawl_scrape.rs` UNSUPPORTED_KEYS): crawl runs unredacted in lenient mode | HIGH |
| E3.6 PII | per-request redacting runs still render a page PNG into the CAS (`pipeline.rs`, D-E364b-1) | HIGH |
| E3.6 PII | `routes.rs` calls `run_with_inline_request`, so per-request `redact_pii` never reaches the stage (D-E364a-2); scanner panic hook can print page text (D-E364a-1) | medium |
| E3.1 OTP | runtime stores screenshot/visual artifacts and emits ObservationReady before the edge strips (`observation.rs`); `attach.rs` delegate_step skips the read-denial check | HIGH x2 |
| E3.1 OTP | OTP methods are inherent `ChromiumoxideDriver` methods, unreachable through `Arc<dyn BrowserDriver>` (D-E31c-1); nothing constructs `IntegrationCoreOtpSource`; IC route lacks the verified caller; `admission.rs` lacks FillOtpRef; broker `fill_otp` (MP ask) | HIGH (chain inert, so no exposure yet) |
| E3.9 evals | Browserless token in a URL (D-E3967-1) | HIGH |
| E3.3-h / e1 / k | hit-test route answers 501 until a trait method plus `BrowserPointHit` land in `quarry-browser/src/lib.rs`; public `fingerprint_id` exposes the backend node id (D-E33h-1); rect never produced (needs `target_box` and `DOM.getBoxModel`); AX warnings dropped at the trait boundary | medium |
| E3.4 sessions | `types.rs` `run_is_resumable` treats only `active` as resumable (blocks `QUARRY_EDGE__RUN_TTL`); `close.rs` drain does not mark suspended; refused downloads stay in artifact storage; download ledger not released; delegate step path ungated | medium |
| E3.5 webhooks | `resources.go` change_detected batch path fans out unfiltered (must be fixed before any notifyOnChange filter is enabled); crawl handoff does not save `req.webhook`; control create does not call NormalizeJobWebhook | medium |
| E3.7 | Idempotency-Key not covered by the internal HMAC (D-E371-1); search page fetcher follows redirects without re-checking hosts (W23-b-E377a-1, shared with `/v1/answer`); `search_routes.rs` does not pass prefetched pages to E3.7.7b; nothing sets `max_parallel`; crawl approval settings read from env, SPA prompt not wired to the signal | medium/low |
| E3.8 | no producer names dns/tls/connect (E3.8.1), no production caller passes FetchPolicy (E3.8.2), `tier_stalled`/`fetch_deadline_exceeded` not in `error.rs` + Go mirror + ERROR_REASONS.md, stream/crawl paths not wrapped (E3.8.3), no caller of `with_reputation` (E3.8.7), `NoEgressDriver` unbuilt at the edge (E3.8.8, Data Plane contract decision) | low (dark) |
| Frontend / gateway arms | no gateway arm for `browser_run.steer` (E3.3-g) or `browser_download.send_to_knowledge` (E3.4-7); nothing mounts the OTP card, the downloads drawer or `BrowserPolicySection` (waits for E3.2-g1); chat turn path does not call `resolve_turn_tab_mentions` | low (dark) |
| Docs and flags | none of the W3 flags are in compose or RUNBOOKS; node test failures: openapi-internal (W2-U49 route), tracked internal SDK files, route coverage (`/v1/agent/runs/{}/hit-test`, `/v1/audit/page`, user receipts), pinned route count | low |
| Decisions | execution-core's quarry token is ZDR and the edge refuses durable jobs under ZDR, so `crawl:write` can never start a job (E3.7.10-CP); `QUARRY_EDGE__SCHEDULE_POSTURE` default observe vs off; OD13 counsel before ORIGIN_CONSENT | owner |

## 2. Slice assignments (one writer per hot file)

Start condition for every CW slice: gate O0 done, lane reset to the new integ tip. Each CW slice ends with the compile guard of W23-STATUS.md plus the named tests of its files; the verifier then runs VF-1 on it before the next slice that shares a file starts.

### 2.1 Code-writer (CW)

| Slice | Lane | Exclusive files this round | Items | Order constraint |
|---|---|---|---|---|
| CW-1 PII fail-closed | a | `quarry-runtime/src/pipeline.rs`, `quarry-edge/src/crawl_scrape.rs`, `quarry-edge/src/routes.rs`, `quarry-runtime/src/ingest_client.rs` | crawl refuses `redact_pii`; no PNG/CAS render while redacting (wrap in PiiEnforcingStore); routes pass per-request `redact_pii`; panic-hook text; ingest sink check | first (HIGH) |
| CW-2 OTP read denial | b | `quarry-runtime/src/observation.rs`, `quarry-edge/src/agent_routes/attach.rs`, `quarry-edge/src/agent_routes/step.rs`, `quarry-runtime/src/observation_redaction.rs` | strip before store and before ObservationReady; delegate_step check; release marks on close and sweep | first (HIGH); rebased on main's +222 lines in `observation.rs` |
| CW-3 eval credential | c | `lab/evals/src/release_matrix.rs`, `.github/workflows/quarry-v2-eval-tier-c.yml` | Browserless token out of the URL (header or env), no secret value in logs | first (HIGH) |
| CW-4 E3.2-g1 + gateway arms | d | `src/shared/actions/action-registry.ts`, gateway `domains/browser_policy.rs` [NEW], `domains/actions/handlers.rs`, `domains/actions/dispatchers.rs`, SPA `agent-action-admission.json` (canonical) | build E3.2-g1 as queued; add arms for `browser_run.steer` and `browser_download.send_to_knowledge`; regenerate the admission golden once | the four Model Plane copies, the `.sha256` pin and the zdr-contract-sync line are rewritten by the integrator in one commit after rebasing on model-plane's current copy (the MP lead also regenerated it in W3-25, 1cbdae621) |
| CW-5 Chromium lane (serial) | a (after CW-1) | `quarry-browser/src/lib.rs`, `quarry-browser/src/chromiumoxide.rs` (sole writer) | E3.1-c OTP methods onto `BrowserDriver` → E3.3-h `hit_test_point` + `BrowserPointHit` + fingerprint accessor (fix D-E33h-1) → E3.3-e1 `target_box` → E3.3-k warnings through the trait | strictly in that order, one commit each |
| CW-6 run lifecycle | b (after CW-2) | `quarry-edge/src/agent_routes/types.rs`, `close.rs`, `run_registry.rs`, `run_sweeper.rs`; `step.rs` only after CW-2 lands | `run_is_resumable` widened; drain marks suspended; refused-download delete and ledger release; gate the delegate step path | after CW-2 (shares `step.rs`) |
| CW-7 control webhooks + idempotency | c (after CW-3) | `quarry-control/internal/resources/{resources.go,webhooks.go,change_webhook.go}`, `quarry-edge/src/handoff.rs` | batch fan-out filter; NormalizeJobWebhook on create; Idempotency-Key inside the HMAC; crawl handoff saves `req.webhook` (the `routes.rs` hunk waits for CW-1) | after CW-1 for the routes.rs hunk |
| CW-8 fetch reliability | d (after CW-4) | `quarry-runtime/src/{fetch.rs,fallback_driver.rs,policy.rs,retry.rs}`, `quarry-core/src/error.rs`, orchestrator `internal/errs/`, `docs/ERROR_REASONS.md` | E3.8.1 producers; E3.8.2 FetchPolicy caller; new reasons mirrored; E3.8.3 stream/crawl wrap (routes.rs after CW-1); E3.8.7 `with_reputation` caller | after CW-1 for routes.rs; rebased on main's +152 in `fetch.rs` |
| CW-9 docs, compose, contracts | any (last) | `Quarry-v2/docs/{openapi.yaml,openapi-internal.yaml,RUNBOOKS.md,CONTRACTS.md}`, `deploy/compose/*` (append-only anchors), `scripts/openapi-route-allowlist.json` | every W3 flag in compose and RUNBOOKS (defaults off); fix the 5 known node failures; SDK regen only if the generator image is available | last, after all routes settle |

Not assigned (decision or other plane): E3.8.8 edge wiring (Data Plane `POST /v1/artifacts/lookup` contract), E3.6-6a office-extract manifest row (Control R-13), E3.2-e1/e2 route callers (Model Plane attach driver ask), E3.7.10-CP ZDR-token decision, ORIGIN_CONSENT (OD13 counsel), E3.8.4d remainder (after CW-8).

Append-only files for every slice: `MP-ASKS.md`, `W23-depth.md`, compose anchors. Conflict rule as before: keep both sides, integ first.

### 2.2 Verifier (VF)

| Slice | When | What | Records |
|---|---|---|---|
| VF-0 | now, no base change needed (run against main HEAD in a throwaway worktree) | re-run the 2026-11-04 host gate on main (main changed Quarry since); the W3 tests never run: verevonv3 vitest for BrowserPolicySection, approval card and OTP notice, DownloadsDrawer, steer-input, crawl-site-approval, region-attach, tab mentions, browser dock; gateway `cargo test` (`RUST_MIN_STACK=16777216`); auth-core jest (web-policy, plane-service-principals-policy.contract, quarry-token-zdr-claims golden); integration-corev2 `go test ./internal/actions/...`; user-core `go test` plus migration 044 on scratch Postgres; convex-core `npx tsx --test` for browserActivity | per unit: unit-proven yes/no; moves the 39/58 figure |
| VF-1 | after each CW slice | compile guard, the slice's named tests, a security review row in W23-depth.md for HIGH items | W23-STATUS.md batch row |
| VF-2 | with the user, on images built from main HEAD | SC1 wrapper legs, A.7, restart, production overlay; SC2a [USER] token; SC2b; SC3 RSS 32; SC5 cross-org; SC6 second org; SC7 W2 remainder after the X-12 SSE fix | SC table in 1.2 |
| VF-3 | after VF-2 | W3 flag-on evidence per epic (plan 3.3 "evidence" lines): E3.5 two-org webhook, E3.6 PII fixture crawl, E3.7 50-URL batch, E3.4 restart-resume and download-to-Data-Plane, E3.1 `otp-login-fixture.sh` | stack-proven count |

## 3. Blockers

1. **Gate O0**: integ is 148 commits behind main and, since ba15d31dc, 1 ahead; no CW slice should start until main is merged into integ (orchestrator may not merge).
2. **W3-E3.2-g1 not built**: keeps the Settings > Browser section (E3.2-g2) hidden.
3. **Three HIGH security gaps on the redaction/credential path** (CW-1, CW-2, CW-3); the OTP chain is inert, so the E3.1 gaps are latent, but the PII gaps apply as soon as `QUARRY_EDGE__PII_REDACTION` is turned on.
4. **Model Plane asks still open** (section 4.2): broker `fill_otp`, execution-core `resume_run_id`, broker WS7.K web-policy snapshot, nats.conf `mp.v1.trace.run.*` subscribe for application-convex-model, attach per-tab consent, `fill_otp` approval fields, `browser_run.steer_taken`.
5. **Stack evidence needs the user** (second account, [USER] token, production overlay); SC1 to SC7: 0 of 7 fully proven, nothing re-run on current images.
6. **Owner decisions**: ZDR quarry token vs durable jobs (E3.7.10-CP), SCHEDULE_POSTURE default, OD13 counsel, Data Plane artifacts lookup contract (E3.8.8).
7. **Admission golden has two writers** (Quarry lead and MP lead): handled in CW-4 by a single integrator commit.

## 4. Model Plane status (sync)

### 4.1 Where Model Plane is

- Branch `model-plane` HEAD 2af78fea2 (2026-11-07), 18 commits ahead of main; main holds all of `mp-w2w3` (177 commits, merged 2026-11-03) and `w23/integ`.
- Model Plane's own Wave 3 is EP-1 to EP-14 (`docs/plans/wave3/`). Wave-mode batches W3-09 to W3-25 are integrated (last W3-25, 2026-11-05). The owner took all 105 open Wave 3 decisions at their recommended options (ledger 2026-11-04). On 2026-11-07, S1 slices landed for EP-1 (browser takeover, give-back continue caller), EP-2 (web reading, `find_in_page max_passages`), EP-3 S4, EP-5, EP-6, EP-7, EP-8 S2, EP-9, EP-10, EP-11, EP-13; EP-14 S1 is on `mp-ep14-s1-gap` (1 commit, not on model-plane). Everything is dark (flags off), compile-checked or unit-tested, not deployed.
- Quarry-facing Model Plane rows: W2 rows built and tested in process (scratch Postgres where needed); 14 W3 rows built and compile-checked (MP-W2W3-STATUS.md). Final mp-w2w3 regression (2026-11-02): execution-core lib 1763/2 (1 flake, 1 fixed), model-gateway lib 2650/1 known, session-core 705/0, Go modules green.
- Live: browser broker redeployed 2026-10-20; the `mp-browser-cap` fix deployed to live dev 2026-10-29 for SC1 leg C. Nothing else from these waves is deployed.

### 4.2 MP-ASKS.md (Quarry, Frontend and Control asks to Model Plane), checked against main and model-plane

| Ask | Status | Check |
|---|---|---|
| W2-FE-L5a admission golden re-vendor | CLOSED | SPA and `apps/Model Plane/api/contracts` copies have the same blob (d5632bd7) on main and model-plane |
| W2-CP-WS7I + W3-E3.2-f quarry token claims golden | CLOSED | model-gateway copy pin = auth-core pin = 6c3044d5 on main and model-plane |
| W2-CP-INNH10B `api_key_revoked` consumer | CLOSED | MP-2 (e52521b81 and following); broker has the consumer |
| W2-CP-INNH10A / W2-FE-GW1 key-token shape, key-grant door | CLOSED (built dark) | MP-1 0beba0eb1, MP-3 871c94ab3 |
| W2-U39b RenewGrant for the sweeper | MP side CLOSED; Quarry renewer wiring dark behind `QUARRY_EDGE__HUMAN_GRANT_RENEW` | broker RenewGrant present |
| W3-E3.2-e1 broker origin-extension grant kind | CLOSED (dark) | 58b324cc9, `BROWSER_BROKER_ORIGIN_EXTENSION_ENABLED=0` |
| W3-E3.6-7 OCR and PII-model rows on the service lane | CLOSED (dark) | aa892fc8e, `MODEL_GATEWAY_AI_LAYOUT_SERVICE` / `_AI_PII_SERVICE` |
| W3-E3.2-b swap redirect text | CLOSED (dark) | ae278fee3, `QUARRY_BROWSER_AGENT_APPROVAL_REDIRECT=0`; SPA `APPROVAL_REDIRECT_ENABLED=false` can follow |
| W3-E3.7.10-CP execution-core job tools | CLOSED (dark) | 47b0994dc; Quarry still refuses service principals on job routes; ZDR-token decision open |
| W2-DOC-A ledger entries | PARTIAL | DR-15.1 and WS8.A are in the ledger; the ADR-1 "one issuer" entry and the gateway.proto comment are not |
| W3-E3.3-g steer inbox | PARTIAL | steer at the step boundary built (E3.3-f 8ece1d33c); `browser_run.steer_taken` event absent |
| W3-E3.3-c / E3.3-d tab refs and region BrowserScraped | PARTIAL | E3.3-i guard test only; attachments are not trust-classified |
| W2-Q-WS7K broker web-policy snapshot (proto 20, ClaimPolicyResolver) | OPEN | no match in browser-broker |
| W2-AP-S7a nats.conf subscribe `mp.v1.trace.run.*` for application-convex-model | OPEN | only the producer's publish line exists |
| W3-E3.1-ab broker `fill_otp` sensitive action | OPEN | no match in broker `grant.go` |
| W3-E3.1-i `fill_otp` approval record fields | OPEN | no match in execution-core |
| W3-E3.4-2-3 execution-core `resume_run_id` | OPEN | no match in execution-core |
| W3-E3.2-e2 attach per-tab consent check | OPEN | no match in execution-core |

Model Plane asks to other planes (second half of MP-ASKS.md): unchanged by this pass; the Quarry ones (Tier B injection page, run_source producer, C5 rows in `zdr_matrix`, Quarry mark-tab route, `warnings[]`/`settle_ms` on the wire) map onto CW-5 (`warnings[]`) and CW-9 (docs).

### 4.3 PLAN.md, ledger and the build plan header are out of sync

- `apps/Model Plane/docs/PLAN.md` was last edited 2026-08-05 (0c2d43580) and still shows Phase 1 to 8 as ❌ and Phase 0 and 9 as 🟡 with "18 / 30 foundation gates". It does not reflect WS01-WS27 (Model Plane chat build plan), the Quarry W2/W3 Model Plane rows, or EP-1 to EP-14. Owner of the file: the Model Plane session (`model-plane` branch, worktree with 112 uncommitted entries), so it is not edited here (one writer per hot file). Proposed status block for that session to paste under "Current baseline":

  > Status 2026-11-07: phase markers below predate the 2026-10-14 chat build plan. Delivery is tracked in `docs/plans/model-plane-chat-build-plan-2026-10-14.md` (WS01-WS27, Waves 1-2), `docs/plans/wave3/` (EP-1 to EP-14; owner decisions taken 2026-11-04), the depth register `docs/plans/model-plane-depth-register.md` and `docs/decisions/ledger.md`. Quarry W2/W3 Model Plane rows: `.plan-work/quarry-w1w2/MP-W2W3-STATUS.md`. Everything after 2026-10-20 is dark (flags off) and not deployed except the browser broker (2026-10-20) and the `mp-browser-cap` fix on live dev (2026-10-29). Phases 1 (orchestration shell), 2 (capability platform), 3 (approvals), 4 (tasks/cron, WS20) and 6 (operator shell, EP-6 hubs) are partly built behind flags; 9 stays partial until the live proofs.

- Ledger: current on model-plane through 2026-11-07. Missing: the ADR-1 web-policy issuer entry asked by W2-DOC-A (text is in MP-ASKS.md lines 54-72).
- Build plan header (`docs/plans/quarry-browser-build-plan-2026-10-14.md` lines 18-25) still reads "W3 batches B1 to B3, 38.5 of 278, B4 and later not built". The proposed replacement row, for the next docs commit on main (not made here because the plan file is edited on main, not on integ):

  > | W3 (E3.1 to E3.9) | 72 of 73 queue units landed (B1-B3 plus W3-B4 to W3-C4; not built: E3.2-g1; partial: E3.8.4d); about 30 % reachable with flags on | unit gate 2026-11-04 on Quarry Rust and Go; Frontend, auth-core, integration-core, user-core and convex W3 tests not run; 0 units stack-proven; SC1-SC7: 0 fully PASS (SC4 PASS on live dev only) |

## 5. Next actions, in order

1. Parent/owner: stop direct commits on the old integ base, then approve gate O0 (merge main into `w23/integ`, reset lanes).
2. VF-0 now (does not need O0).
3. CW-1, CW-2, CW-3, CW-4 in parallel on lanes a to d.
4. CW-5 to CW-8 as their predecessors land; CW-9 last.
5. Model Plane session: the seven OPEN/PARTIAL asks in 4.2 and the PLAN.md block in 4.3.
6. User session: VF-2 stack legs, then VF-3.
