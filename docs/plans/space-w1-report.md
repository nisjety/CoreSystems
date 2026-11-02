# Space wave W1 report (M0 + M1), for the owner

Date 2026-10-27. Branch `space` (cut from `model-plane` e603fde78, HEAD 2e5e9609c), nothing pushed. Sources: `space-build-ledger-w1.md`, `space-depth-register-w1.md` (683 rows: P0 0, P1 100, P2 368, P3 215), plan 2.6, 4.1.1, 6.4.

Bottom line: W1 is built to scope, compiles, and its pure-code tests are green. Nothing is proven live: no review round, no stack, no image rebuilt. There is no P0, but clusters 1, 2, 3 and 7 below need fixing before a live run.

## 1. Delivered against the exit criteria (plan 4.1.1)

| Criterion | Status |
|---|---|
| M0 seams P-SC1, P-GW1, P-UC1, P-CX1 | Code done. Convex deploy of the split schema unverified (D1-028). |
| M0-T0 seed, fixtures, baseline | Partial. Seed, `space-fixtures.ts`, `space` Playwright project exist; baseline JSON not captured (D1-011); seed `--apply` never ran. |
| M0-CI `space-gate.yml`, `convex-core.yml` | Code only. Never run on a real PR; not a required check (D1-002); red for SPA/auth-core until the baseline exists (D1-001). |
| M1 (1) scenarios S1..S7, S9 on rebuilt images, non-author | NOT proven. M1-S8 (SCIM 501, sandbox caps) is unit-tested only. |
| M1 (2) G2/G3 | Plan approved at defaults 2026-10-21. G3 open: `SESSION_CORE_SPACE_ZDR` ships `observe` (D1-390). SD-14 assumed at default (D1-356). |
| M1 (3) run guard ON before human posts | Met by code (guard default ON, posts core default 0). Live behaviour unverified (D1-551). |
| M1 (4) containment ON plus a week of counters | ON in code; counter week not started (D1-568). |
| M1 (5) audit enforce proven in production, SCIM and SSO deployed | Code only; no HAR, no deploy. |
| M1 (6) ZDR tour, zero rows | Not run; known gaps in cluster 1. |
| M1 (7) dev flags on, staging with second member | Dev defaults on; staging untouched. |
| M1 (8) numbers in ledger | In the W1 ledger (session-core 0071, 0072, 0073, 0085; user-core 033, 034, 035; integration-corev2 0014; proto tags; flags). Model-ledger entry not checked. |

Built short of its slice: WS04-S7 gateway, SPA and docs legs (archive routes, banner, flag) are missing (D1-352), so an archived room reads as not-found and M1-S6 cannot pass. Four SPA flags (thread pane, send idempotency, connectivity banner, room artifacts) are build-time `VITE_*` stand-ins, not BFF `features.*`.

## 2. The numbers

| Item | Value |
|---|---|
| Slices and legs | 70 entries: 68 built, 1 partial (M0-T0), 1 already present |
| Commits since e603fde78 | 105 (17 are test-phase commits) |
| Files changed | 507, +77,748 / -3,087 |
| Tests written | 1,728 in 17 clusters |
| Run / passed | 2,266 / 2,260 (run includes pre-existing tests; 6 skipped SPA cases) |
| Failures found and fixed | 43, nearly all stale tests or mocks; two compile breaks |
| Written but deferred | 144 |
| Compile | `cargo check` green (mp-contracts, session-core, model-gateway, execution-core, gateway); `go build ./...` green in user-core, org-core, audit-core, integration-corev2, Model Plane `go/gen`; Convex tsc clean in a scratch copy; SPA `tsc -b`: 34 errors in 3 untouched files; auth-core not type-checked (D1-380); gateway `clippy -D warnings` red on 14 old errors |

