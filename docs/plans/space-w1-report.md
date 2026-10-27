# Space wave W1 report (M0 + M1), for the owner

Date 2026-10-27. Branch `space` (cut from `model-plane` e603fde78, HEAD 2e5e9609c), nothing pushed. Sources: `docs/plans/space-build-ledger-w1.md`, `docs/plans/space-depth-register-w1.md` (683 rows: P0 0, P1 100, P2 368, P3 215), plan sections 2.6, 4.1.1, 6.4.

Bottom line: W1 is built to scope and compiles, and the pure-code test tier is green. Nothing has been proven live. No review rounds ran and no stack was brought up, so every M1 scenario is still "claimed by code only". There is no P0, but four findings need a fix before a live run (clusters 1, 2, 3 and 7 below).

## 1. Delivered against the exit criteria (plan 4.1.1)

| Criterion | Status |
|---|---|
| M0 seams P-SC1, P-GW1, P-UC1, P-CX1 | Code done. "Test counts unchanged" shown only by the test phase; Convex deploy of the split schema is unverified (D1-028). |
| M0-T0 seed, fixtures, baseline tooling | Partial. Seed script, `space-fixtures.ts` and the `space` Playwright project exist. The baseline JSON under `docs/plans/evidence/baseline-<date>/` is not captured (D1-011); the seed `--apply` never ran. |
| M0-CI `space-gate.yml`, `convex-core.yml` | Code done. Never run on a real PR; not a required check (D1-002); stays red on SPA/auth-core until the baseline exists (D1-001). |
| M1 (1) scenarios M1-S1..S7, S9 on rebuilt images by a non-author | NOT proven. Code exists for all; no image rebuilt. M1-S8: SCIM 501 and sandbox caps are code and unit tests only. |
| M1 (2) G2 before WS01-S3/WS02-S3; G3 before enforce flips | Plan approved at defaults 2026-10-21. G3 is open: `SESSION_CORE_SPACE_ZDR` ships `observe` (D1-390), so no enforce flip yet. SD-14 was assumed at default (D1-356). |
| M1 (3) WS09-S1 guard ON before human posts | Met by code: guard default ON, `SPACE_HUMAN_POSTS_CORE` default 0 (dev 1). Live behaviour of the derived run phase is unverified (D1-551). |
| M1 (4) WS17-S1 containment ON, one week of counters | ON by default in code. The one-week counter review has not started (D1-568). |
| M1 (5) WS05-S1 enforce proven in production, S2/S3 deployed | Code only. No HAR, no deploy. |
| M1 (6) ZDR tour, zero rows | NOT run. Known gaps in cluster 1. |
| M1 (7) dev flags on, staging with a second member | Dev defaults on in code and compose. Staging not touched; second member seed unproven. |
| M1 (8) ledger rows for numbers | Recorded in the W1 ledger (migrations session-core 0071, 0072, 0073, 0085; user-core 033, 034, 035; integration-corev2 0014; proto tags; flags). Entry in the Model ledger tables not checked. |

Not built inside "built" slices: WS04-S7 gateway, SPA and docs legs (archive routes, banner, flag `space_room_archive_api`; D1-352). Until they land, an archived room reads as not-found in the SPA and M1-S6 cannot pass. Also open: the SPA reads build-time `VITE_*` stand-ins for four flags (thread pane, send idempotency, connectivity banner, room artifacts) instead of BFF `features.*`.

## 2. The numbers

| Item | Value |
|---|---|
| Slices and legs | 70 ledger entries: 68 built, 1 partial (M0-T0), 1 already present (WS07-S9.spa) |
| Commits since e603fde78 | 105 (includes 17 test-phase commits) |
| Files changed | 507, +77,748 / -3,087 (`git diff --shortstat e603fde78..HEAD`) |
| Tests written in the test phase | 1,728 across 17 clusters |
| Tests run / passed | 2,266 run (includes pre-existing tests in touched suites) / 2,260 passed; the 6 other run cases are skipped SPA cases |
| Failures found and fixed | 43, nearly all stale tests or test mocks; 2 compile breaks (session-core DB test call sites, a Space test file) |
| Written but skipped (deferred tier) | 144 |
| Depth rows from tests | 57 |
| Compile | `cargo check` green for mp-contracts, session-core, model-gateway, execution-core and the gateway; `go build ./...` green in user-core, org-core, audit-core, integration-corev2, Model Plane `go/gen`; Convex tsc clean in a scratch copy; SPA `tsc -b` leaves 34 errors in 3 untouched files; auth-core not type-checked (no node_modules, D1-380) |
| Lint | gateway `clippy -D warnings` fails on 14 pre-existing errors (D1-600) |

