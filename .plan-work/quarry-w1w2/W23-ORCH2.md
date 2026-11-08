# W23 orchestration, pass 2 (Quarry Wave 3 build vs proven, Model Plane sync)

Written 2026-11-08 by a fresh orchestration subagent on `w23/integ` (worktree `.claude/worktrees/innh-lane-integ`, HEAD `9da2c64b8`). It follows `W23-ORCH.md` (2026-11-07) and records only what changed since then. Only this file is committed. No code was written, nothing was pushed, merged, deployed or flag-flipped. Facts are from `git` and `git grep` against `HEAD`, `main` and `model-plane`. No tests were run in this pass.

## 0. Branch facts

| Fact | Evidence | Consequence |
|---|---|---|
| **Gate O0 is done.** main was merged into integ | da5cc09c6 "merge: main into w23/integ for W3 B4 (CW lane)", 2026-11-07 14:10; `main...HEAD` = 0 / 10 | CW slices can start. integ is now 10 ahead of main (E3.2-g1, the CW-1 crawl fix, docs) and 0 behind |
| main has not moved since 2a776c837 (2026-11-06) | `git log -1 main` | Nothing new to bring in from main |
| **Lanes not reset.** `w23f/lane-a..d` are 0 ahead and 261 to 271 behind integ | `rev-list --left-right` | A fast-forward is safe (no lane work would be lost), but it has not been done. Owner or integrator: `git -C innh-lane-X merge --ff-only w23/integ` before giving a lane a CW slice |
| `model-plane` HEAD 5fbbe2c79 (2026-11-07 22:13) is 28 ahead of main and of integ; merge-base with integ is main (2a776c837) | `rev-list` | Model Plane is not waiting on integ, and integ is not waiting on Model Plane, but the next merge to main from either side will conflict (see 0.1) |
| `mp-ep14-s1-gap` is 1 ahead of model-plane | `rev-list` | Unchanged from pass 1 |
| **A second writer is active in this worktree, with staged, uncommitted changes**: `quarry-runtime/src/observation.rs` (+72: a PII-redacting run refuses agent `Screenshot` and `Pdf` with `pii_redaction_failed` / `capture_not_redactable` before anything reaches the CAS, plus a test) and `quarry-runtime/src/pipeline.rs` (4 items made `pub(crate)` for that) | `git diff --cached --stat` | That writer owns `observation.rs` and `pipeline.rs` until it commits. This file was committed with a path-limited commit so the staged work stays staged and untouched. The CW-2 and CW-1-remainder slices wait for it |

### 0.1 Merge conflict that is already certain

The same four files changed on both sides since main:

- `verevonv3/src/shared/actions/action-registry.ts`, `action-registry.test.ts`, `model-access.ts`
- `verevonv3/src/shared/api/contracts/agent-action-admission.json` (generated)

The admission golden now exists in three versions:

| Where | Blob | Content |
|---|---|---|
| integ SPA copy | 438b5cbc | E3.2-g1 rows (`browser.get_org_policy`, `browser.set_org_policy`) |
| integ Model Plane canonical and pin | d5632bd7 | the main copy, without the g1 rows |
| model-plane SPA and canonical | 38f1596c | the W4-B viewer-registry rows (a9b084e72), without the g1 rows. The canonical pin row in `zdr-contract-sync.test.sh` is c93bc64d |

Resolution rule for whoever merges second: keep both sides in `action-registry.ts` and `model-access.ts`. Do **not** hand-merge the JSON. Regenerate it once (`VEREVON_UPDATE_GOLDEN=1 pnpm vitest run src/shared/actions/admission-contract.test.ts`), then copy the bytes to the canonical file and its three vendored copies, and move the `.sha256` pin and the `zdr-contract-sync.test.sh` row (around line 209). This is the CW-4 integrator step from pass 1. It now also has to include model-plane's W4-B rows. One commit, one writer.

## 1. Wave 3 build vs proven (Quarry queue: W3-B4 to W3-C4 = 58 units, plus B1 to B3 = 15)

| Measure | Pass 1 (11-07) | Now | Change and source |
|---|---|---|---|
| Units landed on integ | 72 of 73 (99 %) | **73 of 73 (100 %)**, 1 partial | E3.2-g1 built dark: cf54d1ef5 gateway (`BROWSER_POLICY_ADMIN_API`), ddf826ad0 SPA (`VITE_BROWSER_POLICY_SETTINGS`), 7de79ed70 error copy. E3.8.4d is still partial (log canary, Redis behavioural tests, crawl `max_age` key) |
| Queue units reachable end to end with the flag on | about 17 of 58 (30 %) | **about 18 of 58 (31 %)**, estimate | g1 is reachable, but turning it on is blocked until Model Plane takes the admission golden (0.1). Nothing else was wired |
| Units whose code ran in a unit-test gate | about 39 of 58 (67 %) | **about 47 of 58 (81 %)**, estimate | W23-W3-VERIFY.md (11-07) ran verevonv3 vitest on the W3 paths: 403 of 413 files passed. None of the 17 failures is from integ: 11 + 3 also fail on model-plane, 1 is the stale zdr-refusals constant, 1 is run-console drift, 1 is g1 copy, fixed in 7de79ed70. Also ran: the gateway bin suite (1812 / 0, with g1), and integration-corev2 (E3.1-g `mail_otp.go` has no tests). **Still not unit-run:** E3.2-f and E3.7.10-CP (auth-core jest, no node_modules in integ), E3.1-g (no test file), user-core migration 044, convex-core browserActivity, E3.8.4d remainder. The 10 runtime and 14 edge Windows-only artifact-store failures were not re-run on Linux |
| Units proven on a stack | 0 of 58 | **0 of 58** | By plan decision (the user tests W3 after the merge) |
| SC1 to SC7 fully PASS | 0 of 7 (6 partial, SC2b not run) | **0 of 7**, unchanged | Nothing re-run on current images |

