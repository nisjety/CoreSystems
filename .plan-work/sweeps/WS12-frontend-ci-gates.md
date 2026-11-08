# WS12 frontend CI gates: code map (F-t3-066, F-t3-067)

Read-only sweep, 2026-10-15. Nothing was executed: no tests, builds, lint, gate scripts, or git writes. Counts are grep or wc results.

**Trees and sources**
- W = worktree `C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b`, HEAD afb83928. Status: only `?? .plan-work/`.
- L = live tree `C:/dev/CoresSystem`, HEAD afb83928. Status from `.plan-work/live-tree-status.txt` (324 entries, snapshot 2026-10-14 20:56), re-checked with `git --no-optional-locks status|diff` on the relevant paths.
- Audit `03-model-plane-chat-and-agents-audit.md`: F-t3-066 card at lines 1208-1219, F-t3-067 card at 1220-1231, section 9.1 rows at 1883-1884, Appendix D line at 2145 (F-t3-066 only). F-t3-067 has no Appendix C or D entry.
- Verdicts: MATCH; DRIFT (found at other lines, new range given); ABSENT; MISMATCH (cited mechanism differs). Live: `clean` = not in the status file; `M` = modified; `??` = live-only.

## 0. Findings in brief

**F-t3-066 (UI copy and lint gates)**
- The card's ChatMessages.tsx lines (approval buttons, RUNG_LABEL) match L, where the file is M (+229 lines changed). W is 2 lines lower (Godkjenn at W:1915, L:1917).
- The card undercounts the leaks. Extra bare literals not in the card: ChatMessages.tsx:2521 in W (2523 in L, a ternary `'Godkjenner…' : 'Godkjenn'`), ChatArtifactPanel.tsx:354 and :607 (both clean). A grep heuristic finds 126 candidate lines in features/chat and features/agents (section 2c).
- Existing gate the card omits: `src/features/chat/lib/chat-localization.test.ts` requires `i18n.tr` on naming attributes only (aria-label, aria-description, alt, placeholder, title), and only under features/chat. It does not check JSX text, which is where the card's leaks sit.
- `eslint.config.js` already has a `no-restricted-syntax` key (:93-107: the A1 posture selector at :99-103 and the speech selectors spread at :106). The card's proposed second block must re-spread them; the file's own comment says a second key replaces the first (:104-105).
- Counts: `i18n.tr(` occurs 4,650 times in src (4,631 in non-test files), against the card's "about 984". Hex literals in features/agents: 541 occurrences, against "about 537".
- Unresolved conflict: `chat-localization.test.ts:16-18` says `pnpm lint` cannot run on the pinned TypeScript 7. `package.json:49` pins `typescript` to the TS6 alias (`npm:@typescript/typescript6@^6.0.2`). Not run.