Not covered by the test phase:
- DB tier: about 105 Postgres-gated tests (Control 27 + 14, session-core 25 + 23 + 15, SCIM redaction) are written and compile but never ran. Migrations 0071 to 0085 and 033 to 035 have never been applied by a test.
- Docker, `docker compose config`, `nginx -t`, image builds, Convex live push.
- Playwright: about 20 e2e cases in 6 specs are written and only listed, not run.
- Live LLM, live realtime timing (1.5 s p95, 35 s degraded), revocation timing (15 s p95).
- Baseline capture (M0-T0), `space-gate.yml` on GitHub, seed `--apply`.
- `zdr-contract-sync.test.sh`: 26 to 30 failures, mostly pre-existing, so `REQUIRE_CONTRACT=1` cannot be green (M1-S4 needs it).
- Pre-existing red, unchanged: `action-surface-contract.test.mjs` dispatcher equality (11 registry ids without a handler arm; D1-046, D1-601).

## 3. Depth priorities: top 12 clusters of the 100 P1 rows

No P0 and no confirmed hole in committed code. Items marked "suspect" look like holes but are unverified.

| # | Cluster (why it matters) | Rows | Slices | Fix |
|---|---|---|---|---|
| 1 | ZDR coverage on Space writes. Observe mode persists posts in ZDR Spaces; two model-gateway routes that store data (presentation, memory controls) have no ZDR guard (suspect hole); no BFF route calls `org_zdr_lookup`; 8 routes lack a `space-surface` row; the room send key is sent for ZDR orgs. | D1-390, 401, 402, 403, 151, 174, 638, 639 | WS06-S2/S4, WS01-S4/S8, WS02-S3 | M |
| 2 | Memory containment is incomplete. Other background memory writers (skill proposals, learning events, dedup passes) not audited; read decisions never self-heal `space_kind`; legacy-room filter cost unknown; withheld list shows only `scope='user'`; 15 DB tests never run. | D1-558, 559, 560, 569, 570, 571, 646 | WS17-S1/S2 | M |
| 3 | Human posts leaking into model context and append authority. `message_search` not filtered for `post_kind` (suspect: model-callable recall could surface posts); v1 digest does not bind `post_kind`; second-caller risk on the in-tx gate; member INSERT binds never run on Postgres; reduced room-read detection rests on a ref. | D1-072, 073, 075, 090, 091, 541, 527 | WS01-S1/S3, WS07-S9a | S |
| 4 | Control membership authority. Control-native writes flip source to `member_ops` incl. the registrar; GDPR saga leaves membership rows of erased users; rejected audience is re-registered every 10 min; one lagging user breaks a whole audience; streams stay open on archived rooms. | D1-324, 322, 333, 335, 354, 353, 268, 267 | WS04-S4/S5/S7 | M |
| 5 | Production credential registry. Scopes `spaces:membership:act/read`, `directory:read:self`, `spaces:policy:admin:*` exist only in dev compose; production `USER_CORE_SERVICE_CREDENTIALS` is outside the repo. Without them every ACT call is 403 while the flag shows the UI. | D1-323, 334, 343, 225, 344, 226 | WS04-S4/S5/S6, WS03-S3 | S |
| 6 | Identity and audit. Old SSO users already promoted to admin keep it; audit-core tests not updated for role enforcement; Trust Center now aggregates 200 rows not 500; dead SCIM store paths remain. | D1-376, 361, 362, 370 | WS05-S1/S2/S3 | S |
| 7 | SPA Solid 2 defects that break the headline flow. Human post never calls the API (journal read right after write); connectivity banner halts reactivity when on; `RegionBoundary` never classifies (accessor not called); per-tab panel isolation leaks; optimistic reply row lingers or doubles; two pane/room send pipelines can run at once. | D1-651, 652, 662, 674, 425, 120, 121, 143, 292 | WS01-S6/S7b/S8, WS07-S1, WS04-S2.spa | M |
| 8 | Never-run boot and DB paths. Router merge of POST onto GET/DELETE may panic at model-gateway boot; migration 0071 must precede the new binary; Convex error shape and split-schema deploy; M0-T0 seed assumptions. | D1-102, 074, 028, 276, 012, 091 | P-CX1, WS01-S1/S4, WS04-S2.gw, M0-T0 | S to run, unknown to fix |
| 9 | Typed denials, flood and relay. 503 for typed refusals on five routes; Control 403 loses typed details on the stream; SPA catch and timeline swallow typed 403; no per-user limit on posts or member ops; upstream error bodies relayed unfiltered; retrieval decline carries no reason. | D1-214, 184, 242, 258, 259, 284, 109, 110, 342 | WS01-S5, WS02-S4, WS03-S2/S5/S7, WS04-S2.gw/S6 | M |
| 10 | Contract pins and CI baseline. space-gate cannot go green without the baseline; not a required check; stale pin for the token manifest; admission golden missing rows (regenerated in tests, Model Plane consumers not run); metric-label list stale; `client_contract` literal mirrors. | D1-001, 002, 011, 013, 022, 046, 234, 626, 465 | M0-CI/T0, P-CFG, WS03-S4, WS06-S3, WS07-S2 | S |
| 11 | Realtime and run-guard edges. Trigger failures have no alert; "who archived" absent from the log; golden hand-authored with no drift guard in CI; org-core blip downgrades every stream; run guard 409 not mapped for wake, invoke and browser starts; refused send leaves an orphan post; sandbox spawn permit not held for process lifetime; `limit_hit` unread. | D1-158, 159, 175, 185, 195, 549, 550, 417, 418 | WS02-S1/S3/S4/S5, WS09-S1, WS06-S7 | M |
| 12 | Legs and flag delivery not built. Archive gateway and SPA legs; BFF `features.*` for four flags; sendId shape mismatch (`_` or `-` first char gives 400); `aux` renamed `sidepane`; Go and Python proto regen owed. | D1-352, 283, 307, 150, 439, 447, 448, 525, 526, 103, 132, 275 | WS04-S7, WS01-S5/S8, WS07-S9/S11/S12 | M |