Not covered by the test phase:
- DB tier: about 105 Postgres-gated tests written, compiling, never run. No migration (0071 to 0085, 033 to 035, 0014) has been applied by a test.
- Docker, `docker compose config`, `nginx -t`, image builds, Convex live push.
- Playwright: about 20 cases in 6 specs only listed, not run.
- Live LLM, realtime timing (1.5 s p95, 35 s degraded), revocation timing (15 s p95).
- Baseline capture (M0-T0), `space-gate` on GitHub, seed `--apply`.
- `zdr-contract-sync.test.sh` has 26 to 30 failures (mostly pre-existing), so `REQUIRE_CONTRACT=1` cannot pass; M1-S4 needs it. `action-surface-contract` dispatcher equality (11 ids) is also red at HEAD.

## 3. Depth priorities: top 12 clusters of the 100 P1 rows

No confirmed hole in committed code. "Suspect" means it looks like one but is unverified.

| # | Cluster and why it matters | Rows | Slices | Fix |
|---|---|---|---|---|
| 1 | ZDR on Space writes. Observe mode persists posts in ZDR Spaces; two model-gateway routes that store data (presentation, memory controls) have no ZDR guard (suspect); no BFF route calls `org_zdr_lookup`; 8 routes lack a surface-registry row; room send key sent for ZDR orgs. | D1-390, 401, 402, 403, 151, 174, 638, 639 | WS06-S2/S4, WS01-S4/S8, WS02-S3 | M |
| 2 | Memory containment gaps. Other memory writers (skill proposals, learning events, dedup) not audited; read decisions never stamp `space_kind`; withheld list shows only `scope='user'`; filter cost unknown; 15 DB tests never ran. | D1-558, 559, 560, 569, 570, 571, 646 | WS17-S1/S2 | M |
| 3 | Human posts versus model context and append authority. `message_search` not filtered by `post_kind` (suspect: recall could surface posts); v1 digest does not bind `post_kind`; second-caller risk on the in-tx gate; member INSERT never run on Postgres. | D1-072, 073, 075, 090, 091, 541, 527 | WS01-S1/S3, WS07-S9a | S |
| 4 | Control membership authority. Any Control write flips source to `member_ops` incl. the registrar; GDPR saga leaves erased users' membership rows; rejected audience retried every 10 min; one lagging user blocks an audience; streams stay open on archived rooms. | D1-324, 322, 333, 335, 354, 353, 268, 267 | WS04-S4/S5/S7 | M |
| 5 | Production credential registry. New scopes (`spaces:membership:act/read`, `directory:read:self`, `spaces:policy:admin:*`) exist only in dev compose; production registry is outside the repo, so every ACT call is 403 while the UI shows. | D1-323, 334, 343, 225, 344, 226 | WS03-S3, WS04-S4/S5/S6 | S |
| 6 | Identity and audit. SSO users promoted to admin before the fix keep it; audit-core tests not updated for enforcement; Trust Center aggregates 200 rows not 500; dead SCIM store paths. | D1-376, 361, 362, 370 | WS05-S1/S2/S3 | S |
| 7 | Solid 2 defects on the headline flow. A human post never calls the API (journal read right after write); connectivity banner halts reactivity; `RegionBoundary` never classifies; per-tab panel isolation leaks; optimistic reply row lingers or doubles; pane and room can run two sends. | D1-651, 652, 662, 674, 425, 120, 121, 143, 292 | WS01-S6/S7b/S8, WS07-S1, WS04-S2.spa | M |
| 8 | Never-run boot and DB paths. POST merged onto GET/DELETE routes may panic model-gateway at boot; 0071 must precede the new binary; Convex error shape and split-schema push; seed assumptions. | D1-102, 074, 028, 276, 012, 091 | P-CX1, WS01-S1/S4, WS04-S2.gw, M0-T0 | S to run |
| 9 | Typed denials, flood, relay. 503 for typed refusals on five routes; stream 403 loses typed details; SPA catch and timeline swallow typed 403; no per-user limit on posts or member ops; upstream error bodies relayed unfiltered. | D1-214, 184, 242, 258, 259, 284, 109, 110, 342 | WS01-S5, WS02-S4, WS03-S2/S5/S7, WS04-S6 | M |
| 10 | Pins and CI baseline. Gate cannot go green without baseline; stale token-manifest pin; admission golden rows (regenerated in tests, Model Plane consumers not run); metric-label list stale; `client_contract` literal. | D1-001, 002, 011, 013, 022, 046, 234, 626, 465 | M0-CI/T0, P-CFG, WS03-S4, WS06-S3, WS07-S2 | S |
| 11 | Realtime and run-guard edges. No alert on trigger failure; "who archived" missing from the log; org-core blip downgrades all streams; run-guard 409 unmapped for wake, invoke, browser; refused send leaves an orphan post; spawn permit not held for process life; `limit_hit` unread. | D1-158, 159, 175, 185, 195, 549, 550, 417, 418 | WS02, WS09-S1, WS06-S7 | M |
| 12 | Legs and flag delivery not built. Archive gateway and SPA legs; BFF `features.*`; sendId shape (`_` or `-` first char gives 400); proto regen for Go and Python owed. | D1-352, 283, 307, 150, 439, 447, 448, 525, 526, 103, 132, 275 | WS04-S7, WS01-S5/S8, WS07-S9/S11/S12 | M |