### 1.1 HIGH items from pass 1 section 1.3, rechecked at HEAD

| Item | State at 9da2c64b8 | Evidence |
|---|---|---|
| Crawl ignores `redact_pii` (CW-1) | **FIXED** | 9da2c64b8: `redact_pii` and `redactPii` added to `crawl_scrape.rs` UNSUPPORTED_KEYS, plus test `crawl_refuses_redact_pii_never_silently_ignores_it`. No W23-STATUS row and no VF-1 run recorded yet |
| Per-request `redact_pii` never reaches the stage (D-E364a-2) | OPEN | `routes.rs:1089` and `:2416` still call `run_with_inline_request` (redact = false). The route does a text-only post-pass (`redact_pii_text`, E3.6-5) |
| PNG render to the CAS on per-request redacting runs (D-E364b-1) | OPEN, latent | Latent while D-E364a-2 is open. Land both together, as planned |
| Scanner panic hook prints page text (D-E364a-1) | OPEN | No change in `pipeline.rs` |
| Agent screenshot/PDF stored on a redacting run | **IN PROGRESS** (staged, other writer) | `observation.rs` staged diff, section 0 |
| OTP: screenshots stored and ObservationReady emitted before the edge strips; `attach.rs` delegate_step skips the read-denial check (CW-2) | OPEN | No commit touches them. The staged `observation.rs` change covers PII captures, not the OTP strip ordering |
| Browserless token in the URL (CW-3, D-E3967-1) | OPEN | `lab/evals/src/provider_matrix.rs:316` `format!("{}/content?token={}", …)`. **Correction to pass 1:** the file is `provider_matrix.rs`, not `release_matrix.rs`. The workflow `.github/workflows/quarry-v2-eval-tier-c.yml:42` documents the leak and does not upload the report |

### 1.2 Cross-plane test drift seen in VERIFY, rechecked

- model-gateway quarry-token claims golden: **CLEARED by O0**. The fixture blob equals the auth-core blob (52e039ea).
- run-console-client `browser_attach_shared_jar`: **CLEARED by O0**. The client has the case on HEAD and on model-plane.
- SPA admission copy against the Model Plane canonical: still RED. Fixed only by the 0.1 step.
- zdr-refusals TS constant (`ae4473ce…`) versus the pins (`422af9d1…`): the same on HEAD and model-plane. Re-run `zdr-refusals.test.ts` before calling it either way.

## 2. Model Plane status

### 2.1 Where Model Plane is (model-plane 5fbbe2c79)

- Since pass 1 (2af78fea2): 10 commits on 2026-11-07. EP-12 S1 export. Wave-4 lettered slices: W4-A R1 replay scrubber, W4-B P1 viewer registry, W4-C C1 typed computer connector (execution-core `computer_action.rs`, capability migration 0038, a zdr-posture-matrix row), W4-D M1a/M1b MCP-app network policy and CSP builder, W4-F T1 trigger record. Wave-5 numbered slices: 069 rewind side chat, 078 feedback surfaces, 079 typed flag registry (new `apps/Control Plane/flag-registry` Go module), 086 review as run.
- Every new item is behind a switch that is off by default. Each is unit-tested per its ledger entry. None is deployed. Deployed state is unchanged: the browser broker (2026-10-20) and `mp-browser-cap` on live dev (2026-10-29).
- Model Plane EP-1 to EP-14 S1 slices (pass 1) are unchanged. EP-14 S1 is still only on `mp-ep14-s1-gap`.
- Golden pins moved on model-plane: zdr-posture-matrix (2f33e15a), org-agent-policy-schema (6a8982bd), agent-action-admission (c93bc64d). integ carries the main values. Merging model-plane brings them, and `zdr-contract-sync.test.sh` must be re-run after that merge.

### 2.2 PLAN.md and ledger

- `apps/Model Plane/docs/PLAN.md`: last commit is still 0c2d43580 (2026-08-05) on both main and model-plane, so it is still out of date. The status block proposed in W23-ORCH.md section 4.3 has not been pasted. It now also needs one more sentence: "W4 (A to F) and W5 slices started 2026-11-07, dark." The owner of the file is the model-plane session. Not edited here.
- `docs/decisions/ledger.md` on model-plane: 38 dated 2026-11 entries, newest 2026-11-07 (WS17 S9 rewind undo, W4-D M1, EP-2-S2 `max_passages`). Still missing: the ADR-1 "one web-policy issuer" entry asked by W2-DOC-A. `git grep` finds no "ADR-1" or "one issuer" in the ledger.

