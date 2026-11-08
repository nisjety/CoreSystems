# WS19-office-deliverables - Verified Office deliverables

Audit item: F-t3-014 (Priority, Partial, XL, W2). Written 2026-10-15 against worktree `C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b` (HEAD afb83928, clean) and the live tree `C:/dev/CoresSystem` (same HEAD + uncommitted work). Abbreviations: `MP` = `apps/Model Plane`, `RS` = `MP/rust/services`, `FE` = `apps/Frontend Plane/verevonv3`. Paths are worktree paths unless marked [live-only]. Every path:line below was opened in this pass unless it says "(sweep)".

## 1. Scope, items resolved, owner

**Item:** F-t3-014 only. **Owner plane:** Model Plane (execution-core = verifier + sandbox mount, model-gateway = report relay + `skill_file`, capability-core = bundle registry). **Supporting:** Frontend Plane (artifact panel registry + previewer). No Control, Data or Ingestion Plane change.

**Card split, kept as three tracks:** (a) verify-and-render step inside `code_interpreter` + LibreOffice/fonts image layer; (b) SKILL.md bundles in a content-addressed store + `skill_file` read tool + read-only sandbox mount; (c) renderer registry + Office previewer in the artifact panel.

**IN**
- A first-party verifier that runs on every Office file (`docx xlsx pptx pdf`) that `code_interpreter` produces: structural checks (always), then LibreOffice render + xlsx recalculation + thumbnails (when the image layer is present). A typed report goes to the model (so it repairs) and to the user (badge, preview).
- Bundle format, store, read tool and read-only mount for SKILL.md bundles; four first-party Office bundles (docx, xlsx, pptx, pdf).
- A renderer registry in the artifact panel replacing the set-based `artifactRenderKind`, and a server-rendered Office preview.
- Hermetic guarantees preserved: no network, per-call workspace deleted on every path, `--clearenv` allowlist, `cap.command.sandbox` binding unchanged.

**OUT**
- Attachments / editing an uploaded file in place (WS08 S5/S6, plan G6). This stream only reuses WS08's read-only bind plumbing.
- Deck pipeline and brand templates (F-t3-038), skill lifecycle/promotion UI (F-t3-058, WS07), third-party/imported bundles (F-t3-059), per-thread sandbox computer (WS22), typed file tools (WS24).
- The model "looking at" thumbnails (needs image parts inside tool results; D9). V1 gives the model deterministic layout lint, not pixels.
- Replacing the in-browser export (`FE/src/features/chat/lib/chat-export/index.ts:159-175`): it stays (audit section 8 strength).

| Slice | Tag | Title | Effort |
|---|---|---|---|
| S0 | [BUILD] | LibreOffice-under-bwrap feasibility spike + fidelity corpus | S, 3 d |
| S1 | [FINISH] | FE renderer registry (behaviour-preserving refactor) | S, 2-3 d |
| S2 | [BUILD] | Structural verifier in execution-core + shared report contract + model-facing report | M, 6-8 d |
| S3 | [BUILD] | Verification reaches the user: `check` on the attachment event, badge, enforce policy | M, 5-6 d |
| S4 | [BUILD] | LibreOffice + fonts image layer: render, recalc, thumbnails, preview payload | L, 9-11 d |
| S5 | [BUILD] | Office previewer in the artifact panel | M, 5-6 d |
| S6 | [BUILD] | Skill bundle store + API in capability-core | L, 8-10 d |
| S7 | [BUILD] | `skill_file` tool + three-stage disclosure | M, 5-6 d |
| S8 | [BUILD] | Read-only `/skills` mount in the sandbox | L, 8-10 d |
| S9 | [BUILD] | Four first-party Office bundles | M, 5-6 d |
| S10 | [BUILD, deferred] | Do-loop parity and Do file delivery (outline) | not sized |

Nothing in this stream is purely [FINISH] except S1: the verifier, image layer, bundle store and previewer do not exist today (grep, sweep section 4 "Not present").

## 2. Verified current state

### 2.1 What exists (opened)

| Fact | Evidence |
|---|---|
| `code_interpreter` runs in bwrap: `WorkspaceWrite` over one per-call root, `network: Disabled`, env is an allowlist (`PATH, HOME, TMPDIR, MPLCONFIGDIR, MPLBACKEND, PYTHONDONTWRITEBYTECODE, LANG`); workspace removed by a Drop guard | `RS/execution-core/src/code_interpreter.rs:1-34, 305-348, 430-452, 579-582` |
| Output = top-level regular files only (symlinks and dirs skipped), 8 MiB per file, 16 MiB total, over-cap files listed with `truncated:true`; `.py`/scratch dirs excluded | `code_interpreter.rs:79-82, 642-715` |
| bwrap argv: `--ro-bind / /` baseline, writable roots bound after, `--unshare-net` when Disabled, no `--proc /proc`; `LaunchOptions.extra_ro_binds` exists (emitted after writable binds) and **no production caller passes it** | `RS/execution-core/src/sandbox.rs:91-113, 156-224`; `executor.rs:250-263` (`execute_sandboxed_in_dir` builds `LaunchOptions` without it) |
| Timeout: code default **120 s** (`EXECUTION_CODE_TIMEOUT_SECS`); the tool description tells the model "hard ~30s" | `executor.rs:53`; `RS/model-gateway/src/tool_loop.rs:4307` (WT) |
| Image: `debian:bookworm-slim`, runs as uid 10001, apt python libs (openpyxl, xlsxwriter, python-docx, reportlab, pypdf, pillow, lxml, numpy, pandas, matplotlib), `python-pptx==1.0.2` via pip; no LibreOffice, no poppler, no extra fonts | `RS/execution-core/Dockerfile:41-97` |
| compose: `execution-core` has a scoped seccomp profile for bwrap, no memory/CPU limit visible in its block | `MP/deploy/docker-compose.yml:1005-1020` |
| Only the **Ask loop** turns produced files into UI artifacts: gateway `code_interpreter_events` maps `files[]` to `artifact` + `attachment` events (same data URI twice, id `{call}-file-{n}`); `content_b64` appears in no other service or FE file (grep) | `RS/model-gateway/src/tool_loop.rs:365, 479-571`; `artifacts.rs:481-511` |
| Binary events (`data:` URIs) bypass the resume buffer | `RS/model-gateway/src/sse_events.rs:726-732, 972` |
| `ChatEvent::Attachment {id,name,mime,url,size}` has 5 construction sites; SSE family `artifacts`; payload JSON built at `sse_events.rs:506-513` | `artifacts.rs:482, 801`; `sse.rs:5899`; `sse_events.rs:1620`; `tool_loop.rs:10957` |
| A different SSE event already named `verification` exists (claim verification, family `citations`): do not reuse the name | `sse_events.rs:355-357, 413` |
| ZDR: `ExecuteStep` refuses every tool when the caller or request is ZDR ("disabled until its lifecycle is persistence-free"), so `code_interpreter` is wholly unavailable under ZDR today | `RS/execution-core/src/grpc.rs:650-661, 881` |
| Health probe imports the advertised python modules (not under bwrap); no LibreOffice/render probe | `RS/execution-core/src/health_attest.rs:156-179, 606-700` |
| Both loops already have a skill read-back tool (`reattach_skill`) sharing one contract module; gateway reads skills from **session-core** `ListAgentSkills`; skill injection is keyword-overlap, <=3 skills, 8,000-char budget (ledger 2026-08-22) | `tool_loop.rs:3291-3330`; `RS/execution-core/src/runtime_loop/mod.rs:2007-2018`; `MP/rust/crates/mp-contracts/src/skill_recovery.rs:1-40`; `skills.rs:306-308`; `MP/docs/decisions/ledger.md:1470` |
| `agent_skills` is written by capability-core SQL and by session-core; both services use the same database `session_core` | `MP/deploy/docker-compose.yml:723, 1388`; `go/services/capability-core/internal/api/registry_apis.go:46, 110`; `RS/session-core/migrations/0004_tasks_cron_hooks_skills_memory.sql:191` |
| `skill_packages` / `skill_resources` exist but are TEXT-only, capability-registry shaped, no Go reader | `go/services/capability-core/migrations/0003_capabilities_registry.up.sql:134-173` |
| `workspace_cas.rs` is the only object-store client; Rust only; key `cas/<sha256>` is **global** (no org prefix); no MinIO/S3 code in any Go service (`grep MINIO *.go`: 0 hits) | `RS/execution-core/src/workspace_cas.rs:1-16, 36, 140-172` |
| execution-core and model-gateway both hold a tonic `CapabilityCoreClient` | `RS/execution-core/src/grpc.rs:1336-1340`, `capability_policy.rs:14, 382`; `RS/model-gateway/src/state.rs:134, 408` |
| Artifact panel: `artifactRenderKind` is set-based; `pdf` with a data/http source -> iframe, docx/xlsx/pptx -> download card | `FE/src/features/chat/components/chat-artifacts.ts:25, 65-68, 196-209`; `ChatArtifactPanel.tsx:448-460, 558-575` |
| Join point artifact -> generated file already exists | `chat-media-markdown.tsx:66` (`selectGeneratedFileForArtifact`); `chat-artifacts.ts:332, 358`; `GeneratedFile` `chat-types.ts:140-146`; guard `chat-normalizers.ts:1050`; handler `use-chat-controller.ts:3076-3083`; SSE read `chat-client.ts:1297, 1523` (native + AG-UI custom envelope) |
| H5 proof harness for `code_interpreter` exists: spec, TS helper, python oracle | `FE/tests/e2e/product-code-interpreter.spec.ts`, `code-interpreter-proof.ts:1-40`, `tests/e2e/fixtures/code-interpreter/h5_oracle.py`, `tests/code-interpreter-proof.test.ts` |