P2 and P3 themes (not listed row by row): about 30 deferred live flows; unverified live behaviour on nginx, axum, Solid 2 and Convex; per-row subqueries and full scans that need an index at scale (org room sync, artifacts scan, `ILIKE` directory); unreviewed Norwegian copy; decision tokens in query strings (access-log exposure); owner-with-revoked-membership can still post (existing path).

## 4. Owner questions (from 112 decision rows), recommendation first

| # | Question | Recommended answer |
|---|---|---|
| 1 | SD-12: when to flip `SESSION_CORE_SPACE_ZDR` to enforce, and who writes the copy? | Flip in dev after one clean week of counters; counsel owns the copy; mark `process_registry` inert under ZDR now. |
| 2 | May the organization room be renamed? Org-room sync is off by default in production, so org rooms stay owner-only until `SPACE_ORG_ROOM_RECONCILE` is on. | Block rename of the org room; turn reconcile on per org at M4. |
| 3 | Archive: accept SD-14 (Control read-class state), reuse scope `spaces:membership:act`, keep streams open on archived rooms? | Accept; close live streams on archive, reads stay. |
| 4 | Should an add adopt an `application_sync` row, and should the registrar stay protected after any Control write? | Adopt on add at cutover; keep an explicit registrar guard independent of source. |
| 5 | Policy semantics: add the browser bit to the catalogue; keep "refuse only newly enabling ZDR-inert/unproven bits"; keep the `zdr_blocked` field. | Yes to all three. |
| 6 | Where does the production credential registry live and who adds the new scopes (SD-27)? | Operator answers; add before any W1 flag goes on in staging. |
| 7 | Move the four build-time `VITE_*` flags to BFF `features.*`? | Yes, next depth pass, so production kill needs no rebuild. |
| 8 | Thread semantics: member posts bump `last_activity_at` only; `reply_count` excludes assistant turns; collapse says "N more messages". | Accept; posts never become list previews. |
| 9 | Allow posts while a thread awaits approval; accept an orphan post when a send is refused by the run guard (queue is SD-15 C2)? | Allow; accept for W1, queue in W2. |
| 10 | Rate limits for posts and member ops (none today). | Per-user process-wide limiter, 30 per minute, as directory search already does. |
| 11 | Legacy withheld memory: list all scopes, add an explicit keep action, keep fail-closed on DB error? | Yes to all three. |
| 12 | Test configuration: no deterministic dev model exists (E3/SD-27); NO copy unreviewed. | Use the owner's Anthropic key with `claude-sonnet-5-5` as `E2E_SPACE_MODEL`; native review of Norwegian copy before production. |

## 5. Deviations that change later waves