### 2.3 MP-ASKS.md (asks from Quarry, Frontend and Control to Model Plane), rechecked on model-plane HEAD

| Ask | Pass 1 | Now | Check |
|---|---|---|---|
| W3-E3.2-g1 admission golden copy (new, 11-07) | not yet asked | **OPEN, now conflicting** | model-plane regenerated the golden for W4-B without the g1 rows (0.1) |
| W2-Q-WS7K broker web-policy snapshot (`ClaimPolicyResolver`) | OPEN | OPEN | 0 hits on model-plane |
| W2-AP-S7a nats.conf `mp.v1.trace.run.*` subscribe for `application-convex-model` | OPEN | OPEN | The `application-convex-model` block subscribes `mp.v1.run.*.event` and its inbox only. The trace subject appears only in a producer `publish` |
| W3-E3.1-ab broker `fill_otp` | OPEN | OPEN | 0 hits |
| W3-E3.1-i `fill_otp` approval fields | OPEN | OPEN | 0 hits |
| W3-E3.4-2-3 execution-core `resume_run_id` | OPEN | OPEN | 0 hits |
| W3-E3.2-e2 attach per-tab consent | OPEN | OPEN | 0 hits for `tab_consent` (the MP-ASKS text also waits on OD13 counsel) |
| W3-E3.3-g `browser_run.steer_taken` | PARTIAL | PARTIAL | 0 hits |
| W2-DOC-A ADR-1 ledger entry | PARTIAL | PARTIAL | 2.2 |
| W3-E3.3-c/d tab refs, trust classification | PARTIAL | PARTIAL | no change seen |
| Other rows (closed in pass 1) | CLOSED | CLOSED | not re-opened |

Result: 7 OPEN, 3 PARTIAL. None closed since pass 1. One new and conflicting (g1).

## 3. Blockers (ordered)

1. **Staged work by another writer in this worktree** (`observation.rs`, `pipeline.rs`). It holds the CW-2 and CW-1-remainder files. Nobody else should edit those two files until it commits, and no one should run `git commit -a` or `git add -A` here.
2. **Lanes a to d not fast-forwarded** to integ after O0.
3. **Admission golden three-way divergence** (0.1). This blocks turning on `BROWSER_POLICY_ADMIN_API` / `VITE_BROWSER_POLICY_SETTINGS` and the next main merge.
4. HIGH open items: CW-3 token in the URL, CW-2 OTP strip ordering and delegate_step, D-E364a-2 together with D-E364b-1.
5. CW-1 has no VF-1 row: 9da2c64b8 has no W23-STATUS entry and no recorded test run.
6. Model Plane asks: 7 OPEN, 3 PARTIAL. PLAN.md is stale and the ADR-1 ledger entry is missing.
7. Owner decisions are unchanged: ZDR quarry token versus durable jobs (E3.7.10-CP), SCHEDULE_POSTURE default `observe`, OD13 counsel, Data Plane artifacts lookup (E3.8.8), X-19 receipt default.
8. Stack proof needs the user: SC1 to SC7 are 0 of 7, and W3 is 0 of 58 stack-proven.

## 4. Next slice

**CW-3, eval credential (lane c, HIGH).** It is the only HIGH slice whose files nobody holds.

- Files (exclusive): `apps/Ingestion Plane/Quarry-v2/lab/evals/src/provider_matrix.rs` and `.github/workflows/quarry-v2-eval-tier-c.yml`.
- Do: send the Browserless token in a header (or in a body field Browserless accepts) instead of `?token=`. Strip any URL and query from error text before it goes into the report. Add a unit test showing that neither the report nor the error string contains the token value. Then let the workflow upload the provider-matrix report again, or keep it excluded and update the comment.
- Guard: `cargo test -p quarry-evals` (92 tests at the last gate) and `cargo check` on the evals crate. Add a W23-STATUS batch row. Close D-E3967-1 in `W23-depth-lane-c.md` (append only).
- Before it starts: `git -C .claude/worktrees/innh-lane-c merge --ff-only w23/integ`.

In parallel, if there are more writers:

- **VF-1 for CW-1** (no file overlap): `cargo test -p quarry-edge crawl_scrape` plus the crawl request tests, and a W23-STATUS row for 9da2c64b8.
- **After the staged writer commits:** CW-1 remainder (`routes.rs` → `run_with_inline_request_redacting`, the page_renderer skip and PiiEnforcingStore, the quiet panic hook), then CW-2 (OTP strip before store and before ObservationReady, the delegate_step read-denial check).
- **Integrator, one commit, when either integ or model-plane next goes to main:** the 0.1 admission-golden regeneration.
- **Model Plane session:** the asks in 2.3, the PLAN.md block, and the ADR-1 ledger entry.