### 2.2 Live-only / differs-in-live (must land first)

| File | Why | Must land first |
|---|---|---|
| `RS/model-gateway/src/tool_loop.rs` (M +370/-55; code_interpreter def at live 4319-4322, arm 2554, files_in 2580) | S2/S3/S7 edit def, arm, events fn, tool allowlists | WS01 commits the live wave |
| `RS/model-gateway/src/sse_events.rs` (M +202/-1, adds `RetentionClass`; Attachment name at live 471) | S3 adds fields to `Attachment` | WS01 |
| `RS/model-gateway/src/sse.rs` (M +814/-69), `RS/execution-core/src/grpc.rs` (M +76/-22), `runtime_loop/mod.rs` (M +18), `MP/rust/crates/mp-contracts/src/lib.rs` (M +2) | S3 (sse.rs:5899 literal), S8 (grpc.rs, runtime arm), contract `pub mod` line | WS01 |
| `RS/model-gateway/tests/zdr_feature_table.rs`, `zdr_posture_matrix.rs`, `zdr_refusal_inventory.rs`, `MP/api/contracts/zdr-refusals.json` [live-only, untracked] | golden ZDR cases added by S2/S7 | WS01 |
| `FE/package.json` (M +2 scripts) | no conflict | n/a |

Clean in live (safe to edit now): `code_interpreter.rs`, `executor.rs`, `sandbox.rs`, `Dockerfile`, `workspace_cas.rs`, `health_attest.rs`, `tools.rs`, `artifacts.rs`, `skills.rs`, all capability-core, the four protos, `chat-artifacts.ts`, `ChatArtifactPanel.tsx`, `chat-export/*` (sweep section 8).

### 2.3 Where the audit text is stale or wrong (code wins)

1. Card cites `tool_loop.rs:4310-4325`: WT definition is 4305-4309 (live 4319-4322).
2. Card says a read-only bundle mount "is a new staging path". The primitive exists (`extra_ro_binds`) but was **never run live**; and a mount destination must exist on the host root because `--ro-bind / /` makes the root read-only (writable roots work only because they are real host dirs). S0 must prove a `/skills` mountpoint baked into the image works.
3. OD18 is a recommended default, not an owner ruling (`.plan-work/common/rules-ledger-adrs.md:258, 272`). "Terra" is the ChatGPT-subscription route (`code-interpreter-proof.ts:4`), not an environment; the H5 live-proof harness already exists in the repo (whether it ever ran live: not determined).
4. Tool description says "hard ~30s timeout"; real default is 120 s (`executor.rs:53`). Fix in S2 so the model plans render-heavy work correctly.
5. The card implies Office files from `code_interpreter` reach users generally. Only the Ask loop delivers them; a Do run gets base64 in tool output but no artifact path (S10).
6. `skills.rs:1-7` says operators preload skills "from disk". No loader exists (grep `SKILLS_DIR|SKILL.md|.skill.md` in `model-gateway/src`: 0). First-party skills need an explicit source (S9).
7. Sweep gap 6 resolved: capability-core and session-core share one DB (`session_core`, compose 1388 vs 723); skills are read by the gateway from session-core, written by both.
8. H11 ("post-write diagnostics", CHAT_GAP `:270`) is the same idea as S2; fold it in, do not build twice.
9. Audit section 8 lists "Office exports built in the browser" as a strength: S1/S5 keep `chat-export` untouched.

## 3. Decisions and open questions