- Paths: `components/sidepane/` replaces `aux/` (Windows reserved name): WS12-S7, WS13-S4a, WS17-S6, WS20-S5 must use it. Run guard lives at `session-core/src/run_phase.rs`; gateway workstream seam is `dispatchers_spaces.rs`; spaces-client is still one `core.ts` plus a barrel.
- Flags: build-time SPA stand-ins; gateway flags live on `SpaceFeatureFlags`; limiters are process-wide statics (new AppState fields would break five test literals), so WS04-S8 and later reuse that choice; `SPACE_RUN_THREAD_GUARD` defaults ON.
- Reuse of sibling work: send dedupe (`client_message_id`), chunk recovery key, safe-href and `audit-core-ci.yml` already exist; WS07-S2-style duplicates must not be added.
- Hooks W2 must honour: WS09 adds `work` to `ROOM_VISIBLE_METADATA_KEYS` (reduced first), WS10 derives `awaiting_approval`, WS08 replaces name-based agent attribution with ids and maps the run-guard 409, WS12 adds `space-surface` rows and WS17-S3 replaces the derived `room_unverified` with a column.
- Contract and catalogue: metric labels widened beyond the design; route class `Config` added; artifacts keyset is `(created_at, message_id, artifact_id)`; stream degrade is 3 consecutive failures; denial copy uses code, never status.
- Goldens (`space-events`, `space-surface`) are hand-authored with drift tests, not generators.

## 6. Recommended next steps, in order

1. Depth pass A (code, no stack): clusters 3, 7, 1, 2, then 9 and 10. Cluster 7 first in time, since the headline flow cannot send today.
2. Side-by-side stack, not the shared dev stack. Reason: migrations 0071 to 0085, user-core 033 to 035, flags and the Convex push would hit shared databases. Plan: a separate compose project name and private network; its own Postgres, Dragonfly, NATS volumes (fresh); host ports offset; images tagged from the commit under test; a second Vite port (not 5173); flags set only in that project. Check free disk on C: first. Tear down with its own project name only.
3. Test configuration for every live run: the owner's Anthropic API key from the local environment (never printed, never committed) and model `claude-sonnet-5-5`, set as `E2E_SPACE_MODEL`. No Verevon budget, balance, genius or any other Verevon model; fixtures already reject those ids.
4. Boot smoke first (cheap): migrations apply, router constructs (D1-102), Convex "functions ready", flags on then off. Then capture the baseline from a clean worktree of e603fde78, run the seed `--apply`, then M1-S1 to S9 per plan 6.4 by a non-author, then the ZDR tour and flag-off drills. Run the ~105 DB tests on the scratch database in the same window.
5. Depth pass B after the first live findings: clusters 4, 5, 6, 8, 11, 12. Resolve owner questions 1 to 12 before it.
6. Make `space-gate` a required check; wire the new node tests into the gate self-test job; one `control-train-repin.sh --apply`.

Wave 2 (M2.1: WS08, WS09 S2-S8, WS10 S1-S5b): not blocked in the sense of code dependencies, since WS01, WS02, WS06-S4 and WS09-S1 exist. Recommendation: do not start it yet. Start after step 1 and the boot smoke of step 4, and let the live M1 proof run in parallel. Blockers to clear: G4 decisions (SD-00, SD-05, SD-20, SD-21, SD-26), the run-guard 409 mapping (D1-549), cluster 11 hooks, and the baseline. Building W2 on an unproven human-post and stream core compounds any defect found live.

## 7. Risks

- No review rounds: every slice has one author and the committed code has not been read by a second person. Cross-slice seams are the likeliest place for defects.
- No live proof: boot, migration, router, Convex and nginx behaviour are assumptions. Several tests passed against stubs and wiremock doubles that encode the builders' beliefs about Control, org-core and Session Core.
- Solid 2.0.0-rc.0 semantics are only partly understood; three defects came from it and more may exist (context reaching lazy panels, `Errored` with lazy imports).
- Authority paths rely on tests with a stub store: audit-core SQL narrowing, `DiagnoseDenial` SQL, member INSERT and ledger paths were never executed on Postgres.
- Shared-file sweeps: path-limited commits carried other builders' hunks (D1-266, 447, 481, 504); reverting a single slice is not clean.
- Windows only: runs used Git Bash and CRLF checkouts; Linux CI behaviour of the new gate scripts is unverified. Two files are staged but not committed (`control-train-repin-test.sh` mode, `space-contract-vectors-sync.test.sh`), and `.plan-work/` and three plan docs are untracked.
- Flags default on in dev and off in production by code path, so an unset `APP_ENV` in staging silently behaves like dev or production; the three-flag posts rollout (BFF, model-gateway, session-core) must flip together (D1-116).
- Pre-existing red suites (zdr-contract-sync, action-surface, clippy, 34 SPA tsc errors) can mask a new regression until the baseline exists.
