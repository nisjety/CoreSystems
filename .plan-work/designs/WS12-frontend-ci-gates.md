# WS12-frontend-ci-gates - UI quality gates and contract/architecture guardrails in CI

Date 2026-10-15. Read-only design. Nothing was built, linted or deployed; the only commands run were greps, `wc`, `git --no-optional-locks` reads, one python scan of global.css and read-only `gh` queries (GitHub API, runs and branches).

Aliases (repo-relative; quote paths, they contain spaces): **FE** = `apps/Frontend Plane/verevonv3`; **MG** = `apps/Model Plane/rust/services/model-gateway`; **MP** = `apps/Model Plane`; **W** = worktree at HEAD afb83928; **L** = live tree `C:/dev/CoresSystem` (HEAD plus ~324 uncommitted entries). Tags: **[live-only]** exists only uncommitted in L; **[M-live]** modified in L.

## 1. Scope, items resolved, owner

Items: **F-t3-066** (remove Norwegian-only controls; enforce i18n and design tokens in CI) and **F-t3-067** (field-level SSE contract between Rust and the SPA; plane-boundary gate; log-content gate; size ratchet). Wave W1, effort S + L per audit; this design re-estimates 27-35 engineer-days in 12 slices (section 8).

**Owner.** Frontend Plane owns the UI gates, the ESLint config, the SPA contract test and generated types (root CLAUDE.md "Architecture Rules"). Model Plane owns the SSE producer and its golden (`MP/docs/capability-ownership-matrix.md:125` "Public invoke, stream" = model-gateway). Repo-root gates (`scripts/`, `.github/workflows/`) have **no plane owner**: `apps/master-ownership-matrix.md` has no row for them (sweep section 5; only CI mention is the evals row). Proposed owner: Frontend Plane maintainers for the SPA gates, Model Plane for the payload golden, and one named maintainer for the new `scripts/gates/` (decision D2).

**IN.**
- Make the existing CI actually run where work lands (S1) and make the buf gate real (S2).
- Fix the named Norwegian-only and English-only sites (S3); a scoped ESLint gate for bare UI copy and raw colours with a bulk-suppression baseline (S4); an English-locale leak smoke helper and a per-surface state checklist (S5).
- Extend the SSE taxonomy gate to the second emitter family (S6); Rust-generated payload golden, vendored and pinned (S7); SPA read checker plus generated TS payload types (S8).
- Plane-boundary gate (S9), log-content gate (S10), file size ratchet (S11), stack-free browser-fixture job and bundle budget (S12).

**OUT.** ICU or namespace catalogs and a key-parity test (audit drops them; inline `i18n.tr(no, en)` stays the convention, `FE/src/shared/i18n/index.tsx:22`). Converting `payload()` to serde structs (D3 rejects it for now). ts-rs/schemars/utoipa/specta. A Model Plane clippy gate (kept out on purpose, `.github/workflows/model-plane-rust-ci.yml:121-129`; optional ratchet in D11). Function-level size limits (deferred, D8). Raw-colour enforcement in CSS beyond a count test (no stylelint). Gating `features/spaces`, `features/dashboard/home` (BrowserChrome) until their owning sessions opt in (section 6). Fixing FleetRunConsole's unrouted `/v1/fleets` calls (found, not owned here).

**Split.** [FINISH] S1, S2, S3, S6 (extend what exists). [BUILD] S4, S5, S7, S8, S9, S10, S11, S12.

## 2. Verified current state

### 2.1 CI reality (the most important finding; the audit does not mention it)

| Fact | Evidence |
|---|---|
| The GitHub default branch is `dev`, last commit 2026-03-29, and has **no** `.github/workflows` | `gh repo view` -> `dev`; `gh api repos/nisjety/CoreSystems/branches/dev` -> 461b155a, 2026-03-29; `gh api .../contents/.github/workflows?ref=dev` -> 404 |
| The repo's whole Actions history is **3 runs, all `SSE taxonomy freshness` push failures** (2026-09-23 to 2026-10-11) | `gh api repos/nisjety/CoreSystems/actions/runs?per_page=100` returns 3 runs; `gh workflow list --all` lists one workflow. `gh run list --workflow=verevonv3-ci.yml` / `model-plane-rust-ci.yml` / `contract-goldens.yml` / `buf.yml` -> HTTP 404 "not found on the default branch" |
| No PR has ever been opened; work lands by pushing branches | `gh api .../pulls?state=all` returned nothing |
| `verevonv3-ci.yml`, `buf.yml`, `quarry-v2-ci.yml`, `verevonv3-security.yml` fire only on PR to or push to `main` (or `[main, develop]`); the active integration branch is `fix/ui-premium-restore` (origin/HEAD, 45 commits ahead of `origin/main`) | `.github/workflows/verevonv3-ci.yml:10-21` (`pull_request: branches: [main]`, `push: branches: [main]`); `buf.yml:3-12`; trigger scan of all 19 workflows; `git rev-list --count origin/main..origin/fix/ui-premium-restore` = 45 |
| `model-plane-rust-ci.yml`, `contract-goldens.yml`, `dataplane-v2-ci.yml`, `secret-scan.yml` take PRs to any base, but pushes only on `main` | trigger scan (`pull_request:` with no `branches`) |
| Scheduled jobs (`verevonv3-security` weekly `:19-20`, quarry cron, dataplane nightly) can only run from the default branch, which has no workflows | GitHub rule + `dev` has none; not run-verified |
| `sse-taxonomy.yml` is the only gate that runs (push, any branch). It fails at HEAD | 3 failed runs; `.plan-work/common/migrations-protos.md` section 5: `--check` exits 1 in W (doc lacks `plan_proposed`, `source_scope_applied`), exits 0 in L (`sse taxonomy up to date (23 events)`) |

Consequence: every "CI gate" in this design is advisory until S1 lands. Gates must also be **independent of pre-existing red** (memory: `pnpm typecheck`/`lint`/`vitest` have pre-existing failures; not re-run here), so each new gate is its own job or step with its own baseline.

### 2.2 F-t3-066: UI copy and tokens

- Locale API: `tr: (noText, enText) => string` (`FE/src/shared/i18n/index.tsx:22`, default `:32`, provider `:62`); `defaultLocale = 'no'` (`FE/src/shared/i18n/locales.ts:3`). Norwegian-only leaks are therefore invisible in the default locale and in every test that renders without an `I18nProvider` (tests use the fallback, `index.tsx:25-33`). Existing locale tests set `window.localStorage.setItem(localeStorageKey,'en')` and wrap in `<I18nProvider>` (`FE/src/features/chat/components/ChatMessages.failure-actions.test.tsx:176-181`, `ChatMessages.read-aloud.test.tsx:166-170`): this is the pattern S5 generalises.
- Existing gate: `FE/src/features/chat/lib/chat-localization.test.ts` scans `src/features/chat/**/*.tsx` (not tests) for naming attributes (`aria-label`, `aria-description`, `alt`, `placeholder`, `title`; `:24`) with literal words (`HAS_WORDS :61`, scan root `:71-93`). It does not look at JSX text or string constants, which is where the leaks sit. Its header (`:16-18`) says "`pnpm lint` cannot run against the pinned TypeScript 7": **stale**. `FE/package.json` aliases `typescript` to `npm:@typescript/typescript6@^6.0.2` and pins `typescript-eslint ^8.69.0` (`:45-50`), and `pnpm-lock.yaml:89-103` resolves TS6 6.0.2, typescript-eslint 8.69.0, eslint 10.10.0. The checked-out `node_modules` in W is stale (eslint 10.8.1, TS 7.0.2, typescript-eslint 8.67.0). CI installs with `--frozen-lockfile` (`verevonv3-ci.yml:180`), so CI lint uses the lockfile versions.
- ESLint today: `FE/eslint.config.js` ignores `apps`, `packages`, worktrees (`:44`); one product-source block for `src/**/*.{ts,tsx}` excluding tests (`:71-72`) holds `no-restricted-imports` (demo/mock modules), a single `no-restricted-syntax` array (A1 posture selector plus `...WEB_SPEECH_SYNTAX`, `:93-106`; comment `:104-105` says a second key replaces the first), `no-restricted-globals :112`, `no-restricted-properties :113`. No JSX-literal rule, no raw-colour rule, no stylelint. A precedent exists for proving a lint rule fires from vitest against the real config: `FE/src/features/voice/web-speech-guard.test.ts:10-24` (`new ESLint()`, `lintText`, asserts every `no-restricted-syntax` message matches `/governed speech path/`; S4 must keep its fixtures free of our new messages) and `FE/src/shared/read-data/fabrication-guard.test.ts`.
- Verified bare literals (W lines, all opened). [M-live] ChatMessages.tsx is +229 lines in L, so these are +2 in L (Godkjenn L:1917); all other files below are clean in L.