| # | Decision | Options | Recommendation | Gates |
|---|---|---|---|---|
| D1 | OD18 (fonts, caps) is unruled | (a) wait; (b) proceed on the recommended default: metric-compatible fonts, 60k chars unchanged, caps stay WS08's | (b), reversible (font list is one Dockerfile line). Caps are WS08 S7, not here. | S4 font list only. S0-S3, S6-S8 free. |
| D2 | Failed verification: advisory or enforce | (a) always deliver + badge; (b) withhold a file whose report is `failed` once, tell the model, deliver on the second failed attempt of the same name in the turn; (c) block always | Ship (a); (b) behind `CODE_INTERPRETER_VERIFY_ENFORCE` once S0/S2 corpus shows < 2% false `failed` | S3 enforce half only |
| D3 | Blob store for bundles | (a) Postgres bytea keyed `(org_id, sha256)` in capability-core; (b) MinIO via `workspace_cas.rs` | (a) for v1. Deviates from the card's "reuse workspace_cas.rs": the writer is Go (no S3 client exists in Go), execution-core already has the capability-core channel, bundles are small (<= 8 MiB, <= 256 files), and the global `cas/` key would need an org prefix anyway. Revisit if a bundle cap is ever raised. | S6, S8 |
| D4 | Preview delivery | (a) server PDF, transient, in the attachment event; (b) client libs (docx-preview/SheetJS); (c) persisted previews | (a). (b) is the Space session's lazy xlsx grid (overlap-space #13, S567) and registers into S1's registry later. (c) needs the artifact revision store (M796). | S5 |
| D5 | Where LibreOffice runs | (a) in the execution-core image, inside the same bwrap; (b) a new render sidecar service | (a): no new service identity or file-shipping edge, same hermetic policy, no `SandboxBackend` trait (deferred in the ledger 2026-08-22). S0 measures image delta and cold start; switch to (b) only if the gate in S0 fails. | S4 |
| D6 | Who may author tenant bundles; third-party import | (a) org admin zip upload only; (b) also URL/marketplace import | (a). (b) waits for F-t3-059 supply-chain rules. Scripts only ever run inside `cap.command.sandbox`; every upload/delete is audited (WS06). | S6 |
| D7 | First-party skill delivery | (a) image-baked bundles + gateway-registered `builtin:` skills; (b) per-org seeded `agent_skills` rows | (a): no migration, no per-org write, `replace_learned` already leaves non-`session-core:` sources alone (`skills.rs:92-117` (replace_learned at 102)). One SKILL.md source file, two consumers, a contract test pins equality. | S9 |
| D8 | `skill_package*` tables | (a) reuse; (b) new tables | (b). Existing ones are TEXT-only, registry-shaped, reader-less; leave them for F-t3-058. | S6 |
| D9 | Model "look" at pages | (a) v1 text lint only; (b) tool-result image parts | (a). (b) needs inference-core image parts in tool results (WS08 S3 only covers user-message images): new item. | none |

No `docs/decisions/ledger.md` entry is reversed. Two entries to add: "LibreOffice runs in the sandbox image, not a sidecar (D5)" and "Office verification is advisory until measured (D2)". Slices that need no decision: S0, S1, S2, S3 (advisory), S7, S9-text. Needs integrator-ratified proto numbers: S8 (`ExecuteStepRequest` = 13), S6 (new messages only).

## 4. Slices (build order)

### S0. LibreOffice-under-bwrap feasibility spike + fidelity corpus [BUILD] (S, 3 d)

**Goal.** Replace three unmeasured assumptions (sweep gap 3): does `soffice` run under the exact bwrap argv, what does the layer cost, does a `/skills` mountpoint work on a read-only root.
**Files**
- NEW `MP/scripts/verify-office-render.sh` - modelled on `verify-sandbox-isolation.sh`: inside the built execution-core image as uid 10001 under the argv from `sandbox.rs:156-224` (no `/proc`, `--ro-bind / /`, HOME/TMPDIR in the workspace) runs `soffice --headless --norestore -env:UserInstallation=file://$HOME/.lo --convert-to pdf`; asserts exit 0, PDF header, no network namespace reachability, wall time, RSS peak.
- NEW `RS/execution-core/tests/fixtures/office/` - corpus: 4 good + 6 deliberately broken files (truncated zip, missing rel target, formula to missing sheet, `#DIV/0!` cell, empty pptx, pdf with 0 pages), each <= 30 KB. Also fixes the false-`failed` baseline for D2.
- Spike-only: a throwaway `--ro-bind <host-dir> /skills` run with `/skills` created in the image.
**Flag/ZDR.** None (tooling). **Tests.** The script plus `cargo test -q -p execution-core sandbox` unchanged. **DoD.** A one-page result in `MP/MODEL_PLANE_STATUS.md` (measured: image MB delta, cold and warm convert seconds, peak RSS, whether `/skills` mounts, whether macros/links update are disabled by profile). Gate: cold <= 25 s inside the 120 s ceiling, peak RSS <= 1 GiB, else D5 -> (b). **Evidence.** Captured script output from the rebuilt image (run via `MP/scripts/compose.sh`, per memory note use the `rust:1` container for Linux-gated tests on Windows). **Deps.** none. **Risks.** soffice needs `/proc` or a writable `/tmp`; result may force `--ro-bind /proc` (changes the isolation claim; refuse that and fall to structural-only).

### S1. FE renderer registry [FINISH] (S, 2-3 d, parallel with S0/S2)

**Goal.** One place decides how an artifact is shown, with no visible change.
**Files**
- NEW `FE/src/features/chat/components/artifact-renderers.ts` - `ArtifactRenderer { id, match(artifact, file?) -> boolean, priority }`, `registerArtifactRenderer`, `resolveArtifactRenderer`. Built-ins reproduce today's order: image, html, document, code, pdf(addressable), binary, text.
- `FE/src/features/chat/components/chat-artifacts.ts:196-209` - `artifactRenderKind` becomes a thin wrapper over the registry (keeps the exported `ArtifactRenderKind` union at :25 and every existing test at `chat-artifacts.test.ts:153-175`).
- `FE/src/features/chat/components/ChatArtifactPanel.tsx:425-470` - `Switch/Match` stays; the Match for a registered extra kind is a `Dynamic` slot (used by S5).
**Flag/ZDR.** None; no persistence. **Tests.** NEW `artifact-renderers.test.ts` (precedence, unknown kind -> binary, kind-less data URI -> binary, later registration beats built-in only when `priority` higher); existing `chat-artifacts.test.ts`, `ChatArtifactPanel.test.tsx` unchanged and green. `cd "apps/Frontend Plane/verevonv3" && pnpm vitest run src/features/chat/components/chat-artifacts.test.ts src/features/chat/components/ChatArtifactPanel.test.tsx src/features/chat/components/artifact-renderers.test.ts && pnpm typecheck`. **DoD.** Zero snapshot/test diff for existing kinds. **Evidence.** Dev stack (`:5173` caveat in memory: second dev server for a worktree) screenshot of an HTML, PDF, xlsx card before/after. **Deps.** none. **Risks.** WS12 i18n gate on the same file (no new strings here); conflict with WS09 on `ChatArtifactPanel.tsx` (rule in section 5).

### S2. Structural verifier + shared report contract + model-facing report [BUILD] (M, 6-8 d)