**F-t3-067 (contract and architecture gates)**
- The card's line counts (agent.rs 13,521; sse.rs 13,325; http_routes.rs 11,681) match L, and all three files are M. W has 13,328, 12,580 and 11,538.
- `ChatEvent::payload()` in sse_events.rs starts at L:490 (the card's range fits L). In W it starts at :425.
- The ratchet claim is wrong: `apps/Control Plane/auth-core/scripts/lint-ratchet.mjs` baselines with ESLint suppression files (:191, :229) and contains no hash logic.
- `.github/workflows/buf.yml:21` and `:23` pass `apps/Model Plane/proto` unquoted, so the shell splits the path at the space. Effect not determined (not run).
- Existing gates the card omits: `.github/workflows/contract-goldens.yml` (drift check over goldens, pins and vendored copies), `scripts/tests/zdr-contract-sync.test.sh`, the grep-based fabrication-guard job (`verevonv3-ci.yml:39-106`), and `apps/Frontend Plane/verevonv3/apps/gateway/scripts/action-surface-contract.test.mjs` (registry-to-dispatcher parity, run by `pnpm test`).
- Live-only (`??`): a ZDR-refusal golden chain (mp-contracts generator, then `api/contracts/zdr-refusals.json`, then an SPA vendored copy pinned by sha256). It vendors JSON goldens; it does not generate TypeScript types.
- Absent: codegen crates (ts-rs, schemars, utoipa, specta) under apps, a size ratchet, a plane-boundary DB gate, a log-content gate. Model Plane clippy is left out on purpose (`model-plane-rust-ci.yml:121-129`).

## 1. Citation verification

| Card | Cited path:lines | Claim | W verdict | Live | Corrected (W / L) |
|---|---|---|---|---|---|
| 066 | apps/Frontend Plane/verevonv3/src/features/chat/components/ChatMessages.tsx:1917,1924 | bare "Godkjenn" and "Avvis" buttons | DRIFT: literals at :1915, :1922 | M; L MATCH | W :1915,:1922 / L :1917,:1924 |
| 066 | ChatMessages.tsx:2536-2542 | Norwegian rung labels | DRIFT: GRANTABLE_RUNGS :2534; RUNG_LABEL :2536-2540 | M; L MATCH | W :2536-2540 / L :2538-2542 |
| 066 | apps/Frontend Plane/verevonv3/src/features/chat/components/ChatArtifactPanel.tsx:397,581 | bare "Last ned" link text | MATCH | clean | same |
| 066 | apps/Frontend Plane/verevonv3/src/features/agents/components/FleetRunConsole.tsx:150-158 | English-only "Pause fleet" | MATCH: literal :155; English note :159 is just outside the range | clean | :155, :159 |
| 066 | eslint.config.js (resolved: apps/Frontend Plane/verevonv3/eslint.config.js) | no JSX-literal or raw-colour rule; no stylelint | MATCH on absence (grep: no stylelint dependency or config). INCOMPLETE: a no-restricted-syntax key exists at :93-107 | clean | same |
| 066 | apps/Frontend Plane/verevonv3/apps/verevon-web/DESIGN.md (no line in card) | DESIGN.md is the marketing-site spec | MATCH (:1 title "Verevon marketing site — design system"; :3-5 scope) | clean | same |
| 066 | (no path) | inline `tr(no, en)` is the documented convention | MATCH: `tr: (noText, enText) => string` at apps/Frontend Plane/verevonv3/src/shared/i18n/index.tsx:22 (also :32, :62) | clean | same |
| 066 | (no path) | "about 984 other strings use i18n.tr" | NOT REPRODUCED: 4,650 `i18n.tr(` occurrences in src; 4,631 in non-test files | clean | n/a |
| 066 | (no path) | "features/agents holds about 537 hex literals" | ~MATCH: 541 occurrences of `#[0-9a-fA-F]{3,8}` in features/agents *.ts and *.tsx (tests not excluded) | clean | n/a |
| 066 | (no path) | "roughly 15 to 20 leaking sites" | UNDERCOUNT: 126 candidate lines (section 2c) | mixed | n/a |
| 067 | .github/workflows/sse-taxonomy.yml:44-62 | runs taxonomy --check and proves it can fail | MATCH: check at :45; drift proof at :49-62 | clean | same |
| 067 | gen-sse-event-taxonomy.py (path resolved) | event-name coverage only, no payload check | MATCH: apps/Model Plane/scripts/gen-sse-event-taxonomy.py checks event names and case labels (:81-83, :130-134); no payload-field logic | clean | same |
| 067 | apps/Model Plane/rust/services/model-gateway/src/sse_events.rs:490-500 | payload() hand-builds json! | DRIFT: fn payload at :425; arms read at :491-505 | M; L MATCH (fn :490; json! :492, :494, :502) | W :425 / L :490-502 |
| 067 | agent.rs 13,521 lines (path resolved) | size-ratchet target | DRIFT: 13,328 lines | M; L MATCH (13,521) | apps/Model Plane/rust/services/execution-core/src/runtime_loop/agent.rs |
| 067 | sse.rs 13,325 lines | size-ratchet target | DRIFT: 12,580 lines | M; L MATCH (13,325) | apps/Model Plane/rust/services/model-gateway/src/sse.rs |
| 067 | http_routes.rs 11,681 lines | size-ratchet target | DRIFT: 11,538 lines | M; L MATCH (11,681) | apps/Model Plane/rust/services/model-gateway/src/http_routes.rs |
| 067 | apps/Control Plane/auth-core/scripts/lint-ratchet.mjs ("baselined by hash") | ratchet baseline mechanism | MISMATCH: ESLint suppression JSON (:191 `.eslint-clean-suppressions.json`; :229 `eslint-suppressions.json`); no hash code | clean | same |
| 067 | .github/workflows/buf.yml (card: "passes an unquoted path") | buf lint on a path with a space | MATCH: :21 `buf lint apps/Model Plane/proto`; :23 `buf breaking` with the same unquoted path and `--against` | clean | same; effect not determined |
| 067 | (no path) | buf breaking runs; contract goldens sha256-pinned | MATCH: buf.yml:23; model-plane-rust-ci.yml:92-96; contract-goldens.yml:55-74. sha256 files exist for connection-zdr-capabilities.json and zdr-posture-matrix.json only; infer-statuses.json has none | clean (zdr-contract-sync.test.sh is M) | same |
| 067 | (no path) | no ts-rs, schemars, utoipa or specta under apps | MATCH: no hit in Cargo.toml, package.json, *.rs, *.ts, *.tsx, *.sh, *.py or *.mjs under apps (node_modules excluded). Adjacent: apps/Control Plane/auth-core/src/orpc/openapi.controller.ts:4, :36 (Zod to JSON Schema for OpenAPI) | clean | same |
| 067 | (no path) | "Model Plane clippy" missing | MATCH, deliberate: model-plane-rust-ci.yml:121-129 (304 pre-existing pedantic warnings). No clippy step in that workflow | M (live adds one job, not clippy) | same |
| 067 | (no path) | no cross-plane DB or log-content gate | MATCH by grep over .github/workflows and scripts/. scripts/coresystem-cross-plane-preflight.sh is a read-only Docker and topology check | clean | n/a |
| 067 | (chat-client.ts reads, field level) | SSE consumer exists | MATCH: 42 `case '…':` labels; onUnknownEvent at :664, :1451, :1719; stream paths :1767, :1794 | M (L adds a `retrieval_notice` case at :1740) | apps/Frontend Plane/verevonv3/src/shared/api/chat-client.ts |
| 067 | (no path) | hermetic gates modelled on scripts/tests with self-tests | MATCH: scripts/tests/zdr-contract-sync.test.sh runs with `--self-test` (contract-goldens.yml:71) | M | same |

## 2. Affected-file map

Lines are W unless marked L. "clean" = not in the live status file.

### 2a. F-t3-066 (UI copy and lint)

| Path | Role | Lines | Live | Existing tests |
|---|---|---|---|---|
| apps/Frontend Plane/verevonv3/src/features/chat/components/ChatMessages.tsx | approval block, plan gate, RUNG_LABEL, ToolChips | bare: 1915, 1922, 2521 (L 2523), 2536-2540 (L 2538-2542); grep candidates: 1644 ('Verktøy' tab label), 2028 (status words), 2358 (fallback JSX) | M (+229) | 13 ChatMessages.*.test.tsx files (codeblock, continue, export, metrics, read-aloud, schedule-action, truth, plan-gate, generated-files, run-notify, chart, processing-line, failure-actions). No test queries the approve or reject buttons by name. ChatMessages.chart.test.tsx:104, :207 and generated-files.test.tsx:56 assert Norwegian labels |
| apps/Frontend Plane/verevonv3/src/features/chat/components/ChatArtifactPanel.tsx | artifact actions | bare: 354 ('Vis forhåndsvisning' / 'Vis kode'), 397 and 581 ('Last ned'), 607 (Norwegian sentence in JSX) | clean | ChatArtifactPanel.test.tsx:198, :219 assert 'Last ned' |
| apps/Frontend Plane/verevonv3/src/features/agents/components/FleetRunConsole.tsx | fleet control | bare: 155 ('Pause fleet'); English note 159 | clean | FleetRunConsole.test.tsx covers the fleet header only; no assertion mentions pause or cancel (grep) |
| apps/Frontend Plane/verevonv3/src/features/chat/components/ChatLiveRunPanel.tsx | run status and trace copy | grep candidates: 69, 352-404, 475-476 ('Fullført', 'Kjører') | clean | none found by name |
| apps/Frontend Plane/verevonv3/src/features/chat/components/ChatPanels.tsx | run proof, plan detail | grep candidates: 633, 685, 816, 1002, 1286-1289, 1424 | clean | ChatPanels.test.tsx:795, :805 assert rung labels |
| apps/Frontend Plane/verevonv3/src/features/chat/components/chat-normalizers.ts | error and turn copy | grep candidates: 124-130, 286-526, 1168, 1211 (not read in full) | M (+39) | chat-normalizers.zdr-refusal.test.ts (??) |
| apps/Frontend Plane/verevonv3/src/features/chat/components/use-chat-controller.ts | feedback and error copy | grep candidates: 2055-2118, 3430-3690 | M (+4) | use-chat-controller.zdr-refusal.test.ts (??) |
| apps/Frontend Plane/verevonv3/src/features/agents/components/AgentRunConsole.tsx | already bilingual: 'Godkjenn' and 'Avvis' via i18n.tr at :1320, :1331, :1355. This is the in-repo pattern to copy | i18n.tr uses at :521, :1300, :1718 | M (+11) | AgentRunConsole.test.tsx (M) |
| apps/Frontend Plane/verevonv3/src/features/agents/components/AgentInstallationsPage.tsx | Norwegian copy | grep candidates: 50, 62 (may be multi-line i18n.tr arguments) | clean | not determined |
| apps/Frontend Plane/verevonv3/src/features/dashboard/home/BrowserChrome.tsx (outside card scope) | bare Godkjenn | 373, 1804 (text); 2040 (aria-label 'Godkjenninger'); 2043 (span) | clean | not determined; chat-localization.test.ts does not cover this folder |
| apps/Frontend Plane/verevonv3/eslint.config.js | lint rules | no-restricted-imports :74-85 (demo and mock modules); no-restricted-syntax :93-107 (A1 :99-103; speech spread :106 from :24-31); no-restricted-globals :112; no-restricted-properties :113; ignores :44 ('apps', 'packages') | clean | none (config) |
| apps/Frontend Plane/verevonv3/src/shared/i18n/index.tsx | i18n.tr API | :22 (type), :32 (default), :62 (locale), :93 (translateApiError re-export) | clean | src/shared/i18n/errors.test.ts |
| apps/Frontend Plane/verevonv3/src/shared/i18n/errors.ts | error-copy seam | — | M (+12: ZDR wording delegated to the catalogue) | errors.test.ts; errors.zdr-refusal.test.ts (??) |
| apps/Frontend Plane/verevonv3/src/shared/i18n/en.ts, no.ts, locales.ts | dictionaries and locale list | — | clean | not read |
| apps/Frontend Plane/verevonv3/src/features/chat/lib/chat-localization.test.ts | EXISTING gate: naming attributes must go through i18n.tr. NAMING_ATTRIBUTES :24; HAS_WORDS :61; scan limited to src/features/chat :71-93 | — | clean | self |
| apps/Frontend Plane/verevonv3/src/features/chat/lib/chat-accessibility.test.ts | a11y tests (44px targets :26; polite live region :60) | — | clean | self |
| apps/Frontend Plane/verevonv3/apps/verevon-web/DESIGN.md | marketing-site design system | :1-5 | clean | none; verevon-web is outside root eslint (:44) |
| apps/Frontend Plane/verevonv3/package.json | scripts: build :9, lint :11, test :13, test:e2e :15, typecheck :17, verify :18 | — | M (+2: demo:build, demo:probe) | — |
| .github/workflows/verevonv3-ci.yml | jobs: fabrication-guard :39-106 (grep only); gateway-rust :108 (clippy -D warnings at :138); spa-verify :143-193; docker-image :195 | — | M (+55: demo-product-build job) | — |
| apps/Frontend Plane/verevonv3/src/features/chat/components/ChatMessages.failure-actions.test.tsx | ZDR ErrorNotice tests | :9, :160 | M (+34) | self |

### 2b. F-t3-067 (contracts and gates)

| Path | Role | Lines | Live | Existing tests |
|---|---|---|---|---|
| .github/workflows/sse-taxonomy.yml | CI gate | triggers :16-30; check :44-45; drift proof :49-62 | clean | self (drift proof) |
| apps/Model Plane/scripts/gen-sse-event-taxonomy.py | checker | PRODUCER :37; CONSUMER :38; DOC :39; NON_CHATEVENT_CONSUMER_CASES :43-49; VARIANT_ARM :81-83; consumer_cases :130-134 | clean | CI drift proof only |
| apps/Model Plane/docs/sse-event-taxonomy.md | committed event taxonomy | — | M (+4: rows for error, plan_proposed, retrieval_notice) | checked by the script |
| apps/Model Plane/rust/services/model-gateway/src/sse_events.rs | ChatEvent enum (W :105, L :144); family() (W :351, L :410); name() (W :398); payload() (W :425, L :490); tests mod (W :1046, L :1139); names_and_payload_shapes_are_stable (W :1507, L :1602) | 1,899 W; 2,100 L | M (+203 changed: RetentionClass (Zdr, ZdrWaived; L :108) on the usage frame; RetrievalNotice and ZdrError (L only)) | names_and_payload_shapes_are_stable (usage and citation payload keys, W :1507-1545) |
| apps/Model Plane/rust/services/model-gateway/src/sse.rs | SSE transport: error_stream (W ~:118); invoke_stream_sse (W hunks :858-912); mod subscription_route_tests (W hunks :562-571) | 12,580 W; 13,325 L | M (+883 changed: ZDR stream paths zdr_error_stream, prepared_zdr_refusal_stream, direct_infer_turn, zdr_answer_stream) | subscription_route_tests (W); L adds a_reduced_zdr_answer_says_so_before_it_streams and a_zdr_answer_without_a_notice_streams_no_notice |
| apps/Model Plane/rust/services/model-gateway/src/http_routes.rs | route table: POST /v1/invoke/stream (:137); GET /v1/threads/:thread_id/events (:181); GET /v1/runs/:run_id/events/replay (:218); GET /v1/runs/:run_id/events (:220) | 11,538 W; 11,681 L | M (+237 changed: ZDR gating in handlers, zdr_or_refuse) | not determined |
| apps/Model Plane/rust/services/execution-core/src/runtime_loop/agent.rs | run loop; largest file in the card | 13,328 W; 13,521 L | M (+207 changed: caller parameter threaded through run_agent, run_agent_with_tools, run_rounds, continuation_descriptor, and tests) | in-file tests (mod tests; W hunks after :7176 and :7264) |
| apps/Frontend Plane/verevonv3/src/shared/api/chat-client.ts | SSE consumer: 42 case labels; onUnknownEvent :664, :1451, :1719; stream paths :1767, :1794 | — | M (+53: ChatRetrievalNoticeEvent type; retrieval_notice case at L :1740; isZdrCode import) | chat-client.test.ts (M, +22: retrieval_notice routing test) |
| apps/Frontend Plane/verevonv3/src/shared/api/sse.ts | SPA SSE error path (ApiError gets a feature) | — | M (+4) | not determined |
| apps/Frontend Plane/verevonv3/src/shared/api/http.ts | error-body parsing (zdr feature key) | — | M (+24) | http.zdr-refusal.test.ts (??) |
| apps/Frontend Plane/verevonv3/src/shared/api/zdr-refusals.ts | renders ZDR codes from the vendored catalogue | — | ?? | zdr-refusals.test.ts (??) |
| apps/Frontend Plane/verevonv3/src/shared/api/contracts/zdr-refusals.json and .sha256 | vendored SPA copy; sha256 9c2b5458… | — | ?? | pinned at zdr-contract-sync.test.sh:219-220 (L) |
| apps/Model Plane/api/contracts/zdr-refusals.json and .sha256 | golden generated by mp-contracts (zdr-contract-sync.test.sh:109, L) | — | ?? | zdr-contract-sync.test.sh (L) |
| apps/Model Plane/rust/crates/mp-contracts/src/zdr_refusal.rs | golden generator (`MP_UPDATE_GOLDEN=1`) | — | ?? | live-only mp-contracts/tests/zdr_retention_matrix.rs |
| apps/Model Plane/rust/services/model-gateway/tests/zdr_refusal_inventory.rs | inventory test | — | ?? | self |
| apps/Model Plane/proto/model_plane/v1/gateway.proto | InvokeRequest (:182); `features` (:241) names the opt-in SSE families | — | clean | none |
| apps/Model Plane/proto/model_plane/v1/orchestration.proto | SSE-compatible run events and ids (:112, :1001, :1014) | — | clean | none |
| .github/workflows/contract-goldens.yml | goldens job (:55-74): drift check (:66), self-test (:71), loader test (:74) | — | clean | self |
| scripts/tests/zdr-contract-sync.test.sh | drift check over goldens, pins and vendored copies; `--self-test` | — | M (+95: Z-05a and Z-05b pins, L) | self-test |
| scripts/tests/zdr-contract-schema.mjs and zdr-contract-schema.test.mjs | loader for the refusal catalogue | — | M (+24) and clean | zdr-contract-schema.test.mjs |
| .github/workflows/model-plane-rust-ci.yml | rust job :69-119 (fmt :99, check :105 and :112, test :119); contract-goldens step :92-96; clippy omission note :121-135 | — | M (+50: session-core-receipts job) | — |
| .github/workflows/buf.yml | buf lint :21; buf breaking :23 | — | clean | — |
| apps/Model Plane/proto/buf.yaml and proto/baseline.binpb | buf module config; breaking baseline (both exist) | — | clean | — |
| apps/Control Plane/auth-core/scripts/lint-ratchet.mjs | changed-file ESLint ratchet on a suppression baseline (:8, :15-29, :191, :229, :251) | — | clean | lint-ratchet.test.mjs (package.json:16) |
| apps/Control Plane/auth-core/src/orpc/openapi.controller.ts | OpenAPI from Zod (:4, :36); the only schema generation found, non-Rust | — | clean | not determined |
| apps/Frontend Plane/verevonv3/apps/gateway/scripts/action-surface-contract.test.mjs | registry ids in src/shared/actions/action-registry.ts must equal dispatcher ids in apps/Frontend Plane/verevonv3/apps/gateway/src/domains/actions/handlers.rs | — | clean | self; run by `pnpm test` |
| apps/Frontend Plane/verevonv3/apps/gateway/scripts/product-truth-contract.test.mjs | asserts DashboardComposer.tsx omits /api/v1/projects, and StudioPage.tsx contains the Norwegian literal 'Midlertidig Studio-prosjekt' (an i18n move breaks this test unless it changes) | — | clean | self; run by `pnpm test` |
| scripts/coresystem-conformance.sh (repo root) | aggregates the cross-plane preflight (:197-203, :263-271) | — | clean | scripts/tests/coresystem-conformance-test.sh |
| scripts/coresystem-cross-plane-preflight.sh | read-only Docker and topology preflight; not a DB boundary gate | — | clean | scripts/tests/coresystem-cross-plane-preflight-test.sh |
| scripts/lint-env-files.sh; scripts/lint-resource-taxonomy.sh | repo-root lint scripts; not invoked from .github/workflows (grep) | — | clean | none found |
| .githooks/pre-commit | local gitleaks secret gate (115 lines); not a UI or SSE gate | 115 | M (+2 excludes for the product-demo bundle) | — |

### 2c. Candidate scan (heuristic, not an AST scan)
- Method: `grep -rnP "[æøåÆØÅ]"` over features/chat/components and features/agents/components (*.ts, *.tsx). Excluded: test files and lines with `i18n.tr`, `tr(`, comments.
- Result: 126 lines (124 in features/chat, 2 in features/agents).
- False positives: multi-line `i18n.tr` arguments, and non-UI constants such as error strings in chat-normalizers.ts.
- Misses: ASCII-only Norwegian words (Godkjenn, Avvis, Last ned, Vis kode, Verktøy) unless the line also has æøå.

## 3. Related contracts

- **SSE event types.** `ChatEvent` enum in model-gateway sse_events.rs (W :105, L :144). Live-only variants: `RetrievalNotice` (L :368, :438, :480, :672) and `ZdrError` (L :384). Accessors: name() (W :398), family() (W :351, L :410), payload() (W :425, L :490). No `.proto` file names ChatEvent (grep -w, both trees).
- **Gateway routes.** model-gateway http_routes.rs: POST /v1/invoke/stream (:137), GET /v1/threads/:thread_id/events (:181), GET /v1/runs/:run_id/events/replay (:218), GET /v1/runs/:run_id/events (:220). SPA: /api/v1/chat/stream (chat-client.ts:1767) and /api/v1/chat/stream/resume/{requestId} (:1794).
- **Proto.** 18 files in apps/Model Plane/proto/model_plane/v1/ (Glob). Relevant: gateway.proto (InvokeRequest :182; `features` :241 is the opt-in SSE family list); orchestration.proto (:112, :1001, :1014, SSE ids and Last-Event-Id). sessions.proto is M (SessionRetentionService; not WS12).
- **Contract goldens.** apps/Model Plane/api/contracts/: connection-zdr-capabilities.json (+ .sha256), zdr-posture-matrix.json (+ .sha256), infer-statuses.json (no .sha256), README.md (M), zdr-refusals.json (+ .sha256, L only). SPA vendored copy: apps/Frontend Plane/verevonv3/src/shared/api/contracts/zdr-refusals.json (+ .sha256, L only).
- **Action contracts.** apps/Frontend Plane/verevonv3/src/shared/actions/ (action-registry.ts, types.ts, catalog-manifest.ts, action-client.ts, agent-tools.ts, and tests). Dispatcher: apps/Frontend Plane/verevonv3/apps/gateway/src/domains/actions/handlers.rs.
- **SQL migrations.** apps/Model Plane/rust/services/session-core/migrations/. W latest: 0043_audit_outbox_requeue.sql. L latest: 0045_zdr_waived_runs_org_index.sql, with 0044_run_retention_receipt.sql and revert/ also live-only. WS12 needs no migration.
- **Config.** apps/Frontend Plane/verevonv3/{eslint.config.js, package.json, tsconfig.json, vite.config.ts, playwright.config.ts}; apps/Model Plane/rust/Cargo.toml (workspace); Rust gateway manifest apps/Frontend Plane/verevonv3/apps/gateway/Cargo.toml.
- **Generated types.** None. No ts-rs, schemars, utoipa or specta dependency under apps.
- **Env files.** None relevant found by name grep.

## 4. Test and build commands (from files; none executed)

- **Frontend** (apps/Frontend Plane/verevonv3/package.json): `pnpm lint` = `eslint .` (:11; eslint.config.js:44 ignores apps, packages and .claude/worktrees). `pnpm typecheck` = `tsc -b` (:17). `pnpm test` = four `node --test` gateway scripts, then `vitest run` (:13). `pnpm build` = `pnpm typecheck && vite build` (:9). `pnpm verify` = lint, typecheck, test, build (:18). `pnpm test:e2e` = `playwright test` (:15). Single file: `pnpm exec vitest run <path>`.
- **Frontend CI** (.github/workflows/verevonv3-ci.yml): fabrication-guard (:39-106, grep only); gateway-rust (:108; `cargo clippy --all-targets -- -D warnings` at :138); spa-verify (:143-193: builds @quarry/client and @verevon/remote-core, then `pnpm verify`).
- **Rust gateway:** `cargo test --manifest-path "apps/Frontend Plane/verevonv3/apps/gateway/Cargo.toml" --all-targets` (run from the repo root; the SPA doc gives the path relative to verevonv3) (root CLAUDE.md, Build And Run).
- **Model Plane crates** (name at Cargo.toml line 2): model-gateway (apps/Model Plane/rust/services/model-gateway), execution-core (apps/Model Plane/rust/services/execution-core), mp-contracts (apps/Model Plane/rust/crates/mp-contracts). Workspace: apps/Model Plane/rust/Cargo.toml. Target test: `cargo test -p model-gateway names_and_payload_shapes_are_stable`.
- **Model Plane CI** (.github/workflows/model-plane-rust-ci.yml, job `rust` at :69): `cargo fmt --all -- --check` (:99); `cargo check --workspace` (:105); `cargo check --workspace --all-targets` (:112); `cargo test --workspace --no-fail-fast` (:119).
- **Golden regeneration:** `MP_UPDATE_GOLDEN=1 cargo test -p mp-contracts zdr_refusal` (apps/Model Plane/api/contracts/README.md, L).
- **Taxonomy:** `python3 "apps/Model Plane/scripts/gen-sse-event-taxonomy.py" --check` (sse-taxonomy.yml:45). Without `--check` it rewrites the doc (script docstring, :22-24).
- **Contract drift:** `bash scripts/tests/zdr-contract-sync.test.sh` and `--self-test` (contract-goldens.yml:66, :71; model-plane-rust-ci.yml:95-96). `node --test scripts/tests/zdr-contract-schema.test.mjs` (contract-goldens.yml:74).
- **buf:** `buf lint apps/Model Plane/proto` (buf.yml:21); `buf breaking apps/Model Plane/proto --against apps/Model Plane/proto/baseline.binpb` (:23). Both unquoted.
- **Control Plane ratchet** (apps/Control Plane/auth-core/package.json): `lint` = `node scripts/lint-ratchet.mjs` (:15); `lint:contract` = `node --test scripts/lint-ratchet.test.mjs` (:16).
- **Repo root:** `bash scripts/tests/coresystem-conformance-test.sh` (exists; no CI reference by grep).

## 5. Owners

- **Frontend Plane** owns the Verevon v3 UI and same-origin gateway and BFF normalization (root CLAUDE.md, "Architecture Rules"). Feature slices sit in src/features/*, and action contracts in src/shared/actions (root CLAUDE.md, "Code Style").
- **Model Plane** owns the chat stream producer. model-gateway owns "Public invoke, stream" (apps/Model Plane/docs/capability-ownership-matrix.md:125). The ingress row is :37, and the protocol row "Public client to gateway: HTTP/JSON + SSE" is :181. apps/Model Plane/CLAUDE.md ("What this is") says Rust owns the hot path (invoke, streaming, session assembly). Root CLAUDE.md gives Model Plane "execution loops", which covers execution-core agent.rs.
- **Control Plane:** apps/Control Plane/auth-core owns lint-ratchet.mjs and its test (package.json:15-16).
- **Repo-root CI and scripts** (scripts/, .github/workflows/): the card says "no plane owner". apps/master-ownership-matrix.md has no row for repo-root CI gates (its only CI mention is the evals row, :91). Not determined.
- **App Shell** (master matrix): :42 maps human-facing UX to App Shell and :41 maps reasoning to Model Plane. The matrix is dated 2026-05-06 and predates Frontend Plane naming, so the App Shell to Frontend Plane mapping is not determined beyond root CLAUDE.md.
- **Binding rules for these gates:** root CLAUDE.md, "No direct database crossing between planes"; root CLAUDE.md, "Zero Data Retention must propagate through any content-persisting boundary" (governs any log-content gate); apps/Model Plane/CLAUDE.md, "One canonical owner per capability" (check docs/capability-ownership-matrix.md); apps/Model Plane/CLAUDE.md, "cross-service invariants ... asserted at runtime and mutation-tested" (pattern for F-t3-067 gates).

## 6. Uncommitted-only (live-only or differs in live)

**Modified in live** (diff stat for the 22 relevant M files: 2,006 insertions, 194 deletions)

| Path | Live | Change |
|---|---|---|
| .github/workflows/verevonv3-ci.yml | M +55 | adds job demo-product-build: demo bundle manifest, rebuild must equal committed bundle, privacy probe |
| .github/workflows/model-plane-rust-ci.yml | M +50 | adds job session-core-receipts: Postgres receipt tests; fails if fewer than 8 run |
| apps/Frontend Plane/verevonv3/package.json | M +2 | adds demo:build and demo:probe scripts |
| .../features/chat/components/ChatMessages.tsx | M 229 lines | ErrorNotice and EmptyChatState ZDR and notice UI (W :3414-3422, :3763-3853); new imports (prompt suggestions, inbox and notification clients); approval and RUNG_LABEL literals unchanged, shifted by +2 |
| .../ChatMessages.failure-actions.test.tsx | M +34 | ZDR refusal ErrorNotice tests |
| .../features/agents/components/AgentRunConsole.tsx | M 11 | onError renders ZDR refusals via zdr-refusals presentation with i18n.tr |
| .../features/chat/components/chat-normalizers.ts | M +39 | ZDR refusal codes and copy (isZdrCode import) |
| .../features/chat/components/chat-types.ts | M 7 | StreamFailureAction adds open_integrations and none |
| .../features/chat/components/use-chat-controller.ts | M +4 | ZDR refusal treated as the turn's answer; passes failure.feature |
| .../src/shared/api/chat-client.ts | M +53 | ChatRetrievalNoticeEvent type; retrieval_notice case (L :1740); isZdrCode import |
| .../src/shared/api/chat-client.test.ts | M +22 | retrieval_notice routing test |
| .../src/shared/api/sse.ts | M 4 | ApiError carries the feature key |
| .../src/shared/api/http.ts | M 24 | ApiError gets a feature field for zdr_feature_not_supported |
| .../src/shared/i18n/errors.ts | M +12 | ZDR refusal wording delegated to the catalogue |
| apps/Model Plane/rust/services/model-gateway/src/sse_events.rs | M 203 | RetentionClass enum (Zdr; ZdrWaived with a reason; L :108) on the usage frame; RetrievalNotice (L :368) and ZdrError (L :384) variants |
| apps/Model Plane/rust/services/model-gateway/src/sse.rs | M 883 | ZDR refusal and answer stream paths, with tests |
| apps/Model Plane/rust/services/model-gateway/src/http_routes.rs | M 237 | ZDR gating in handlers (zdr_or_refuse; DURABLE_MUTATION_FORBIDDEN refusal) |
| apps/Model Plane/rust/services/execution-core/src/runtime_loop/agent.rs | M 207 | caller: &AuthenticatedUser threaded through run-loop functions and tests |
| apps/Model Plane/docs/sse-event-taxonomy.md | M +4 | rows for error, plan_proposed, retrieval_notice |
| apps/Model Plane/api/contracts/README.md | M +1 | documents zdr-refusals.json and its generator |
| scripts/tests/zdr-contract-sync.test.sh | M 95 | Z-05a and Z-05b pins (catalogue golden; SPA vendored copy) |
| scripts/tests/zdr-contract-schema.mjs | M 24 | reads refusal codes from zdr-refusals.json |
| .githooks/pre-commit | M +2 | excludes the generated product-demo bundle from the secret scan |
| apps/Model Plane/proto/model_plane/v1/sessions.proto | M | adds SessionRetentionService (not WS12) |
| .github/workflows/dataplane-v2-ci.yml | M | Data Plane, not WS12 (not summarised) |

**Untracked in live (`??`), WS12-relevant**
- apps/Frontend Plane/verevonv3/src/shared/api/contracts/zdr-refusals.json and .sha256
- apps/Frontend Plane/verevonv3/src/shared/api/zdr-refusals.ts and zdr-refusals.test.ts
- apps/Frontend Plane/verevonv3/src/shared/api/chat-client.zdr-refusal.test.ts; http.zdr-refusal.test.ts; src/shared/i18n/errors.zdr-refusal.test.ts; src/features/chat/components/chat-normalizers.zdr-refusal.test.ts; use-chat-controller.zdr-refusal.test.ts
- apps/Frontend Plane/verevonv3/scripts/build-product-demo.mjs, probe-product-demo.mjs, capture-demo-poster.mjs, demo-live-check.mjs, scene-preview.mjs; apps/Frontend Plane/verevonv3/src/demo/ (directory); apps/Frontend Plane/verevonv3/vite.demo.config.ts (demo tooling used by the live-only demo CI job)
- apps/Model Plane/api/contracts/zdr-refusals.json and .sha256
- apps/Model Plane/rust/crates/mp-contracts/src/zdr_refusal.rs (and tests/fixtures/, tests/zdr_retention_matrix.rs)
- apps/Model Plane/rust/services/model-gateway/tests/zdr_refusal_inventory.rs; apps/Model Plane/rust/services/execution-core/tests/zdr_refusal_inventory.rs; apps/Model Plane/rust/services/inference-core/tests/zdr_refusal_inventory.rs; apps/Model Plane/rust/services/inference-core/src/grpc/zdr_waiver_route_tests.rs
- .github/workflows/quarry-docs-contract.yml (repo-root gate for Quarry docs; out of WS12 scope, same pattern)

**Clean in live (no status entry)**: eslint.config.js; ChatArtifactPanel.tsx and its test; FleetRunConsole.tsx and its test; ChatPanels.tsx and its test; ChatLiveRunPanel.tsx; BrowserChrome.tsx; chat-localization.test.ts; chat-accessibility.test.ts; src/shared/i18n/index.tsx, en.ts, no.ts, locales.ts, errors.test.ts; verevon-web DESIGN.md; sse-taxonomy.yml; buf.yml; contract-goldens.yml; gen-sse-event-taxonomy.py; lint-ratchet.mjs; zdr-contract-schema.test.mjs; repo-root coresystem-conformance.sh and coresystem-cross-plane-preflight.sh; gateway.proto; orchestration.proto; apps/Frontend Plane/verevonv3/apps/gateway/scripts/*.test.mjs; apps/Frontend Plane/verevonv3/apps/gateway/src/domains/actions/handlers.rs; apps/Model Plane/api/contracts (connection-zdr-capabilities.json, zdr-posture-matrix.json, infer-statuses.json).

## 7. Not determined

- Whether `buf lint` and `buf breaking` (buf.yml:21, :23) pass or check anything, given the unquoted path. Not run. The ledger (apps/Model Plane/docs/decisions/ledger.md, section "P0 critical-path list verified item-by-item (2026-08-26)", around line 2418-2479) records 1,502 buf lint errors across 20 protos, so CI state is unclear.
- Whether `pnpm lint` runs, given the conflicting TypeScript claim (chat-localization.test.ts:16-18 vs package.json:49). Not run.
- The true count of bare user-facing Norwegian strings in scope. The 126-line scan mixes false positives and misses ASCII-only words. No AST scan was done.
- Whether the card's second `no-restricted-syntax` block works under ESLint flat-config override rules. The file's own comment (eslint.config.js:104-105) says a second key replaces the first, so re-spreading is required. Not executed.
- Whether `gen-sse-event-taxonomy.py --check` passes on L (not run). The live doc gained three rows, which suggests it was updated for the new variant.
- Field-level drift between sse_events.rs payload() and chat-client.ts reads. No extractor exists. names_and_payload_shapes_are_stable covers only usage and citation payloads.
- Whether any size ratchet threshold is intended. No threshold is defined anywhere.
- Whether any tracing or log macro in model-gateway or execution-core emits raw user content. No log-content gate exists and no audit was done.
- Tests for the chat approval buttons, Pause fleet, ChatLiveRunPanel status words and BrowserChrome labels: none found by name; the grep does not rule out tests under other names.
- Reference-practice claims in both cards ([SRC] items such as Hermes, DeepSeek, Claude desktop ICU counts) were not verified.
- The "state checklist" document the card asks for: not found in verevonv3 docs/*.md, FRONTEND_PLANE_STATUS.md or FRONTEND_PLANE_ROADMAP.md (grep, not exhaustive).
- Default locale and whether the Norwegian-only controls are visible to English users (apps/Frontend Plane/verevonv3/src/shared/i18n/locales.ts not read).
- Whether repo-root CI scripts have a plane owner. The master matrix has no row for them.
- Whether the live tree passes its own CI gates, including the new demo and receipt jobs.
- What the `.githooks/pre-commit` exclusion covers beyond the product-demo bundle (the hook is a secret scan).