| File (under FE/src/features) | Lines | Literal |
|---|---|---|
| chat/components/ChatMessages.tsx | 1915, 1922 | `Godkjenn`, `Avvis` buttons in `ApprovalRequests` (`:1887`; `i18n` already in scope `:1888`) |
| same | 2521 | `{props.pending ? 'Godkjenner…' : 'Godkjenn'}` in `PlanApprovalControl` |
| same | 2358 | `Godkjent: agenten kan nå {RUNG_LABEL[...]}.` bare JSX in the fallback |
| same | 2534-2540 | `GRANTABLE_RUNGS`, `RUNG_LABEL` (module-level Norwegian record, used `:2358`, `:2450`) |
| same | 1642-1645 | tab labels `Oversikt`, `Innsikt`, `Verktøy`, `Kilder` in a `createMemo` array (non-JSX data) |
| chat/components/ChatArtifactPanel.tsx | 354, 397, 581, 607 | `Vis forhåndsvisning`/`Vis kode`, `Last ned` (x2, `ArtifactFileCard :559` has no `useI18n`), a Norwegian sentence in `ArtifactLoadFailure` (`:600`, whose title already uses `i18n.tr`) |
| chat/components/ChatLiveRunPanel.tsx | 69, 352-356, 473-476 | `RUN_STREAM_ERROR`, activity titles `Pauset for godkjenning`, status words `Kobler til …`/`Avbrutt`/`Fullført`/`Kjører` (constants and state, not JSX) |
| agents/components/FleetRunConsole.tsx | 155, 159 | `Pause fleet` and the English note; the file has no `useI18n` |

  Reference pattern already in-repo: `agents/components/AgentRunConsole.tsx:1320,1331,1355` (`i18n.tr('Godkjenn','Approve')`, `i18n.tr('Avvis','Reject')`).
- Existing tests that assert the Norwegian text under the default locale and must keep passing: `ChatArtifactPanel.test.tsx:198,219` ('Last ned'), `ChatPanels.test.tsx:795,805` (rung labels), `ChatMessages.chart.test.tsx:104,207`, `ChatMessages.generated-files.test.tsx:56`, `spaces/components/SpaceApprovalPanel.test.tsx:40,57` (Space, out of scope). No test queries the chat approve/reject buttons or `Pause fleet` by name.
- Counts: `i18n.tr(` 4,650 occurrences in src (4,631 non-test; sweep), not "about 984". Raw hex: `#[0-9a-fA-F]{3,8}\b` appears on 262 lines in features/agents (541 occurrences by the sweep) and on **1** line in features/chat (`chat/lib/chat-export/print.ts`); 422 lines in 39 files across all features (my Grep). A naive scan of `FE/src/styles/global.css` (python, comments stripped, custom-property declarations excluded) finds 2,114 colour literals, 65 of them in 48 `.verevon-chat*` rules (not an AST scan). Inline same-line text between tags: 275 lines in 68 files (my Grep, includes tests).
- State checklist: none exists. The UX spec lives at `FE/apps/verevon-web/plans/chat-workspace/VEREVON_CHAT_DESIGN.md`; section 9.3 criterion 9 "Norwegian and English labels switch together" is marked NOT met (accessible names). `FE/apps/verevon-web/DESIGN.md:1-5` is the marketing-site spec (card is right).
- FleetRunConsole is routed (`FE/src/app/App.tsx:206`) but its three `fetch('/v1/fleets/...')` calls (`FleetRunConsole.tsx:27,33` and the PATCH near `:146`) hit a path with no BFF route (no `fleets` hit in `FE/apps/gateway/src`, `vite.config.ts`, `nginx.conf`) and the button PATCHes `status: 'cancelled'` while labelled "Pause" with a note claiming cascade. Not determined whether Quarry is reachable by another proxy.

### 2.3 F-t3-067: contracts and architecture

- Taxonomy gate (`gen-sse-event-taxonomy.py`, `MP/scripts/`): event-name coverage only. `name()` and `family()` arms parsed by regex (`:81-83`), SPA `case '…':` labels by regex over the whole file (`:130-134`), alias table `NON_CHATEVENT_CONSUMER_CASES :43-49` (5 names). No payload logic. Workflow `.github/workflows/sse-taxonomy.yml:44-62` runs `--check` plus a drift proof.
- **Not every SPA event is covered.** chat-client.ts has 33 distinct `case '…':` labels in L (my grep; the sweep counts 42 raw labels in W); the generator knows 23 `ChatEvent` names (L) + 5 aliases. The other SPA cases (`awaiting_approval`, `approval_state_changed`, `approval_continuation_verified`, `run_paused_for_approval`, `run_resumed_after_approval`, `browser_action_approval_required`, `browser_action_decided`, `retrieval_notice`...) come from two sources the script never reads: the proto-typed `orchestration_event::Event` oneof (`MP/proto/model_plane/v1/orchestration.proto:1033,1036,1042`; name mapping `MG/src/sse.rs:9854-9864` in W; line numbers differ in L) and ad-hoc `Event::default().event("awaiting_approval")` (`sse.rs:9897`, L). Distinct `.event("…")` literals in MG src (L): `done`, `chunk`, `connected`, `step_update`, `error`, `awaiting_approval`.
- Payload construction: `ChatEvent::payload()` hand-builds `json!` with conditional inserts and `serde_json::to_value` for structs (L `MG/src/sse_events.rs:490-560`; W `:425`). Conditional keys (`source_group_label`, `number`, `claim_id`, `start`, `end`) are omitted rather than null. The only payload assertions are `names_and_payload_shapes_are_stable` (usage and citation; W `:1507-1545`, L `:1602`). `ChatEvent` is `pub` in the lib (`MG/src/lib.rs:82`) so an integration test or an in-file test can instantiate it.
- SPA reads: `dispatchEvent` parses `payload` and switches on `event.event` (`FE/src/shared/api/chat-client.ts:1430-1456`), reading `payload.<key>` per case with many camelCase and legacy aliases (`modelUsed`, `tool_call_id`, `call_id`, `result`, `content`...). Helpers outside the switch also read payloads (`emitCitation`, `failureTiming`). `onUnknownEvent` is the runtime fallback.
- Golden chain precedent (reuse it): `scripts/tests/zdr-contract-sync.test.sh` holds `GOLDENS` (`path|sha256`), `VENDORED` (`copy|golden`), `CONSUMERS`, a `--self-test`, and a CR/LF check (goldens are LF only; `.gitattributes:24-29` pins `*.ts/*.tsx/*.js/*.mjs` but **not** `*.json`). Generators are test-gated (`MP_UPDATE_GOLDEN=1`, e.g. `mp-contracts` `zdr_refusal.rs:991` [live-only]). `contract-goldens.yml` path filters name every golden by file name.
- **buf is not a working gate** (audit says "buf breaking runs": wrong). `MP/proto/baseline.binpb` is **0 bytes** (`ls -la`); `MP/docs/decisions/ledger.md:2479-2486` records buf lint at 1,502 errors (1,428 COMMENTS, 74 other) and the 0-byte baseline making `buf breaking` hard-error "image contains no files"; `.github/workflows/buf.yml:21,23` passes the space-containing path unquoted (buf takes one input, so the shell splits it; not run). `MP/go/pkg/cibuf/workflow_test.go:69-79` only checks the YAML text contains `buf lint`, `buf breaking`, the path and `baseline.binpb`.
- `lint-ratchet.mjs` (`apps/Control Plane/auth-core/scripts/`) is **not** hash-baselined (audit wrong): ESLint bulk suppressions (`--suppressions-location`, `:25`; `.eslint-clean-suppressions.json :191`, `eslint-suppressions.json :229`), a changed-file check that the baseline excludes changed files (`:31-37`). Good precedent for S4, wrong precedent for sizes (suppressions record that a violation exists, not its magnitude).
- Sizes (W, tracked files, my `wc -l`): 45 files >= 3,000 lines including generated ones. Hand-written leaders: `FE/src/styles/global.css` 48,984; `agent.rs` 13,328; `tool_loop.rs` 12,717; `sse.rs` 12,580; `session-core/src/grpc.rs` 12,568; `http_routes.rs` 11,538; `model-gateway/tests/e2e_invoke_chain_test.rs` 10,795; `FE/apps/gateway/src/domains/spaces.rs` 9,882; `source_validation.rs` 7,330. L is larger (agent.rs 13,521, sse.rs 13,325, http_routes.rs 11,681; [M-live]). Generated and to exclude: `MP/go/gen/**/*.pb.go`, `apps/Control Plane/auth-core/src/grpc/proto/user/v1/user.ts`, `apps/Frontend Plane/verevon/src/lib/model-plane/gen/**`.
- Plane DB boundary, syntactic reality: every plane's compose files use only plane-local Postgres hosts (MP `postgres`; IP `ingestion-postgres`, `connector-runtime-db`; CP `controlplane-postgres`; AP `application-postgres`; DP `postgres`); the only path-style cross-plane dependency in manifests is `FE/package.json:23` `@quarry/client` `file:../../Ingestion Plane/Quarry-v2/sdks/typescript` (the generated SDK, not a DB). Intra-plane: execution-core, orchestrator-core, capability-core, letta-bridge, cost-core all point `DATABASE_URL` at session-core's `session_core` database (`MP/deploy/docker-compose.yml:1075,1310,1388,1653,1794`); that is inside one plane and not what the gate targets (flag: whether that is intended is not determined).
- Logging: 903 `info!/warn!/error!/debug!/trace!` lines in `MP/rust` (my Grep). A one-line heuristic for content-named values found 5 hits, all error-message values (`speech.rs:228,259`, `retrieval.rs:1065`, `agent.rs:3388`, `tool_loop.rs:6752`): false positives, multi-line macros are missed. Runtime precedent: `agent.rs:8797` asserts provider text is not logged for a pinned run; docs precedent `docs/VERIFICATION.md:485`.