**Goal.** Every produced `docx/xlsx/pptx/pdf` is checked inside a second hermetic sandbox call; the model sees the report in the tool result and can repair before answering. Works on the **current image** (no LibreOffice).
**Files**
- NEW `MP/rust/crates/mp-contracts/src/office_verification.rs` (+ `pub mod office_verification;` in `lib.rs`, live-modified: add after WS01 lands) - `OfficeVerification { name, kind, level: passed|warnings|failed|skipped, checks: Vec<Check{id, severity, message<=200 chars}>, stats }`, caps consts (`MAX_FILES=4`, `MAX_CHECKS=40`, `MAX_MESSAGE=200`), `from_untrusted_json` (truncates, scrubs control chars). Shared so producer and consumer cannot drift (CLAUDE.md cross-service pattern).
- NEW `RS/execution-core/src/office_verify.rs` - orchestration: for each output file with an Office extension (<= `OFFICE_VERIFY_MAX_FILES`), create a second `Workspace` (reuse `code_interpreter.rs:305-348`, make `Workspace` `pub(crate)`), copy the file in, run the embedded checker under `MpSandboxPolicy::WorkspaceWrite{network: Disabled}` via `executor::execute_sandboxed_in_dir` with its own timeout (`OFFICE_VERIFY_TIMEOUT_SECS`, default 60) and a process-wide `tokio::sync::Semaphore` (`OFFICE_VERIFY_MAX_CONCURRENCY`, default 2). Read `report.json`; pass through `office_verification::from_untrusted_json`; `scrub::scrub_string` on every message (document content can reach messages).
- NEW `RS/execution-core/src/office_verify/check.py` embedded with `include_str!`, written into the verify workspace like `_verevon_main.py` (`code_interpreter.rs:548-550`). Checks: zip sane (entry count, uncompressed-size ratio, no `..`/absolute names), `[Content_Types].xml`, `_rels/.rels` and every relationship target exist, every XML part parses (lxml), main part present; docx opens in python-docx and has content, leftover placeholders (`lorem ipsum`, `{{`, `[insert`) -> warning; xlsx loads in openpyxl, counts sheets/formulas, formula referencing a missing sheet or defined name -> warning, literal error strings in cells -> failed; pptx opens, slide count, shape extending beyond slide bounds -> warning, empty slide -> warning; pdf reads in pypdf, >= 1 page, encrypted -> failed, no extractable text on a text PDF -> warning.
- `RS/execution-core/src/code_interpreter.rs:599-605` - `CodeResult` gains `#[serde(skip_serializing_if = "Vec::is_empty")] verification: Vec<OfficeVerification>`; called after `collect_output_files` (597) while the workspace is alive; never modifies returned bytes. Mode from `CODE_INTERPRETER_OFFICE_VERIFY`.
- `RS/execution-core/src/health_attest.rs` - no change in S2.
- `RS/model-gateway/src/tool_loop.rs` `code_interpreter_events` (479-571): copy `verification` (text only, never base64) into the `summary` JSON the model reads and append one sentence: failed -> "fix and regenerate under the same filename before telling the user it is ready"; passed -> "checked structurally; this does not prove it looks identical in Microsoft Office". Provenance stays `unscreened` (pattern at the arm, 2548-2600).
- `tool_loop.rs:4307` description (live 4320) - one sentence on automatic verification; correct "~30s" to the real ceiling; the library list is unchanged so the parity test `..._names_every_library_the_sandbox_image_ships` (10712, reads the Dockerfile) is untouched.
**Proto/API.** None: the report rides inside `ExecuteStepResponse.output` (`execution.proto:321`), as `files` does.
**Flag.** `CODE_INTERPRETER_OFFICE_VERIFY` = `off|structural|render`, default `off` in code, `structural` in dev compose after proof (compose block `MP/deploy/docker-compose.yml:1005+`); unknown value -> `off` + warn. Rollback: set `off`. Contract test `MP/scripts/tests/office-verify-compose-contract-test.sh` (pattern of the letta test in `flags-and-config.md`).
**ZDR.** Verification runs only inside `ExecuteStep`, which refuses ZDR (`grpc.rs:881`); nothing is persisted, workspace Drop-guarded, reports logged as counts only. Golden case: add `(code_interpreter office verify, ZDR)` -> refused in `zdr_feature_table.rs` [live-only]. If persistence-free ZDR execution later enables `code_interpreter`, verification inherits it unchanged (same step, same workspace lifecycle).
**Tests.** execution-core unit (`office_verify.rs`): `a_truncated_zip_reports_failed`, `a_missing_relationship_target_fails`, `a_valid_python_docx_passes`, `a_formula_to_a_missing_sheet_warns`, `zip_traversal_names_are_refused`, `report_text_is_scrubbed_and_capped`, `non_office_files_are_skipped`, `returned_file_bytes_are_never_modified`, `verify_workspace_is_removed_on_timeout`, `concurrency_is_bounded`; `code_interpreter.rs`: `verification_key_absent_when_mode_off`; mp-contracts: `from_untrusted_json_truncates_and_rejects_unknown_level`; model-gateway: `code_interpreter_summary_carries_verification_and_no_base64`, existing 11048/11151 tests unchanged. Run: `cd "apps/Model Plane/rust" && cargo test -q -p execution-core --all-targets && cargo test -q -p mp-contracts && cargo test -q -p model-gateway --all-targets` (Linux-gated bwrap tests in the `rust:1` container; Windows: two cargo targets fail pre-existing, use `--lib`).
**DoD.** Corpus from S0: every broken file `failed`, every good file `passed`. **Evidence.** Rebuild execution-core + model-gateway via `MP/scripts/compose.sh`, mode `structural`; Playwright `product-code-interpreter.spec.ts` flow (`code-interpreter-proof.ts` cases) plus one prompt that makes the model write a corrupt `.docx`: capture the SSE `tool_result` with `verification[].level=failed`, the model's repair call, the second result `passed`, and the execution-core log lines (counts, no content); `ls` of the temp dir proves no `verevon-code-*` remains. **Deps.** S0 corpus (not a blocker for coding), WS01 live wave for `tool_loop.rs`. **Risks.** False `failed` on unusual-but-valid files (corpus + advisory default); verifier cost doubles Office-turn latency (structural step is a python start, ~1-3 s, measured in S0).

### S3. Verification reaches the user [BUILD] (M, 5-6 d, depends S2)

**Goal.** The user sees "checked / warnings / failed" on the file, honestly worded; optional withhold-once.
**Files**
- `RS/model-gateway/src/sse_events.rs:221-227, 506-513, 726-732` - `ChatEvent::Attachment` gains `check: Option<OfficeVerification>` (and `preview: Option<…>` reserved for S5, absent now); serialised only when `Some` (older clients ignore the key; no new event name, so `sse-event-taxonomy.md` rows and the WS12 taxonomy gate are unchanged). Update the 5 literal sites (`artifacts.rs:482, 801`; `sse.rs:5899`; `sse_events.rs:1620`; `tool_loop.rs:10957`).
- `tool_loop.rs` `code_interpreter_events` - attach `check` to the matching `attachment_event`; `artifacts.rs:481` signature gains `check`.
- Enforce (flag-gated): in the same function, a file with `level=failed` produces no `artifact`/`attachment` event on the first failing attempt of that name in the turn; the summary says "withheld". Per-turn counter local to the tool-loop call (no cross-replica state). A second failed attempt delivers with `check` so the user is never left with nothing.
- `FE/src/features/chat/components/chat-types.ts:140-146` - `GeneratedFile.check?`; `chat-normalizers.ts:1050, 1296` - guard and normaliser accept it; `chat-client.ts:1297, 1523` - read `check` in both the native and the AG-UI custom paths; `use-chat-controller.ts:3076` unchanged apart from the type.
- Generated-file card (where `ChatMessages` renders `turn.files`; component not opened) - chip "Kontrollert" / "Advarsler" / "Feil funnet" with `i18n.tr(no, en)` pairs and the list of messages; wording never says "verified in Office".
**Flags.** `CODE_INTERPRETER_VERIFY_ENFORCE` (model-gateway, opt-in, default off). **ZDR.** The check is a short text list stored with the turn exactly like the file card; it exists only when `code_interpreter` ran, which ZDR forbids; golden case reuses S2's row. **Tests.** model-gateway: `attachment_event_carries_check_only_when_present`, `a_failed_file_is_withheld_once_then_delivered` (enforce on), `enforce_off_delivers_failed_with_check`; sse golden payload (WS12 S7) updated; FE: `chat-client.test.ts` (check read on both paths), `chat-normalizers` guard test, `ChatMessages.generated-files.test.tsx` (chip text en/no, no chip when absent). Commands as S1 + `cargo test -q -p model-gateway --all-targets`. **DoD.** Corrupt file -> chip "Feil funnet" (advisory) or withheld then repaired (enforce); clean file -> "Kontrollert". **Evidence.** Same live prompt pair as S2 with screenshots of both chips and the SSE `attachment` frame containing `check`. **Deps.** S2, WS01, WS12 gates. **Risks.** Persisted threads with `check` must still pass `isGeneratedFile`; mitigated by the guard test.