P2/P3 themes: about 30 deferred live flows; unverified nginx, axum, Solid 2 and Convex behaviour; per-row subqueries and scans that need indexes at scale; unreviewed Norwegian copy; decision tokens in query strings; an owner with revoked membership can still post (existing path).

## 4. Owner questions (from 112 decision rows), recommendation first

| # | Question | Recommendation |
|---|---|---|
| 1 | SD-12: when to flip ZDR to enforce, and who writes the copy? | Flip in dev after a clean counter week; counsel owns copy; make `process_registry` inert under ZDR now. |
| 2 | May the organization room be renamed? Its sync is off by default in production. | Block rename; enable reconcile per org at M4. |
| 3 | Archive: accept SD-14, reuse scope `spaces:membership:act`, keep streams open? | Accept; close streams on archive, reads stay. |
| 4 | Should an add adopt `application_sync` rows; should the registrar stay guarded after any write? | Adopt at cutover; keep an explicit registrar guard. |
| 5 | Policy: add the browser bit to the catalogue; refuse only newly enabling ZDR-inert bits; keep `zdr_blocked`. | Yes to all three. |
| 6 | Where is the production credential registry; who adds the scopes (SD-27)? | Operator answers before any W1 flag goes on in staging. |
| 7 | Move the four `VITE_*` flags to BFF `features.*`? | Yes, in the next depth pass. |
| 8 | Thread semantics: member posts bump `last_activity_at` only; `reply_count` skips assistant turns; collapse says "N more messages". | Accept; posts never become previews. |
| 9 | Allow posts while a thread awaits approval; accept an orphan post when the run guard refuses a send? | Allow; accept for W1, queue in W2 (SD-15 C2). |
| 10 | Rate limits on posts and member ops. | Per-user static limiter, 30 per minute, like directory search. |
| 11 | Withheld legacy memory: list all scopes, add a keep action, stay fail-closed? | Yes to all. |
| 12 | Test model (E3/SD-27) and Norwegian copy review. | Owner's Anthropic key with `claude-sonnet-5-5` as `E2E_SPACE_MODEL`; native review before production. |

## 5. Deviations that change later waves