### 2.4 Live-only inputs this stream depends on (land first, WS01 S0)
`MP/api/contracts/zdr-refusals.json(+.sha256)`, `mp-contracts/src/zdr_refusal.rs`, FE `src/shared/api/contracts/zdr-refusals.json(+.sha256)`, `zdr-refusals.ts`, the `*.zdr-refusal.test.ts` files, `scripts/tests/zdr-contract-sync.test.sh` (+95 lines [M-live]), `MG/src/sse_events.rs` (+203: `RetentionClass` L `:108`, `RetrievalNotice`, `ZdrError`), `sse.rs` (+883), `MP/docs/sse-event-taxonomy.md` (+4 rows), `.github/workflows/verevonv3-ci.yml` (+55, job `demo-product-build`), `model-plane-rust-ci.yml` (+50, job `session-core-receipts`), `FE/package.json` (+2). **S7/S8 must not branch before the sse_events/taxonomy/zdr-contract set is committed**, otherwise the golden is generated from a tree that differs from HEAD (HEAD's taxonomy check is red).

### 2.5 Where the audit text is stale or wrong
| Audit | Code |
|---|---|
| about 984 `i18n.tr` strings | 4,650 occurrences |
| 15-20 leaking sites | more; leaks also in constants, state and data tables the card's JSX selectors cannot see (`RUNG_LABEL`, `RUN_STREAM_ERROR`, tab arrays) |
| about 537 hex literals in agents | 262 lines / 541 occurrences; chat has 1 |
| lint-ratchet "baselined by hash" | ESLint suppressions |
| `buf breaking` runs | 0-byte baseline; unquoted path; workflow not on the default branch |
| event-name coverage "is already a CI gate" | gate fails at HEAD, never ran green on GitHub, and misses non-ChatEvent emitters |
| "`pnpm lint` cannot run on TS 7" (test comment, sweep conflict) | lockfile pins TS6; stale node_modules only |
| three size files | 45 files >= 3,000 lines; generated files must be excluded |

## 3. Decisions and open questions

| ID | Question | Options | Recommendation | Gates | Proceeds without it |
|---|---|---|---|---|---|
| D1 | CI reachability: which branch is the integration branch; make workflows run on it; change the repo default branch from `dev` | (a) drop `branches` filters on `pull_request` (any base) and add the integration branch to `push`; (b) retarget to `main` only; (c) also change the GitHub default branch to the integration branch so scheduled and manual runs work | (a) now; (c) as an owner action (repo setting, cannot be done from here). New workflows use `pull_request` with no branch filter | S1 (all later gates are advisory without it) | S3, S5-S10 code can be written; only enforcement waits |
| D2 | Owner of repo-root gates (`scripts/gates/`, `.github/workflows/repo-gates.yml`) | none today; add a matrix row; CODEOWNERS | Add a row to `apps/master-ownership-matrix.md` and a CODEOWNERS entry; one named maintainer | S9-S11 merge rights | all coding |
| D3 | Payload contract mechanism | (a) extend the python generator to parse `payload()` arms; (b) Rust test instantiates every variant (minimal and full) and writes a golden of key sets, types, presence; (c) convert payloads to serde structs plus schemars | (b): robust to conditional inserts and `to_value`; no new crate; same chain as the ZDR goldens. (a) cannot follow `if let Some` inserts reliably; (c) is a large refactor of a hot file and is not needed to close the gap | S7, S8 | S6 |
| D4 | UI copy gate mechanism | `no-restricted-syntax` selectors (audit) vs a 60-line local ESLint rule | Selectors first, re-spreading the existing array; spike in S4 proves esquery 1.7.0 accepts the regex `u` flag and `:not(A > B)`; fall back to a local rule only if it does not (the existing speech guard already proves selectors with regexes work). Baseline with ESLint bulk suppressions | S4 | S3 |
| D5 | Gate scope | chat+agents only (audit) vs all `src/features/**` | chat+agents now, as an exported `UI_GATE_SCOPES` array so Space and Browser sessions opt in by editing one line; **plus** the rule applies at error level to any file newly added anywhere in `src/` (changed-file check) | S4 | all |
| D6 | `Pause fleet` | (a) keep wording, translate; (b) relabel to "Cancel fleet" because the call sends `status: cancelled`; (c) implement pause | (b) in S3, and drop the cascade note; product may still want (c) as a separate item. Not decidable from code whether Quarry supports pause | S3 one line | rest of S3 |
| D7 | buf baseline method | committed `baseline.binpb` regenerated deliberately vs comparing to the base branch by git ref | Committed baseline regenerated at S2 from the integration branch (keeps `cibuf` workflow test and README valid); lint via a ratchet on the JSON error list, not "zero errors" (1,502 today) | S2 | all except S2 |
| D8 | Size ratchet thresholds | default cap, per-file ceiling, function-level | Default cap 3,000 lines for hand-written Rust/Go/TS/TSX/Python/CSS; every file over it gets a ceiling equal to its current length; baseline can only shrink and is read from the base ref so a PR cannot relax it. Function-level deferred until S11 measures function-length distribution (not determined) | S11 | all |
| D9 | Log-content denylist and language scope | Rust only vs Rust+Go | Rust first (model-gateway, execution-core, inference-core, session-core), Go in a second pass of the same script after the Rust baseline is reviewed | S10 | all |
| D10 | Does any existing ledger entry forbid these gates | check `docs/decisions/ledger.md` | none found by grep (i18n, ratchet, eslint, ts-rs, schemars, codegen, size gate: no relevant hit). No reversal needed. The model-plane clippy omission is a workflow comment, not a ledger entry | none | all |
| D11 | Model Plane clippy ratchet (audit lists "Model Plane clippy" as missing) | keep omitted (304 pedantic warnings) vs ratchet on count per lint | Defer; if wanted, `cargo clippy --message-format=json` counts per lint name against a committed baseline, as an optional step of S11. Not in the critical path | none | all |
| D12 | Product wording for the new English strings (`Approve`, `Reject`, rung labels, `Download`) | reuse AgentRunConsole's wording | reuse (`AgentRunConsole.tsx:1320-1355`), rung labels: Read only / Write in the workspace / Do anything, including outbound actions | S3 | n/a |

Open (not blocking): whether intra-plane sharing of the `session_core` database by five Model Plane services is intended (outside this gate); whether the Space session wants `features/spaces` gated before its own cleanup.

## 4. Slices (PR-sized, in build order)

Rollout convention for every gate: first merge with the job `continue-on-error: true` for the first 3 runs (or ESLint `warn`), record the failing set, then flip to blocking; rollback = revert the workflow hunk (no runtime flag exists or is needed). ZDR: gates persist nothing; any fixture is synthetic text; no ZDR golden-matrix case is added except where S7 notes it.

### S1 - Make CI run where the work lands [FINISH] (S, 1-2 d)
- **Goal.** Existing and new gates execute on pushes and PRs to the integration branch, and the current red set is recorded.
- **Changes.**
  - `.github/workflows/verevonv3-ci.yml` (`:11-12` pull_request filter; the `push:` block below it) - remove the `branches: [main]` filter on `pull_request`; add the integration branch to `push` (D1).
  - `.github/workflows/buf.yml:3-12` - same (currently `[main, develop]`).
  - `.github/workflows/verevonv3-security.yml`, `quarry-v2-ci.yml` - same filter change only (they are FE/IP-adjacent and also never trigger on the integration branch); keep their jobs unchanged.
  - NEW `FE/docs/CI_BASELINE_2026-10.md` - table of every job's first-run result and owner of each pre-existing failure (filled from the first runs; not determined today whether `pnpm verify`, `gateway-rust` clippy or `docker-image` pass).
  - NEW `.github/workflows/repo-gates.yml` (empty shell with the trigger block used by S9-S11: PR any base, push integration branch and main, paths `scripts/gates/**`, `scripts/tests/**`, `apps/**`).
- **API/proto/migration.** none. **Flag.** none; rollback is a revert.
- **Tests.** Push a throwaway branch; `gh run list` must show the workflows. Negative proof: a PR with a deliberate `pnpm typecheck` error turns `spa-verify` red.
- **DoD.** Each of verevonv3-ci, model-plane-rust-ci, contract-goldens, buf, sse-taxonomy has at least one recorded run on the integration branch; CI_BASELINE lists reds with owners; D1(c) requested from the repo owner.
- **Evidence.** `gh run list` output and run URLs for each workflow; the baseline table.
- **Deps.** D1; WS01 S0 (sse-taxonomy red at HEAD is fixed by landing the live doc rows). **Risks.** First runs are red for pre-existing reasons (memory: 12 vitest files failing, typecheck missing `@quarry/client`; not re-verified) and may be misread as regressions: mitigate with the baseline doc and per-gate independence.

### S2 - Repair the buf gate [FINISH] (M, 2-3 d)
- **Goal.** `buf breaking` compares against a real baseline; `buf lint` fails only on new violations.
- **Changes.**
  - `.github/workflows/buf.yml:21,23` - quote paths: `buf lint "apps/Model Plane/proto"`; `buf breaking "apps/Model Plane/proto" --against "apps/Model Plane/proto/baseline.binpb"`; add a lint-ratchet step.
  - `MP/proto/baseline.binpb` - replace the 0-byte placeholder with `buf build "apps/Model Plane/proto" -o ...` from the integration branch (README command, `MP/proto/README.md:10`). Buf is not installed on the host (memory): generate with `docker run --rm -v "$PWD:/w" -w /w bufbuild/buf build ...`. [live-only] note: L's `sessions.proto` is +42 lines (additive `SessionRetentionService`); build the baseline from the committed state, never from the dirty tree.
  - NEW `scripts/gates/buf-lint-ratchet.py` - reads `buf lint --error-format=json`, keys each error by (path, type, message), fails on any key not in `MP/proto/buf-lint-baseline.json`; `--update` rewrites; stale entries reported.
  - NEW `MP/proto/buf-lint-baseline.json` (generated; 1,502 entries expected per ledger `:2479`, to be re-counted).
  - `MP/go/pkg/cibuf/workflow_test.go:69-79` - extend to require quoted paths and the ratchet step (text check stays a text check).
  - `MP/proto/README.md` - document the baseline refresh as a reviewed step.
- **Tests.** `cd "apps/Model Plane/go/pkg/cibuf" && go test ./...`; `python3 -m unittest scripts/tests/test_buf_lint_ratchet.py` (NEW; fixture JSON with a new error, a stale entry, an unchanged set); CI self-proof step: remove a field from a copy of a proto and assert `buf breaking` fails (same pattern as `sse-taxonomy.yml:49-62`).
- **DoD.** `buf breaking` red on a deliberately breaking proto change and green on an additive one; `buf lint` green on the current tree, red on a new violation.
- **Evidence.** Two CI runs (red proof, green) and the baseline file diff.
- **Deps.** D7, S1. **Risks.** Baseline taken from a tree that later changes (additive passes; document refresh); WS04/WS09/WS17 add proto fields concurrently (additive passes the gate; they must not regenerate the baseline).

### S3 - Fix the named language leaks [FINISH] (S, 1-2 d)
- **Goal.** Approval, plan-grant, artifact-download and fleet controls follow the locale; no behavioural change.
- **Changes** (all under `FE/src/features`; W lines, +2 in L for ChatMessages.tsx; land after WS01 S0 so the [M-live] file is the base):
  - `chat/components/ChatMessages.tsx:1915,1922` - `i18n.tr('Godkjenn','Approve')`, `i18n.tr('Avvis','Reject')` (pattern `AgentRunConsole.tsx:1320,1331`).
  - same `:2521` - `props.pending ? i18n.tr('Godkjenner…','Approving…') : i18n.tr('Godkjenn','Approve')`.
  - same `:2358,2534-2540` - replace `RUNG_LABEL` with `rungLabel(rung, i18n.tr)`; `Godkjent: agenten kan nå {…}.` -> `i18n.tr('Godkjent: agenten kan nå …', 'Approved: the agent can now …')` with the label interpolated (two full sentences, not concatenation).
  - same `:1642-1645` - tab labels through `i18n.tr` (`Oversikt/Overview`, `Innsikt/Insight`, `Verktøy/Tools`, `Kilder/Sources`).
  - `chat/components/ChatArtifactPanel.tsx:354,397,581,607` - `i18n.tr` pairs; add `const i18n = useI18n()` to `ArtifactFileCard (:559)`; translate the `ArtifactLoadFailure` sentence (`:607`).
  - `chat/components/ChatLiveRunPanel.tsx:473-476` - status words through `i18n.tr` at render (confirm the helper at `:472` sits inside the component; `i18n` is used at `:233`); `:352-356` and `:69` store strings in run state: translate at render by mapping the existing `kind: 'pause'` / error code, not by storing translated text (only if cheap; otherwise leave to the S4 baseline and say so in the PR).
  - `agents/components/FleetRunConsole.tsx:155,159` - add `useI18n`; relabel per D6 (default: `Avbryt flåten / Cancel fleet`, note removed).
- **API.** none. **Flag.** none (copy only). Rollback: revert.
- **Tests.** NEW `chat/components/ChatMessages.approval-locale.test.tsx` (ApprovalRequests and PlanApprovalControl under `en` and `no`: button names, rung labels, granted sentence, pending label); extend `ChatArtifactPanel.test.tsx` (EN `Download`, `Show preview`/`Show code`; NO assertions at `:198,219` stay), `FleetRunConsole.test.tsx` (label in both locales); `ChatPanels.test.tsx:795,805` must still pass (default locale). Command: `cd "apps/Frontend Plane/verevonv3" && pnpm exec vitest run src/features/chat/components/ChatMessages.approval-locale.test.tsx src/features/chat/components/ChatArtifactPanel.test.tsx src/features/chat/components/ChatPanels.test.tsx src/features/agents/components/FleetRunConsole.test.tsx` then `pnpm typecheck`.
- **DoD.** The listed sites render English under `en` and the old Norwegian under `no`; existing tests green; a screen reader name check: `getByRole('button',{name:'Approve'})` under `en`.
- **Evidence.** Rebuilt frontend (second dev server `pnpm dev --port 5174 --strictPort`; :5173 serves the main checkout, memory) with locale toggled to English: screenshots of a chat approval block, the plan-grant control (both rungs), an artifact file card and the fleet console. Dev servers over the Docker bind mount need a restart to see host edits (memory); a production image build (`verevonv3-ci.yml:215-223` form) for the final check.
- **Effort.** S, 1-2 d. **Deps.** WS01 S0 (ChatMessages.tsx base); D6, D12. **Risks.** Merge conflicts in ChatMessages.tsx (+229 live; WS04/WS09/WS14 also edit): keep hunks tiny, edit before them (section 5).

### S4 - UI copy and raw-colour gate with baseline [BUILD] (M, 3-4 d)
- **Goal.** New bare UI copy or raw colours in `features/chat` and `features/agents`, and in any newly added file under `src/`, fail CI; existing violations are baselined and can only shrink.
- **Changes.**
  - NEW `FE/eslint.ui-gates.config.js` - exports `BASE_RESTRICTED_SYNTAX` (the A1 selector and `WEB_SPEECH_SYNTAX`, moved out of `eslint.config.js:25-31,93-106`), `UI_GATE_SCOPES = ['src/features/chat/**','src/features/agents/**']`, `UI_COPY_SYNTAX`, `UI_COLOR_SYNTAX`, and a standalone default export (parser setup `tsconfigRootDir`, no typed linting) so the gate runs without the rest of `pnpm lint`.
  - `FE/eslint.config.js` - import the above; the scoped block re-spreads `BASE_RESTRICTED_SYNTAX` before the UI selectors (a second `no-restricted-syntax` key replaces the first, `:104-105`), excludes `**/*.test.*`.
  - Selectors (D4), messages say "route through `i18n.tr(no, en)`":
    1. `JSXText[value=/\p{L}{2,}/u]`;
    2. `JSXAttribute[name.name=/^(title|aria-label|aria-description|alt|placeholder)$/] > Literal[value=/\p{L}{2,}/u]` and `... > JSXExpressionContainer > ConditionalExpression > Literal`;
    3. `JSXElement > JSXExpressionContainer > :matches(ConditionalExpression, LogicalExpression) > Literal[value=/\p{L}{2,}/u]`;
    4. `Property[key.name=/^(label|title|text|message|detail|description|placeholder|heading|caption|tooltip|hint)$/] > Literal[value=/\p{L}{2,}/u]` (data tables such as the tab array);
    5. `Literal[value=/[æøåÆØÅ]/]` and `TemplateElement[value.cooked=/[æøåÆØÅ]/]` not directly inside an `i18n.tr`/`tr` call (`:not(CallExpression[callee.property.name='tr'] > Literal)`; **spike**: confirm esquery 1.7.0 supports it and the `u` flag, else implement 1-5 as `eslint-rules/ui-copy.js`).
    6. colours: `Literal[value=/#[0-9a-fA-F]{3,8}\b|\b(rgb|rgba|hsl|hsla)\(/]` and the matching `TemplateElement`, message "use a design token (`var(--…)`)". Applies to chat (1 existing hit) and agents (262 lines, baselined).
  - NEW `FE/eslint-suppressions.json` - generated by `pnpm exec eslint -c eslint.ui-gates.config.js --suppress-all src` (ESLint >= 9.24 bulk suppressions; same mechanism as `lint-ratchet.mjs:25`); ESLint reads `eslint-suppressions.json` from the cwd by default. CI uses `--prune-suppressions` check so removed violations must be removed from the file.
  - NEW `FE/scripts/ui-gates-new-files.mjs` - `git diff --name-only --diff-filter=A <base>...HEAD -- 'src/**/*.tsx'` then runs the standalone config over those files with **no** suppressions file, applying the scopes `src/**` (D5), excluding tests. Mirrors `lint-ratchet.mjs` changed-file mode.
  - `.github/workflows/verevonv3-ci.yml` - NEW job `spa-ui-gates` (own `actions/checkout` fetch-depth 0, pnpm 11.3.0, Node 22, the Quarry SDK generate step `:171-177`, `pnpm install --frozen-lockfile`, `pnpm exec eslint -c eslint.ui-gates.config.js src`, then `node scripts/ui-gates-new-files.mjs`). A separate job so pre-existing red in `spa-verify` (lint/typecheck/vitest) cannot hide or block it.
  - `FE/src/features/chat/lib/chat-localization.test.ts` - header comment corrected (`:16-18`); keep the test (it covers `title`/`alt` expression forms the selectors do not) and note the overlap.
  - NEW `FE/src/features/chat/lib/ui-gates.guard.test.ts` - vitest, runs the real config with `new ESLint()` (pattern `web-speech-guard.test.ts`): fixtures for each selector fire; `i18n.tr` arguments, `class=`, `data-*`, `type=` and a locale-neutral `Verevon` (via `// eslint-disable-next-line no-restricted-syntax -- locale-neutral: product name`) do not; a fixture under `src/features/voice/` is unaffected.
- **API/data.** none. **Flag.** rollout as above (warn for 3 runs, then error).
- **Tests.** `cd "apps/Frontend Plane/verevonv3" && pnpm exec vitest run src/features/chat/lib/ui-gates.guard.test.ts src/features/voice/web-speech-guard.test.ts src/shared/read-data/fabrication-guard.test.ts src/features/chat/lib/chat-localization.test.ts` (the three existing ESLint-driven tests must stay green) and `pnpm exec eslint -c eslint.ui-gates.config.js src`. ESLint cold start is about 8 s (`vite.config.ts` comment; per-test timeout is 30 s).
- **DoD.** Gate green on the integration branch with the baseline; adding `<button>Lagre</button>` to a chat component turns `spa-ui-gates` red; removing a baselined leak without pruning the file turns it red; the baseline count is recorded in the PR (dry-run `--format json` count; not determined today).
- **Evidence.** Two CI runs (red proof on a throwaway commit, green), the suppression file size, and the output of the dry-run.
- **Effort.** M, 3-4 d. **Deps.** S1, S3 (so the baseline excludes the fixed sites), D4, D5. **Risks.** esquery limits (spike); noisy false positives from selector 4 on non-copy `label` properties (baseline absorbs; tune by adding `:not` on `key` names); `eslint-suppressions.json` merge conflicts across sessions (conflict rule: regenerate with `--prune-suppressions`, never hand-edit).

### S5 - English-locale leak smoke and the state checklist [BUILD] (S-M, 2-3 d)
- **Goal.** Catch leaks the AST cannot (strings assembled from data) and give each surface a state checklist.
- **Changes.**
  - NEW `FE/src/test/locale-leak.ts` - `renderInLocale(locale, ui)` (sets `localeStorageKey`, wraps `<I18nProvider>`) and `expectNoNorwegian(container)`: fails on `[æøåÆØÅ]` and a short stop-list of Norwegian UI words (`Godkjenn`, `Avvis`, `Lukk`, `Last ned`, `Vis `, `Skjul`, `Lagre`, `Avbryt`, `Kjører`, `Fullført`). Not a dictionary, a smoke test; the stop-list is a const with a comment that an entry means "this word has no English meaning".
  - NEW `FE/src/features/chat/components/chat-locale-smoke.test.tsx` - renders `ApprovalRequests`, `PlanApprovalControl`, `ArtifactFileCard`/`ArtifactViewer`, the live panel status line and `ChatPanels` run-proof under `en` and runs `expectNoNorwegian`; mirror under `no` asserting no English stop-list words in the same surfaces (`Approve`, `Reject`, `Download`).
  - NEW `FE/docs/UI_STATE_CHECKLIST.md` - one table per surface kind (chat message, approval, artifact, run panel, settings page): loading, empty, error, partial/stale, ZDR-refused (typed refusal from `src/shared/api/zdr-refusals.ts`), offline, NO and EN, narrow viewport, reduced motion, keyboard, screen reader name. Links to criterion 9 (`VEREVON_CHAT_DESIGN.md` section 9.3) and to `chat-localization.test.ts`.
  - `FE/apps/verevon-web/plans/chat-workspace/VEREVON_CHAT_DESIGN.md` section 9.3 criterion 9 - update the state after S3/S4.
- **Tests.** `pnpm exec vitest run src/features/chat/components/chat-locale-smoke.test.tsx`. **DoD.** smoke fails when the S3 fix on `RUNG_LABEL` is reverted (mutation proof recorded in the PR). **Evidence.** the reverted-fix failing output and the green run.
- **Deps.** S3. **Risks.** Stop-list false positives on user content (render with fixtures only).

### S6 - Taxonomy gate covers the second emitter family [FINISH] (S-M, 2 d)
- **Goal.** Every SSE name the SPA handles has a known producer, and every name the gateway emits is known, including orchestration run events and ad-hoc names.
- **Changes.**
  - `MP/scripts/gen-sse-event-taxonomy.py` - add (a) `AD_HOC_PRODUCER_EVENTS` = names found by regex `\.event\("([a-z_]+)"\)` in `MG/src/**/*.rs` minus ChatEvent names, asserted equal to an explicit table with reasons (`done`, `chunk`, `connected` transport; `step_update`, `error` shared with ChatEvent; `awaiting_approval`); (b) parse `orchestration_event::Event::X(_) => "name"` (`sse.rs:9854-9864` W; locate by function in L) and the `OrchestrationEvent` oneof in `orchestration.proto` (`:1033-1042`); every SPA `case` must be in ChatEvent names, the alias table, the ad-hoc table or the orchestration set, and each orchestration name must have a SPA case or an explicit "not consumed by chat" entry. Also fix the known false positive in the script's own comment: a `case '…'` in the AG-UI switch passes the check (`migrations-protos.md` risk 11): restrict `consumer_cases` to the `dispatchEvent` native switch (from `switch (event.event)` to its `default:`).
  - `MP/docs/sse-event-taxonomy.md` - regenerated (new "Other producers" section); commit with the script.
  - `.github/workflows/sse-taxonomy.yml:16-30` - add `apps/Model Plane/proto/model_plane/v1/orchestration.proto` to both path lists; extend the drift proof to also delete an ad-hoc row.
- **Tests.** `python3 "apps/Model Plane/scripts/gen-sse-event-taxonomy.py" --check`; NEW `scripts/tests/test_sse_taxonomy.py` (unittest: unknown SPA case fails; renamed orchestration arm fails; comment inside arm list still parses). **DoD.** gate green on L state after WS01 S0, red for each of the three mutations above. **Evidence.** CI run on a push touching `sse_events.rs` plus the mutation outputs.
- **Deps.** WS01 S0 (the doc rows and `RetrievalNotice`/`ZdrError` land first). **Risks.** Regex parsing of `sse.rs` (12-13k lines) is brittle: parse only the one mapping function body; fail loudly on zero matches (the script's existing "parser broke, not the invariant" convention).

### S7 - Rust-generated SSE payload golden, vendored and pinned [BUILD] (M, 3-4 d)
- **Goal.** A key renamed or removed in `ChatEvent::payload()` fails CI before the SPA silently reads `undefined`.
- **Changes.**
  - `MG/src/sse_events.rs` tests module (W `:1046`, L `:1139`) - NEW `payload_contract` submodule: `fn samples() -> Vec<(ChatEvent /*minimal*/, ChatEvent /*full*/)>` for every variant; an exhaustiveness guard `fn covered(e: &ChatEvent) -> &'static str { match e { ... } }` with **no wildcard** so adding a variant is a compile error until it has samples (the repo's mutation-tested cross-service pattern, `MP/CLAUDE.md` "Code Style"); a test `sse_event_payload_golden` that builds `{event: {family, variants, keys: {key: {types:[...], presence:"always"|"sometimes"}}}}` from `payload("req-1")` of minimal and full samples (`error` merges `Error`, `SubscriptionError`, `ZdrError`), compares with the committed file, and rewrites it when `MP_UPDATE_GOLDEN=1` (pattern `mp-contracts/src/zdr_refusal.rs:991` [live-only]). Deterministic constants only.
  - NEW `MP/api/contracts/sse-event-payloads.json` and `.sha256` (generated).
  - NEW FE vendored copy `FE/src/shared/api/contracts/sse-event-payloads.json` and `.sha256` ([live-only] directory today; created by WS01 S0).
  - `scripts/tests/zdr-contract-sync.test.sh` ([M-live]) - append to `GOLDENS` (digest), `VENDORED` (copy and pin rows), `CONSUMERS` (`...|spa|apps/Frontend Plane/verevonv3/|WS12 S7`); self-test coverage follows the existing mechanism.
  - `.github/workflows/contract-goldens.yml:19-44` - add `"apps/**/sse-event-payloads.json*"` to both path lists; `model-plane-rust-ci.yml` already runs the sync script (`:92-96`) and `cargo test --workspace`.
  - `.gitattributes` - add `apps/**/contracts/*.json text eol=lf` (the sync script fails on CR; JSON is not pinned today).
  - NEW `FE/scripts/verify-sse-frames.mjs` - reads a captured SSE transcript (file or stdin) and checks each frame's keys against the vendored golden (always-present keys present; no unknown keys); used for the live probe below.
- **Proto.** none. **Migration.** none. **Flag.** none (test-only; zero runtime effect).
- **ZDR.** The golden contains key names and types of the ZDR-bearing frames (`usage.retention_class`, `error` with the typed ZDR refusal fields, `retrieval_notice`) so a rename of those keys is caught; no content, no persistence; no new golden-matrix row (the ZDR posture matrix is untouched). Samples use synthetic strings.
- **Tests.** `cd "apps/Model Plane/rust" && cargo test -q -p model-gateway --lib payload_contract`; regenerate with `MP_UPDATE_GOLDEN=1 cargo test -p model-gateway --lib sse_event_payload_golden`; `bash scripts/tests/zdr-contract-sync.test.sh && bash scripts/tests/zdr-contract-sync.test.sh --self-test`. Windows note from memory: two cargo targets fail pre-existing, use `--lib`.
- **DoD.** Renaming `delta` to `text` in `ReasoningDelta`'s `payload()` arm turns the Rust test and the sync script red; adding a variant without samples does not compile; golden/pin/vendored copy agree.
- **Evidence (repo rule: rebuilt image + running stack + real flow).** Rebuild model-gateway (`bash "apps/Model Plane/scripts/compose.sh" up -d --no-deps --build model-gateway`), drive one chat turn through the BFF with all feature families (reasoning, steps, tools, citations, artifacts, usage), capture the SSE transcript with `curl -N`, run `node scripts/verify-sse-frames.mjs <transcript>`; attach transcript and result. Also one ZDR turn to capture the typed refusal frame.
- **Deps.** WS01 S0 (sse_events.rs, taxonomy doc, vendored directory committed), S6 optional. **Risks.** `sse_events.rs` is the hot file for WS02 (Usage cache fields), WS04 (`ids`), WS15, WS27: whoever lands second regenerates the golden in the same PR (rule in section 5); minimal/full samples can miss a conditional branch (the test asserts each `if let` field appears in the full sample by listing optional keys in the sample comment); golden churn.

### S8 - SPA read checker and generated payload types [BUILD] (M, 3-4 d)
- **Goal.** The SPA cannot read a payload key the gateway does not emit, and has typed payloads.
- **Changes.**
  - NEW `FE/scripts/gen-chat-event-types.mjs` (`--check` mode, no deps) - from the vendored golden writes `FE/src/shared/api/chat-event-payloads.generated.ts` (`export interface UsagePayload { input_tokens: number; ... cost_usd?: number | null }`, an `AlwaysKeys`/`SometimesKeys` map and `ChatEventName` union). Header "GENERATED - do not edit".
  - NEW `FE/src/shared/api/chat-event-contract.test.ts` - vitest: (1) types file equals generator output; (2) reads `chat-client.ts`, brace-matches each `case '<name>':` block inside `dispatchEvent` (from `switch (event.event)` at `:1456` to its `default:`), extracts `payload.<key>` and `payload['key']` reads, plus the keys read by `emitCitation`/`failureTiming` (explicit helper list), and asserts `reads ⊆ golden keys ∪ LEGACY_READ_ALIASES[event]`; (3) `LEGACY_READ_ALIASES` (e.g. `done: modelUsed, outputTokens, stopReason`; `tool_call: tool_call_id, call_id, tool, function_name, arguments`) must each still be read (stale alias fails), can only shrink; (4) every always-present key is read or listed in `UNUSED_KEYS` with a reason. Transport events (`connected`, `chunk`, `done`) are listed as `TRANSPORT_EVENTS` and checked only for reads against an inline table (their payloads are not `ChatEvent`).
  - `FE/src/shared/api/chat-client.ts` ([M-live] +53) - adopt the generated type in two places only (`emitCitation`, the `usage` case) to prove the types compile into real code; everything else unchanged.
  - `FE/package.json` - one script line `"gen:chat-events": "node scripts/gen-chat-event-types.mjs"`; the `--check` is covered by the vitest test, so `scripts.test` is unchanged; no new dependency.
- **Tests.** `pnpm exec vitest run src/shared/api/chat-event-contract.test.ts src/shared/api/chat-client.test.ts` and `pnpm typecheck`. Mutation proofs recorded in the PR: rename a read key in a case block; edit the golden by hand (sync script catches it).
- **DoD.** Both directions of (2) red on a rename; the generated types typecheck under `tsc -b`; no behavioural diff in chat-client tests.
- **Evidence.** The mutation outputs and the S7 live transcript re-run against the final SPA.
- **Deps.** S7 (golden), WS01 S0. **Risks.** Reads inside helpers and `toVerevonUiEvent` are easy to miss (the helper list is explicit and the test fails if a new helper receives `payload` unlisted: scan for `(payload` argument passing); `chat-client.ts` is edited by WS02/WS04/WS08/WS09/WS11/WS15 (add-only hunks; section 5).

### S9 - Plane-boundary gate [BUILD] (M, 2-3 d)
- **Goal.** No plane's compose, env example or manifests reference another plane's datastore.
- **Changes.**
  - NEW `scripts/gates/plane-boundary.py` + `scripts/gates/plane-boundary.json` (plane directory -> allowed DB hosts, allowed cross-plane manifest dependencies with reasons). Rules: R1 every `postgres(ql)?(+driver)?://host`, `*DATABASE_URL`, `*_DSN` in `apps/<Plane>/**/docker-compose*.yml` and `**/.env.example` has a host in that plane's list (verified hosts in section 2.3; Redis/Dragonfly/Qdrant/MinIO phase 2 after the first run reports what exists); R2 in non-test Rust/Go/TS/Python source no `dblink`, `postgres_fdw`, `CREATE SERVER`, `\connect`, and no literal DSN host from another plane's list; R3 `Cargo.toml path=`, `go.mod replace`, `package.json file:/link:` must not point into another `apps/<Plane>` directory except the documented exception (`FE/package.json:23` `@quarry/client`, a generated SDK).
  - NEW `scripts/tests/test_plane_boundary.py` and fixtures under `scripts/tests/fixtures/plane-boundary/` (a Model Plane compose pointing at `controlplane-postgres` must fail; the real tree must pass; an exception removed from the JSON must fail). `--self-test` flag runs the fixtures (pattern `zdr-contract-sync.test.sh --self-test`).
  - `.github/workflows/repo-gates.yml` - job `plane-boundary` (`actions/setup-python@v5` 3.12, no deps).
- **ZDR/API/migration.** none.
- **Tests.** `python3 scripts/gates/plane-boundary.py --check`; `python3 -m unittest scripts/tests/test_plane_boundary.py`. **DoD.** Green on the tree, red on each fixture. **Evidence.** CI run plus the fixture failure output.
- **Deps.** S1, D2. **Risks.** Host allowlists go stale when a plane adds a store (the failure message says which file to edit); the intra-plane `session_core` sharing is deliberately not gated.

### S10 - Log-content gate [BUILD] (M, 3 d)
- **Goal.** New code cannot log raw user content; existing sites are baselined by hash.
- **Changes.**
  - NEW `scripts/gates/log-content.py` - tokenises Rust (strings, char literals, line and block comments), finds `trace!|debug!|info!|warn!|error!|tracing::<level>!|event!` invocations with balanced parens (multi-line), and flags a macro whose arguments contain a value-position identifier or field path whose last segment is in the denylist (`prompt, system_prompt, completion, transcript, user_message, answer, snippet, content, text, body`) or a `{:?}`/`%`/`?` format of `req|request|body|messages`. `message` alone is not on the list (it is mostly `status.message()` as the sweep above showed). Inline escape: `// log-content: allow(<reason>)` on the preceding line, reason required.
  - NEW `scripts/gates/baselines/log-content.json` - `sha256(file|normalised macro text)` entries for existing hits (hash, not line number, so moving code does not break; editing the macro requires review). Scope: `MP/rust/services/{model-gateway,execution-core,inference-core,session-core}/src` (D9); Go pass is phase 2.
  - NEW `scripts/tests/test_log_content.py` (unit fixtures: multi-line macro flagged; `error = %status.message()` clean; comment containing `info!(prompt)` ignored; string literal containing the macro ignored; allow-marker without a reason fails; a baselined hash passes and a changed one fails).
  - `.github/workflows/repo-gates.yml` - job `log-content`.
- **ZDR.** This gate is a preventive control for the persistence boundary of logs: no log line may carry turn content; it complements, not replaces, the runtime test at `agent.rs:8797`. No retention change.
- **Tests.** `python3 scripts/gates/log-content.py --check`; `python3 -m unittest scripts/tests/test_log_content.py`. **DoD.** Green with a reviewed baseline (size recorded; not determined today: the single-line scan found 5 false positives, the multi-line count is unknown); red on a new `info!(prompt = %p)`. **Evidence.** CI run, the baseline review notes, and the red proof.
- **Deps.** S1, D2, D9. **Risks.** Name-based false positives (baseline plus inline allow); false negatives from renamed bindings (documented limit: syntactic gate, not taint analysis).

### S11 - File size ratchet [BUILD] (M, 2-3 d)
- **Goal.** Hand-written files above the cap cannot grow; new files above it fail.
- **Changes.**
  - NEW `scripts/gates/size-ratchet.py` and `scripts/gates/baselines/file-size.json` (`{path: ceiling_lines}`), excluding generated files (`*.pb.go`, `**/gen/**`, `*_pb.ts`, files whose first 5 lines contain `@generated`, `Code generated` or `DO NOT EDIT`, listed globs in the JSON) and `node_modules`, `target`, `dist`. Fails when a file exceeds `max(cap, ceiling)`; when a file shrinks the ceiling must be lowered in the same PR (`--update` lowers only); `--base-ref origin/<base>` loads the base branch's baseline and fails if the PR baseline has any larger or new entry (a PR cannot relax its own check).
  - NEW `scripts/tests/test_size_ratchet.py` (growth fails; shrink without lowering fails; new over-cap file fails; generated file ignored; relaxed baseline fails).
  - `.github/workflows/repo-gates.yml` - job `size-ratchet` (fetch-depth 0).
  - Optional (D11): clippy count ratchet step in `model-plane-rust-ci.yml`, off by default.
- **Tests.** `python3 scripts/gates/size-ratchet.py --check --base-ref origin/<branch>`. **DoD.** Baseline includes the files in section 2.3 at their then-current length (L is larger than W: generate on the integration branch after WS01 S0, never from the dirty tree). **Evidence.** CI red proof (append a line to `agent.rs` on a throwaway branch) and green run.
- **Deps.** S1, D2, D8, WS01 S0 (so `agent.rs`/`sse.rs`/`http_routes.rs` ceilings are the committed lengths). **Risks.** The big hot files are edited by most workstreams; the ratchet blocks their growth (intended): the rule is to extract a module when a PR grows a ceiling file (WS03 and WS04 are the likely first hits). Function-level limits are not in this slice (D8).

### S12 - Stack-free browser-fixture job and bundle budget [BUILD] (S-M, 2 d)
- **Goal.** Give WS09's `chunk-recovery` spec and any future fixture-only Playwright spec a CI home, and stop silent bundle growth.
- **Changes.**
  - `.github/workflows/verevonv3-ci.yml` - NEW job `spa-browser-fixtures`: install as in `spa-verify` (`:143-193`), `pnpm exec vite build`, `pnpm exec playwright install --with-deps chromium` (precedent: the [live-only] `demo-product-build` job), `pnpm exec vite preview --port 5176 --strictPort &`, `pnpm exec playwright test --project=chunk-recovery`. The project itself is WS09's (`FE/playwright.config.ts`, `testDir: tests/e2e`; no `webServer` block exists, so the job starts the server); the job is a no-op skip until that project exists.
  - NEW `FE/scripts/check-bundle-budget.mjs` and `FE/bundle-budget.json` - after `vite build` reads `dist/assets/*.js`, gzips, compares the entry chunk and total JS against the committed ceilings (shrink-only, read from the base ref as in S11). First measurement is part of the slice (no `dist` exists in W; not determined).
  - `verevonv3-ci.yml` `spa-verify` - add the budget step after `pnpm verify`.
- **Tests.** `pnpm build && node scripts/check-bundle-budget.mjs`; unit `FE/scripts/check-bundle-budget.test.mjs` (`node --test`). **DoD.** Budget red when a 200 KB dependency is imported in the entry; fixture job green on the WS09 project. **Evidence.** CI runs; build output sizes.
- **Deps.** S1, WS09 (project), D1. **Risks.** Playwright install adds minutes; Chromium download in CI (network) is the same cost the live demo job already accepts.

## 5. Interfaces

**PROVIDES**
| To | Contract |
|---|---|
| All FE workstreams (WS08, WS09, WS10, WS11, WS13-WS16, WS23, WS27) | `spa-ui-gates` job: any new bare UI copy or raw colour in chat/agents, and in any newly added `src/` file, fails; `FE/src/test/locale-leak.ts` helper; `FE/docs/UI_STATE_CHECKLIST.md` |
| WS02, WS04, WS14, WS15, WS27 (any ChatEvent change) | `payload_contract` samples + `MP/api/contracts/sse-event-payloads.json` + generated `chat-event-payloads.generated.ts`: a new variant or key must update the sample table and regenerate the golden in the same PR (`MP_UPDATE_GOLDEN=1 cargo test -p model-gateway --lib sse_event_payload_golden`, then the sync script digest, then the vendored copy) |
| WS15 (typed run lifecycle events) | S6 gate extended to orchestration run events and ad-hoc names; its new events need a SPA `case` or a table entry |
| WS04, WS09, WS17 (proto additions) | working `buf breaking` and a lint ratchet (S2) |
| WS09 | CI home for stack-free Playwright projects and a bundle budget (S12) |
| WS01 | after WS01 lands the ZDR wave, a reachable CI to prove it (S1) |
| All Rust workstreams | log-content gate, plane-boundary gate, size ratchet (new `ceiling` rule for the three Model Plane hot files) |

**CONSUMES**
| From | Contract |
|---|---|
| WS01 S0 | committed ZDR wave: `sse_events.rs`, `sse.rs`, taxonomy doc, `zdr-contract-sync.test.sh`, vendored contract directory; staging stack for S7 evidence |
| WS09 | the `chunk-recovery` Playwright project and spec |
| WS08 | its FE guard test (no silent attachment drop) runs in `pnpm test`; no WS12 change |
| WS03 | action contract tests (existing `action-surface-contract.test.mjs`, `FE/package.json:13`) |
| WS02/WS15/WS27 | add samples for the variants they introduce |

**Shared files (conflict hot-spots) and edit order**
| File | Shared with | Rule |
|---|---|---|
| `FE/src/features/chat/components/ChatMessages.tsx` ([M-live] +229) | WS04, WS09, WS14, WS16 | WS01 S0 commits live set; S3 lands first with 6 tiny hunks, others rebase |
| `MG/src/sse_events.rs` ([M-live]) | WS02, WS04, WS15, WS27 | WS01 S0, then S7 (adds a test submodule only), then others regenerate golden in their PR |
| `FE/src/shared/api/chat-client.ts` ([M-live]) | WS02, WS04, WS08, WS09, WS11, WS15 | S8 touches `emitCitation` and the usage case only; others add-only |
| `scripts/tests/zdr-contract-sync.test.sh` ([M-live]) | WS01 | WS01 first; S7 appends array rows only |
| `.github/workflows/verevonv3-ci.yml` ([M-live] +55) , `model-plane-rust-ci.yml` ([M-live] +50) | WS01, WS09, WS11 | each adds a job; no edits inside existing jobs except S1 trigger filters |
| `FE/eslint.config.js`, `FE/eslint-suppressions.json` | any FE workstream adding code | suppressions regenerated only with `--prune-suppressions`; never hand-edit |
| `FE/package.json` ([M-live] +2) | WS09 | WS12 adds at most one script line |
| `FE/playwright.config.ts` | WS09 | WS09 owns projects; S12 only runs them |

## 6. Cross-session interfaces
- **Verevon Space session (a).** Shared: `SpaceApprovalPanel` (`spaces/components/SpaceApprovalPanel.test.tsx:40,57` assert Norwegian `Godkjenn` under default locale; whether the component uses `i18n.tr` was not opened), `SpacePage.test.tsx:627,659`, `SpaceRoomComposer`. Owner: (a) owns `features/spaces` copy; WS12 owns the config and helper. Assumption: (a) does not edit `eslint.config.js` or the suppression file; it opts in by adding `'src/features/spaces/**'` to `UI_GATE_SCOPES` once its own leaks are cleared. Any new Space file is already covered by the new-file check (D5). WS12 assumes (a) will not add a second SSE parser: Space room events must go through `chat-client.ts` and the golden.
- **Quarry v2 / browser session (b).** Shared: `ChatLiveRunPanel.tsx` (chat-side, per the T2/T3 split in `overlap-quarry.md`: T3 plan owns it, so S3 touches only its status words), `BrowserChrome.tsx` (27 same-line text hits; `Godkjenn` at `:373,:1804`, `aria-label 'Godkjenninger'` `:2040`; owned by (b), out of scope), and the `@quarry/client` `file:` dependency, which is the single documented cross-plane exception in the S9 allowlist (CI must generate the SDK before any new job installs: `verevonv3-ci.yml:171-177`). `quarry-docs-contract.yml` ([live-only]) is the same pattern at Quarry and stays theirs. Assumption: (b) does not add browser event names outside the orchestration oneof or the taxonomy tables; if it does, S6 fails until it registers them.

## 7. Regression guards
| Audit section 8 strength | Touch | Guard |
|---|---|---|
| Typed ChatEvent taxonomy and byte-verbatim relay | S6, S7, S8 add checks; no event renamed, BFF untouched | `sse-taxonomy.yml` (extended), `payload_contract`, `chat-event-contract.test.ts`, `zdr-contract-sync` |
| ZDR by construction | log-content gate; golden carries ZDR frames; S3 touches ZDR-adjacent files ([M-live] ChatMessages) | existing `*.zdr-refusal.test.ts` must stay green; S7 live probe includes a ZDR turn; log gate baseline review |
| Multi-tenant server-side runtime and proto-first contracts with breaking-change CI | S2 makes breaking-change CI real | `buf breaking` red/green proof |
| Rendering, accessibility, calm workspace | S3 changes labels on accessible controls | `chat-accessibility.test.ts` (44 px targets `:26`, live region `:60`), `chat-localization.test.ts`, S5 smoke; no tab-focus change |
| Two loops separate | none touched | plane-boundary gate does not merge or flag the loops; `cross_service_loop_contract.rs` unchanged |
| Model proposes, Quarry executes | none | n/a |
| Credential isolation | S9/S10 reduce risk | gates only add checks |
| Honest outcomes | CI baselines are recorded, not hidden | CI_BASELINE doc lists real reds; no gate is marked "done" without a recorded run (repo evidence rule: source + test + running stack) |

## 8. Effort and risks

**Total.** 27-35 engineer-days: S1 1-2, S2 2-3, S3 1-2, S4 3-4, S5 2-3, S6 2, S7 3-4, S8 3-4, S9 2-3, S10 3, S11 2-3, S12 2. **Parallel tracks:** A (F-t3-066): S3 -> S4 -> S5; B (F-t3-067 contracts): S6 -> S7 -> S8; C (repo gates): S9, S10, S11 independent of A and B after S1. **Critical path:** S1 -> WS01 S0 -> S7 -> S8 (about 9-12 days). S1 and S3 can start immediately; S7/S8 cannot until the ZDR wave is committed.

**Risks**
1. **CI does not run today** (3 runs ever; default branch `dev` has no workflows; no PRs): mitigate with S1 first, D1(c), and a recorded run per gate; no gate counts as done on source alone.
2. **First runs are red for pre-existing reasons** (typecheck, vitest, taxonomy at HEAD, buf): each new gate is its own job with its own baseline; CI_BASELINE lists existing reds with owners.
3. **Hot-file contention with 20+ workstreams** (ChatMessages.tsx, sse_events.rs, chat-client.ts, the three size-ratchet Rust files): small disjoint hunks, edit-order rules in section 5, golden regeneration is the second-lander's job, size ceilings force module extraction.
4. **Selector gate quality** (esquery flags, `:not` with child combinator, false positives, leaks in data not JSX): S4 spike plus fallback local rule; S5 render smoke catches what AST cannot; baseline absorbs noise; honest limit: the gate cannot judge language, only "a word in a string in a UI position".
5. **Golden-based payload contract can miss conditional branches and non-ChatEvent frames** (transport events, run events): full/minimal sample pair, exhaustiveness guard, S6 covers event names for the other emitters, transport payloads checked by an inline table; field-level contract for orchestration events (proto-typed) is not in scope and is already typed by `orchestration.proto`.