### S4. LibreOffice + fonts image layer: render, recalc, thumbnails [BUILD] (L, 9-11 d, depends S0, S2)

**Goal.** `render` mode: convert to PDF in the sandbox, count pages, recalculate xlsx, list substituted fonts, lint layout, and produce a preview PDF and first-page thumbnails.
**Files**
- `RS/execution-core/Dockerfile` - a **separate** `RUN` after line 87 (keeps the Python apt list at 66-77 untouched so the parity test that parses it still passes; read `tool_loop.rs:10664-10732` before editing): `libreoffice-core libreoffice-writer libreoffice-calc libreoffice-impress poppler-utils` + metric-compatible fonts (`fonts-liberation2`, `fonts-crosextra-carlito`, `fonts-crosextra-caladea`, `fonts-dejavu-core`, a Noto subset) - exact names verified in S0 as the file's own comment demands; `fc-cache -f`; pre-warmed LibreOffice profile at `/opt/verevon/lo-profile` (copied into the workspace per call); empty `/skills` dir (S8). Extend the comment block at 53-60.
- `office_verify/check.py` (S2) - render path: copy the profile, `soffice --headless --norestore -env:UserInstallation=… --convert-to pdf`, macro security max and link update off in the profile; page count (pypdf); for xlsx also `--convert-to xlsx` into a scratch copy and read cached values with `openpyxl data_only=True` for `#REF! #DIV/0! #NAME? #VALUE! #N/A` (the shipped file is never replaced by the recalculated copy); `pdffonts` vs requested fonts -> `fonts_substituted`; `pdftoppm -r 60 -png -f 1 -l 3`; blank-page check on thumbnails with Pillow; pptx slide count vs rendered page count; docx/pdf page-count sanity. Outputs: `report.json`, `preview.pdf`, `p1.png..`.
- `RS/execution-core/src/office_verify.rs` - collect preview artifacts under caps (`OFFICE_PREVIEW_MAX_BYTES` 2 MiB PDF, <= 3 thumbs <= 150 KB each) into the report as `render { status: ok|skipped|failed|unavailable, pages, fonts_substituted, preview_pdf_b64?, thumbs_b64[] }`; a missing/hung `soffice` yields `render.status=unavailable` and a `warning`, never `failed`.
- `RS/execution-core/src/health_attest.rs` - separate `office_render_ready` probe (renders a 1-page fixture under bwrap). It must **not** feed `cap.command.sandbox` health: a LibreOffice fault must not disable the sandbox capability.
- `MP/scripts/verify-sandbox-isolation.sh` - add the LibreOffice case (no network, writes only to the workspace).
- `tool_loop.rs:4307` description: one sentence naming the render checks; parity test untouched.
**Proto.** none. **Flag.** `CODE_INTERPRETER_OFFICE_VERIFY=render`; `OFFICE_VERIFY_TIMEOUT_SECS`, `OFFICE_VERIFY_MAX_CONCURRENCY`. Rollback: set `structural`; the image layer is inert. **ZDR.** Same as S2; preview bytes live only in the step result; nothing written outside the Drop-guarded workspace; a ZDR step cannot reach this path.
**Tests.** Linux-gated, skipped without `soffice`: `libreoffice_converts_a_docx_to_pdf_under_the_bwrap_policy`, `recalc_reports_div0_in_a_formula_cell`, `a_missing_font_is_listed_as_substituted`, `soffice_timeout_is_killed_and_reports_unavailable`, `render_argv_has_unshare_net_and_no_extra_binds`, `preview_over_cap_is_dropped_not_truncated`, `shipped_xlsx_bytes_are_not_the_recalculated_copy`; `health_attest.rs`: `office_probe_does_not_change_sandbox_capability_health`. Commands: `cargo test -q -p execution-core --all-targets` in the `rust:1` image plus `bash "apps/Model Plane/scripts/verify-office-render.sh"` against the built image. **DoD.** Corpus passes in render mode; cold render inside the S0 budget; image size delta recorded. **Evidence.** Rebuilt execution-core image (record size before/after), running stack, the H5 prompts: capture `verification[].render` frames with pages, a deliberately `=1/0` sheet flagged, a pdf/docx with a missing-font substitution listed, execution-core log lines with the bwrap argv. **Deps.** S0 result and D5, D1, S2. **Risks.** LibreOffice parser CVEs on model-authored files (contained: separate workspace, no network, timeout, uid 10001; Debian security rebuild cadence is an ops item for WS01), cold start, concurrent OOM (semaphore), fidelity gaps vs Microsoft Office (wording).

### S5. Office previewer in the artifact panel [BUILD] (M, 5-6 d, depends S1, S3, S4)