- Paths: `components/sidepane/` replaces `aux/` (reserved on Windows); WS12-S7, WS13-S4a, WS17-S6, WS20-S5 must use it. Run guard is `session-core/src/run_phase.rs`; gateway seam is `dispatchers_spaces.rs`; spaces-client is one `core.ts` plus a barrel.
- Flags: gateway flags sit on `SpaceFeatureFlags`; limiters are process-wide statics (a new AppState field breaks five test literals), which WS04-S8 onward should repeat; `SPACE_RUN_THREAD_GUARD` defaults ON.
- Already present from sibling work: send dedupe, chunk recovery, safe-href, `audit-core-ci.yml`; do not duplicate.
- Hooks W2 must honour: WS09 adds `work` to `ROOM_VISIBLE_METADATA_KEYS` (reduced first); WS10 derives `awaiting_approval`; WS08 swaps name-based agent attribution for ids and maps the run-guard 409; WS12 adds surface-registry rows; WS17-S3 replaces derived `room_unverified` with a column.
- Contracts: metric labels and route class `Config` widened; artifacts keyset has three columns; stream degrades after 3 consecutive failures; clients key off `code`, never status. Goldens `space-events` and `space-surface` are hand-authored with drift tests.

## 6. Recommended next steps, in order

1. Depth pass A (code only): cluster 7 first (the headline flow cannot send), then 3, 1, 2, 9, 10.
2. Side-by-side stack, not the shared dev stack: migrations 0071 to 0085, user-core 033 to 035, flags and the Convex push would hit shared databases. Use a separate compose project name and private network, fresh Postgres, Dragonfly and NATS volumes, offset host ports, images tagged from the commit under test, a second Vite port (not 5173), flags set only there. Check free disk on C: first; tear down by project name only.
3. Test configuration for every live run: the owner's Anthropic API key from the local environment (never printed or committed) and model `claude-sonnet-5-5` as `E2E_SPACE_MODEL`. No Verevon budget, balance, genius or other Verevon model; the fixtures already reject those ids.
4. Boot smoke (migrations apply, router builds per D1-102, Convex "functions ready", flags on then off). Then baseline capture from a clean worktree of e603fde78, seed `--apply`, M1-S1 to S9 by a non-author per plan 6.4, ZDR tour, flag-off drills, and the roughly 105 DB tests on a scratch database.
5. Depth pass B after live findings: clusters 4, 5, 6, 8, 11, 12, after the owner answers section 4.
6. Make `space-gate` a required check; wire the new node tests into the gate self-test job; run `control-train-repin.sh --apply` once.

Wave 2 (M2.1: WS08, WS09 S2-S8, WS10 S1-S5b): code dependencies exist (WS01, WS02, WS06-S4, WS09-S1), so it is not hard-blocked, but I recommend waiting for step 1 and the boot smoke of step 4, then running W2 code in parallel with the live M1 proof. To clear first: G4 decisions (SD-00, SD-05, SD-20, SD-21, SD-26), run-guard 409 mapping (D1-549), and the baseline. Building W2 on an unproven post and stream core compounds any live defect.

## 7. Risks

- No review rounds: each slice has one author; cross-slice seams are the likeliest defect sites.
- No live proof: boot, migration, router, Convex and nginx behaviour are assumptions, and many tests use stubs that encode the builders' beliefs about Control, org-core and Session Core.
- Solid 2.0.0-rc.0 semantics: three defects came from it; more may exist (context into lazy panels, `Errored` with lazy imports).
- Authority SQL (audit narrowing, `DiagnoseDenial`, member and ledger writes) never ran on Postgres.
- Path-limited commits swept in other builders' hunks (D1-266, 447, 481, 504), so reverting one slice is not clean.
- Windows only (Git Bash, CRLF); Linux CI behaviour of the gate scripts is unverified. Two files are staged but uncommitted; `.plan-work/` and three plan docs are untracked.
- Flags default on in dev and off in production via `APP_ENV`; an unset `APP_ENV` in staging behaves like dev. The three posts flags (BFF, model-gateway, session-core) must flip together (D1-116).
- Pre-existing red suites (zdr-contract-sync, action-surface, clippy, 34 SPA tsc errors) can mask a new regression until the baseline exists.