**Goal.** docx/xlsx/pptx open in the panel as pages, with a "fonts substituted" notice.
**Files**
- `sse_events.rs` + `tool_loop.rs`: `Attachment.preview { mime:"application/pdf", url:<data URI>, pages, fonts_substituted }` populated from S4; `carries_binary` (726) also true when `preview` is present, so the larger frame is never buffered; thumbnails go in `preview.thumbs` (data URIs).
- `FE/src/features/chat/components/chat-types.ts` `GeneratedFile.preview?`; normaliser/guard/handlers as S3. **Previews are transient**: stripped when a thread is snapshotted (the file's `check` persists, the PDF does not); after reload the panel shows the download card plus the stored check.
- NEW `FE/src/features/chat/components/ArtifactOfficePreview.tsx` and a registry entry (id `office-pdf`, matches docx/xlsx/pptx artifacts whose `selectGeneratedFileForArtifact(...)` file has `preview`): reuses the existing sandboxed PDF frame pattern (`ChatArtifactPanel.tsx:448-456`), page strip from thumbs, notice "Skrift erstattet i forhåndsvisningen" / "Fonts substituted in preview". Falls back to `ArtifactFileCard` when no preview.
- Keeps `chat-export` and its Word/PowerPoint menu (`ChatArtifactPanel.tsx:229-255`) untouched.
**Flag.** none (a preview appears only when the payload carries one). **ZDR.** n/a (no code_interpreter under ZDR); previews never persisted. **Tests.** FE: `ArtifactOfficePreview.test.tsx` (preview present -> frame + notice; absent -> download card; reload simulation drops preview, keeps chip), `artifact-renderers.test.ts` extended, `ChatArtifactPanel.test.tsx` (existing 187 xlsx download test still passes when no preview); model-gateway: `carries_binary_is_true_for_a_preview_attachment`. NEW e2e `FE/tests/e2e/office-deliverables.spec.ts` (stack-bound; `pnpm test:e2e -- office-deliverables.spec.ts`; extend `h5_oracle.py` to open the shipped file). **DoD.** An xlsx with a chart renders pages in the panel and downloads the original file byte-identical. **Evidence.** Live Ask turn producing xlsx, docx, pptx; screenshots of the panel and of the download byte hash equal to the sandbox output hash. **Deps.** S1, S3, S4; WS09 (panel edit order), WS12 (i18n gate, bundle budget: the component is not lazy-heavy, it adds no library). **Risks.** Frame size on slow links (cap 2 MiB), PDF viewer differences across browsers.

### S6. Skill bundle store + API in capability-core [BUILD] (L, 8-10 d)

**Goal.** A SKILL.md bundle (SKILL.md + `references/`, `templates/`, `scripts/`) is uploaded once, stored content-addressed per org, listed and read back by manifest, behind an admin-only API.
**Files**
- NEW `go/services/capability-core/migrations/<next>_skill_bundles.up.sql` (+ `.down.sql`): `skill_bundles(id, org_id, skill_id /* soft ref agent_skills.id */, manifest_sha256, file_count, total_bytes, created_by, created_at, updated_at, deleted_at)` unique `(org_id, skill_id)`; `skill_bundle_files(bundle_id, path, sha256, size_bytes, kind, executable)`; `skill_bundle_blobs(org_id, sha256, content BYTEA, size_bytes)` primary key `(org_id, sha256)` (content addressing and per-org dedup; never global). No FK into `agent_skills` (its DDL belongs to session-core migrations).
- NEW `go/services/capability-core/internal/skillbundle/` (`manifest.go`, `validate.go`, `store.go`) - zip in, manifest out; rules: <= 256 files, <= 8 MiB total, <= 1 MiB per file, paths relative, no `..`, no symlinks, no absolute, UTF-8 names, `SKILL.md` required with front-matter `name`, `description` (<= 1024 chars), executable bit only under `scripts/`; manifest digest = sha256 over sorted `(path, sha256)`.
- `go/services/capability-core/internal/api/registry_apis.go` (SkillsHandler, route `:65`) - `PUT /api/v1/skills/{id}/bundle` (zip, org-admin only, flag-gated), `GET /api/v1/skills/{id}/bundle` (manifest), `DELETE`. Skill delete cascades the bundle. Upload/delete emit audit events through WS06's emitter.
- `proto/model_plane/v1/capabilities.proto` (205 lines; new RPCs on `CapabilityCore`, new messages, so no field collision): `ListSkillBundles(org)`, `GetSkillBundleManifest(skill_id)`, `GetSkillBlob(skill_id, sha256)`; messages `SkillBundleSummary{skill_id=1,name=2,manifest_sha256=3,file_count=4,total_bytes=5,updated_at=6}`, `SkillBundleFile{path=1,sha256=2,size_bytes=3,kind=4,executable=5}`, `SkillBundleManifest{skill_id=1,manifest_sha256=2,files=3}`, `GetSkillBlobRequest{skill_id=1,sha256=2}`, `GetSkillBlobResponse{content=1}`. Server handlers in `internal/server/server.go` beside `ValidateSkillBundle:566-579`, org from `verifiedOrganizationID(ctx)`. Regenerate with `buf generate`; `buf breaking` must stay green (add-only).
- `ValidateSkillBundle` (`server.go:566`) is unchanged (it validates the registry row, not bundle files).
**Flag.** `SKILL_BUNDLES_ENABLED` (capability-core, opt-in, default off; upload route 404 when off). Rollback: flag off; tables remain. **ZDR.** A bundle is org configuration, not conversation content: no ZDR refusal at rest; nothing from a user turn is written. Under ZDR the existing skill-injection rule applies unchanged to bundle skills (S7 states it precisely).
**Tests.** Go: `skillbundle/validate_test.go` (zip-slip, symlink, oversize, bad front-matter, duplicate path, exec bit outside scripts, manifest digest determinism), `registry_apis_bundle_test.go` (admin-only, org isolation: org B cannot read org A's blob by digest, delete cascade), `server_test.go` RPC tests. `cd "apps/Model Plane/go/services/capability-core" && go test ./... && go vet ./...`. **DoD.** Upload a sample bundle, read manifest and each blob back with matching digests; cross-org read refused. **Evidence.** Rebuilt capability-core (use `scripts/run-control-plane.sh` for Control; Model Plane via `compose.sh`), curl transcript of upload, manifest, blob, cross-org 404, audit event present. **Deps.** D3, D6, D8; WS06 emitter (stub allowed first), WS07 (skill revisions: bundle changes must appear as a revision action; coordinate). **Risks.** Postgres bloat from bytea (caps above); `agent_skills` has two writers (WS07 D9): bundle writes do not touch it.

### S7. `skill_file` tool + three-stage disclosure [BUILD] (M, 5-6 d, depends S6)

**Goal.** Stage 1 = name/description (match), stage 2 = SKILL.md body (injected, unchanged), stage 3 = `skill_file(skill, path)` reads one referenced file on demand.
**Files**
- NEW `MP/rust/crates/mp-contracts/src/skill_bundle.rs` - shared contract (pattern of `skill_recovery.rs`): path validation, per-read cap (32 KiB text, `offset` paging, binary -> metadata only), result enum. Used by both loops, no shared dispatch.
- `RS/model-gateway/src/tool_loop.rs` - new `"skill_file"` arm beside `reattach_skill` (3291); read-only, org from the verified request, visibility via the same ownership rule `reattach_skill` uses; backed by `state.capability_client` (`state.rs:134`). Add to `conversation_tool_allowed` (4213-4216) and the tool list; `inline_tool_allowed` (615) already admits it; update the enumerating tests (`7632, 9805, 10544, 10574, 10610`). Provenance `org_internal`.
- `RS/model-gateway/src/skills.rs` + `sse.rs:5530` (`fetch_skill_context`) - extend the per-org pull with `ListSkillBundles`; `format_skill_block` (`skills.rs:306`) appends a footer for bundle skills ("Files: N; read with skill_file(skill, path); scripts at /skills/<name>/ inside code_interpreter") counted inside the 8,000-char budget (`tests/skill_budget_contract.rs` extended).
- Do-loop parity deferred to S10; shared contract keeps it cheap.
**Declared class.** `read` (WS05 vocabulary); `cap.command.sandbox` untouched. **Flag.** `SKILL_FILE_TOOL_ENABLED` (gateway, opt-in, default off). **ZDR.** `skill_file` is read-only and writes nothing; it follows whatever `fetch_skill_context` does on ZDR turns (not determined in this pass: S7 must read `sse.rs:5530` and encode the same gate), with a golden row. **Tests.** `skill_file_reads_only_this_orgs_bundle`, `skill_file_refuses_traversal_and_binary`, `skill_file_pages_at_the_cap_with_a_marker`, `bundle_footer_stays_inside_the_skill_budget`, plus mp-contracts unit tests. **DoD/Evidence.** Live turn with the sample bundle: matched skill block shows the footer; model calls `skill_file`; capture the tool_call/tool_result and the org-B negative. **Deps.** S6, WS01 live wave, WS05 (class row), WS07 (skills cache buckets in `skills.rs`: WS07 edits first). **Risks.** Tool-name proliferation in the offered set (token cost): offered only on turns where a matched skill has a bundle.

### S8. Read-only `/skills` mount in the sandbox [BUILD] (L, 8-10 d, depends S6, S7; shares seam with WS08 S5)

**Goal.** Tenant bundle scripts and templates are usable from `code_interpreter` as read-only files at `/skills/<name>/...`, hermetic as before.
**Files**
- `MP/proto/model_plane/v1/execution.proto` - NEW `message SkillMount { string skill_id = 1; string name = 2; string manifest_sha256 = 3; }`; `ExecuteStepRequest` (highest tag **11** `space_id`, line 317, verified) += `repeated SkillMount skill_mounts = 13;` (WS08 S5 takes 12; integrator confirms).
- `RS/model-gateway/src/tool_loop.rs:2554-2570` + `tools.rs:163-215` - pass mounts derived **server-side** from the skills matched this turn (<= 3); the model cannot name a bundle (its `files_in` is unchanged).
- NEW `RS/execution-core/src/skill_mounts.rs` - fetch manifest + blobs through the existing capability channel, verify every sha256 and the manifest digest, materialise into a fresh temp dir `<tmp>/verevon-skills-<call>/<name>/` (dirs 0555, files 0444, scripts 0555), RAII-removed, bounded in-memory LRU of verified blobs keyed `(org, sha256)`.
- `RS/execution-core/src/executor.rs:250-263` and `code_interpreter.rs:583-592` - `execute_sandboxed_in_dir` takes `ro_binds: &[(PathBuf, PathBuf)]` into `LaunchOptions.extra_ro_binds` (`sandbox.rs:112`). **WS08 S5 adds the same parameter**: whichever stream merges first adds it, the other appends its pair (the vector makes a second pair additive). Mount pair: `(<tmp>/verevon-skills-<call>, /skills)`; `/skills` pre-exists in the image (S4 Dockerfile) because the read-only root cannot create mountpoints (S0 proves it); `skills` and `attachments` are reserved names in `validate_input_name` (`code_interpreter.rs:227-264`).
- Passthrough hosts (no bwrap): refuse mounting rather than weaken the claim (`require_requested_isolation` pattern, `executor.rs:149-160`).
**Flag.** `SKILL_BUNDLE_MOUNT_ENABLED` (gateway, opt-in, default off); execution-core honours the field when present. **ZDR.** `ExecuteStep` is refused under ZDR (`grpc.rs:881`) before any fetch; no bundle content is logged (digests and counts only); mount dir deleted on every path. **Tests.** `skill_mounts.rs`: `a_blob_with_a_wrong_digest_is_refused`, `mount_dir_is_removed_on_every_path`, `script_can_read_but_not_write_the_mount`, `names_are_reserved`, `lru_never_serves_another_orgs_blob`; `sandbox.rs`: `skill_mount_follows_the_writable_root_in_argv`; model-gateway: `skill_mounts_come_from_matched_skills_only`; extend `tests/cross_service_loop_contract.rs` (caps consts shared via mp-contracts). **DoD.** `python /skills/<name>/scripts/x.py` runs in the sandbox, `open('/skills/..','w')` fails with a read-only error, no network. **Evidence.** Live Ask turn; capture the bwrap argv with `--ro-bind` to `/skills`, the EROFS result, `verify-sandbox-isolation.sh` still green, temp dir clean. **Deps.** S6, S7, S4 (Dockerfile `/skills`), WS08 S5 (seam), WS01. **Risks.** Merge order with WS08 on `executor.rs`/`code_interpreter.rs`; bundle fetch latency per call (LRU); a Space persistent root must not contain `skills`.

### S9. Four first-party Office bundles [BUILD] (M, 5-6 d, depends S4, S7)

**Goal.** docx, xlsx, pptx, pdf SKILL.md bundles that tell the model how to build then verify, with templates and a `verify` wrapper script.
**Files**
- NEW `MP/skills/office/{docx,xlsx,pptx,pdf}/SKILL.md` (+ `references/`, `templates/`, `scripts/`) - single source files. Each SKILL.md states: build with the shipped libraries, read `verification` and repair, never claim fidelity in Microsoft Office; xlsx: live formulas + native charts + the recalc finding; pptx: slide-count and bounds rules; pdf: font rule per D1.
- `RS/execution-core/Dockerfile` - `COPY` the directory to `/opt/verevon/skills/office` (visible through `--ro-bind / /`, no mount needed).
- NEW `RS/model-gateway/src/builtin_skills.rs` - `include_str!` the four SKILL.md bodies and register them per org at pull time with `source_path = "builtin:office/<name>"` and trigger keywords (`word`, `docx`, `excel`, `regneark`, `powerpoint`, `pdf` ...); `SkillStore::replace_learned` already preserves them (`skills.rs:92-117` (replace_learned at 102)).
- NEW contract test `RS/model-gateway/tests/builtin_skills_contract.rs` - the embedded SKILL.md hash equals the file the Dockerfile copies (cross-service pattern).
**Flag.** `BUILTIN_OFFICE_SKILLS_ENABLED` (gateway, opt-in, default off). **ZDR.** Static text; no persistence. **Tests.** `builtin_skills_are_registered_for_every_org`, `a_learned_refresh_keeps_builtin_skills`, `skill_budget_contract` still passes. **DoD/Evidence.** "make an Excel budget" matches the xlsx skill, the model reads `references/`, builds, verification passes; capture the injected block and the tool chain. **Deps.** S4, S7; WS07 for the skills cache. **Risks.** Skill budget pressure (8,000 chars): keep bodies short, push detail into `references/`.

### S10. Do-loop parity and Do file delivery [BUILD, deferred, not sized]

Outline only. (1) Do runs deliver no files to the user today (finding 5): a delivery path (artifact event from the run's tool result) belongs with WS24/WS03 and needs a persistence/ZDR decision. (2) `skill_file` in the Do loop beside `reattach_skill` (`runtime_loop/mod.rs:2007-2018`) using the S7 contract; mounts for Do runs ride the run's context (WS02 `RunContext`). (3) Verification already runs in `execute_code_interpreter` (`runtime_loop/mod.rs:891-904`) because it lives in `code_interpreter::run`, so Do tool results carry the report from S2 for free.

## 5. Interfaces

**PROVIDES**

| To | Contract |
|---|---|
| F-t3-038 deck pipeline, F-t3-058 skill lifecycle (outside this plan set) | `mp-contracts::office_verification`, the `pptx` bundle, `skill_bundles` tables, `skill_file` contract |
| WS08 | `ro_binds` plumbing is shared (either lands first); reserved names `attachments` (WS08) and `skills` (WS19) |
| WS05 | `skill_file` declared class `read`; `code_interpreter` class unchanged (`sandbox_compute`) |
| WS06 | consumes the audit emitter; provides audit events `skill_bundle.uploaded/deleted` |
| WS07 | bundle revisions: bundle upload/replace/delete must be recorded as a skill revision action; bundles key on `agent_skills.id` without altering that table |
| WS12 | FE strings use `i18n.tr` pairs; SSE golden gains the optional `check`/`preview` keys; no new event name |
| WS09 | `ArtifactOfficePreview` is not virtualised content; frame is lazy |
| Space session (a) | the registry API in `artifact-renderers.ts` |

**CONSUMES**

| From | Contract |
|---|---|
| WS01 | ZDR/live wave committed first; image rebuild + deploy proof procedure; Debian security rebuild cadence |
| WS08 | S5 `RunAttachment`, `execute_sandboxed_in_dir` ro-binds, `attachments/` reservation; "edit an attached xlsx" flows need WS08 S5 (S0-S9 do not) |
| WS05 | class vocabulary for `skill_file` |
| WS06 | audit emitter |
| WS07 | `skills.rs` cache buckets and revision recording |
| Integrator | proto numbers: `ExecuteStepRequest.skill_mounts = 13`, migration names |

**Conflict hot spots and edit order**

| File | Others | Rule |
|---|---|---|
| `model-gateway/src/tool_loop.rs`, `sse_events.rs`, `sse.rs` | WS01, WS02, WS05, WS08, WS25 | WS01 commits the live wave first; S2/S3/S7 add new modules (`skill_bundle.rs`, `builtin_skills.rs`) and thin diffs at the arm, events fn and allowlists |
| `execution-core/src/executor.rs`, `code_interpreter.rs`, `Dockerfile` | WS08, WS22, WS24 | first to merge adds `ro_binds`; the Dockerfile gets one new RUN block per stream, never edits the Python apt list |
| `execution.proto` | WS02, WS03, WS08, WS11, WS24 | add-only; integrator assigns `13`; `buf breaking` in CI |
| `FE ChatArtifactPanel.tsx`, `chat-artifacts.ts`, `chat-client.ts`, `chat-normalizers.ts` | WS09, WS12, WS14, WS17 | S1 first (pure refactor); WS09 transcript edits precede S3/S5 on the controller |
| `mp-contracts/src/lib.rs` | many | one-line `pub mod` additions after the live wave |

## 6. Cross-session interfaces

- **Space session (a):** overlap-space #13 specifies a viewer registry and lazy DOCX/XLSX preview twice (S567 there, M562 here). Owner of the registry interface: this stream (S1, one file, Frontend Plane); the Space session registers its shared-room and client-side xlsx grid renderers into it and must not fork `artifactRenderKind`. Assumed from (a): Space-bound durable uploads stay imports-core/Data Plane; the persistent Space workspace root never uses the names `skills` or `attachments`; a Space `scope` on `agent_skills` (S1267) does not change bundle keys (bundles follow `skill_id`).
- **Quarry/browser session (b):** no shared code. Naming: Quarry's NATS `artifact` event (overlap-quarry #5) is a different channel from the gateway `artifact` SSE; screenshots keep the `image` renderer, so S1's built-ins must keep it. Assumed from (b): none.

## 7. Regression guards (audit section 8)

| Strength | Touch | Guard |
|---|---|---|
| Signed fail-closed capability gate | `skill_file` is a read tool; scripts run only in `cap.command.sandbox`; no new capability, no bypass | WS05 class row; `permission/mod.rs:415` test `code_interpreter_runs_ungated_while_shell_stays_gated` unchanged |
| Two loops separate | gateway `skill_file`, execution-core mount; shared contracts only | `cross_service_loop_contract.rs` extended; no import between services |
| ZDR by construction | verify/mount/`skill_file` add no store | golden rows in `zdr_feature_table.rs`; `grpc.rs:881` refusal remains |
| Honest outcomes | report never says "verified in Office"; render unavailable is a warning, never silent pass | tests `render_unavailable_is_warning_not_pass`, FE chip wording tests |
| Typed ChatEvent taxonomy + byte-verbatim relay | additive optional keys, no new event | WS12 taxonomy gate + Rust SSE golden |
| Credential isolation | sandbox env allowlist untouched; verify uses `child_env_for` | `code_interpreter.rs` env tests; `verify-sandbox-isolation.sh` |
| Kernel-enforced sandbox | LibreOffice runs inside the same argv; no `/proc`, no widened binds (spike fails closed) | S0 script, `render_argv_has_unshare_net_and_no_extra_binds` |
| Office exports in the browser | untouched | `chat-export/index.test.ts` |

## 8. Effort and risks

**Total:** about 58-70 engineer-days serial (S0 3, S1 2.5, S2 7, S3 5.5, S4 10, S5 5.5, S6 9, S7 5.5, S8 9, S9 5.5; S10 unsized). Three independent tracks: (a) S0 -> S2 -> S3 -> S4 -> S5, (b) S6 -> S7 -> S8, (c) S1. Two engineers: roughly 7-8 calendar weeks. **Critical path:** S0 (3) -> S2 (7) -> S4 (10) -> S5 (5.5) -> S9 (5.5) = about 31 days. Card says XL and "L + M + M"; this matches (a) ~ L+L, (b) ~ L+M+L, (c) ~ S+M.

| # | Risk | Mitigation |
|---|---|---|
| 1 | LibreOffice does not run under this bwrap profile (no `/proc`, uid 10001, read-only root) | S0 spike first; fail closed to structural-only; never widen binds; D5(b) sidecar as fallback |
| 2 | Image size, cold start and memory (LO plus fonts) hurt every execution-core start and OOM under concurrency | separate RUN layer, pre-warmed profile, semaphore (default 2), 60 s verify timeout, measured gate in S0, `structural` rollback |
| 3 | Parser CVEs in LibreOffice/poppler on model-authored files | separate verify workspace, no network, timeout, macros and link updates off, report text untrusted and scrubbed, rebuild cadence owned by WS01 |
| 4 | False `failed` blocks good files; users read "Kontrollert" as an Office-fidelity guarantee | advisory default (D2), corpus-measured rate before enforce, explicit wording and tests |
| 5 | Tenant bundle scripts and SKILL.md as a supply-chain/injection vector | admin-only zip upload, audit, caps, no import (D6), sandbox-only execution, read-only mount, digest-verified blobs |
| 6 | Merge pressure on `tool_loop.rs`, `sse*.rs`, `executor.rs`, `ChatArtifactPanel.tsx`, `execution.proto` | new modules, thin diffs, edit-order rules in section 5, S1 first |
