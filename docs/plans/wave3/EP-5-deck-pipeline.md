# EP-5 - Deck pipeline with brand templates (Wave 3 design)

Abbreviations: `MP` = `apps/Model Plane`; `MPR` = `MP/rust`; `MG` = `MPR/services/model-gateway/src`; `EC` = `MPR/services/execution-core/src`; `CC` = `MP/go/services/capability-core`; `MPC` = `MPR/crates/mp-contracts/src`; `FE` = `apps/Frontend Plane/verevonv3`; `FES` = `FE/src`; `BFF` = `FE/apps/gateway/src`. Every `path:line` was opened in worktree `epic-shaw-4d613b` at HEAD `70e67c8d7` (branch `model-plane`) unless marked otherwise. `[INF]` = inference, `[verify live]` = only a running stack can show it. Sibling designs: `docs/plans/wave3/EP-2-research-and-citations.md` (research-to-deck source), `00-integration-round-a.md` (numbers reserved so far). Plan source: `docs/plans/model-plane-chat-build-plan-2026-10-14.md` section 3.3.2 "EP-5", WS19 (section 3.1).

## 1. Scope, items, owner

**Item in scope:** F-t3-038 "Deck pipeline with brand templates: tool-owned slide manifest, per-slide validation, present mode, research-to-deck, and an org template library" (Recommended, audit status Partial, audit effort L; "XL with a sandbox renderer"). Audit dependencies: F-t3-014 (WS19), F-t3-027 (EP-2).

**Owner.**
- Model Plane: the manifest contract (`mp-contracts`), the `deck` tool in model-gateway's Ask loop, the build helper that runs inside the existing `code_interpreter` sandbox, manifest persistence in message metadata (the thread's artifact store), the template bundle flag in capability-core.
- Frontend Plane: slide strip, present mode, template picker, action contracts.
- Data Plane: no write in W3. Research-to-deck reads only through EP-2 (which reads Data Plane through its read API). Template reference files are org configuration and live in capability-core skill bundles (WS19 D3), not in Data Plane (decision D4).
- Control Plane: no change in W3. Image pricing and entitlements belong to F-t3-051 (OUT).

| Slice | Tag | In |
|---|---|---|
| S1 | [BUILD] | Deck build helper and skill text: a fixed python-pptx builder that reads a slide manifest and reports per-slide fit; usable through `code_interpreter` today |
| S2 | [BUILD] | Manifest contract in `mp-contracts` and the `deck` tool in the Ask loop (typed operations, server-authored build, per-slide state) |
| S3 | [BUILD] | Manifest persistence and versions: `deck` artifact kind kept in message metadata, rehydrated on reload |
| S4 | [BUILD] | Slide strip in the artifact panel, drawn from the manifest (FE) |
| S5 | [BUILD] | Present mode with a present gate (FE) |
| S6 | [BUILD] | Brand templates as skill bundles: `deck-theme.json` contract, bundle flag, theme applied by the builder |
| S7 | [BUILD] | Template picker and template list route |
| S8 | [BUILD] | Research-to-deck: build a deck from a document artifact, carrying sources into notes |
| S9 | [BUILD, optional] | Per-slide render thumbnails for decks (render layer page cap) |
| S10 | [FINISH] | Flip switches on the proof org, ledger entries, evidence |

**Explicitly OUT:**
- F-t3-051 images as a governed agent tool (`generate_image` in the tool loops, edit endpoint, C2PA, second provider): EP-5 W4 member. Not designed here. The deck builder does not call any image model; pictures in a deck come only from `code_interpreter` (matplotlib/Pillow) or from a template bundle file.
- Decks in Do runs: Do runs deliver no files today (WS19-S10 is `optional-deferred` in `.plan-work/execution/queue.json`). The `deck` tool is an Ask-loop tool only.
- A new renderer or a long-lived render service. The audit's "XL with a sandbox renderer" is avoided: S9 only reuses the WS19-S4 LibreOffice layer.
- The artifact canvas, revision store and user editing in the Output panel (EP-3, F-t3-031). S3 uses today's thread-owned artifact version counter; EP-3 may later host it.
- Deep research itself, claim ledger and provenance (EP-2 S4-S6). S8 consumes them when present.
- Template thumbnails rendered on upload, a template marketplace, cross-org templates, Space-shared templates.
- Collaborative deck editing and sharing a deck by link (EP-12, DEC-40 share model).
- W5 neighbours: F-t3-080 library of generated files, F-t3-090 time-based media, F-t3-091 dashboards.

## 2. Verified current state

**How decks are made today.**
- The `code_interpreter` tool text advertises python-pptx and tells the model to add charts as PNG (`MG/tool_loop.rs:5399`). The tool takes `language`, `code` and `files_in` (flat name + `content_b64`) (`MG/tool_loop.rs:5400`; dispatch `MG/tool_loop.rs:3310-3368`; `MG/tools.rs:165-215`, `files_in` copied into the tool input at `:210-211`). So a server-authored call can stage a manifest file without any WS08 staging.
- python-pptx 1.0.2 is a pinned pip wheel in its own `RUN` (`MPR/services/execution-core/Dockerfile:59-87`, pip line 82). The LibreOffice + poppler + fonts layer (WS19-S4) and an empty `/skills` mount point are in the image (`Dockerfile:88-110`); first-party Office skills are copied to `/opt/verevon/skills/office/` (`Dockerfile:121-123`). The audit's "no renderer in the image" is stale: the render layer exists in code, but has not been run live (register D2-028, D2-030, D2-239, D2-240 open).
- The first-party pptx skill exists (`MP/skills/office/pptx/SKILL.md`, WS19-S9): 16:9, one message per slide, at most five bullets, 18 pt minimum, reopen and print titles, obey the `office_checks` line. It has no `scripts/` or `templates/` folder (only `SKILL.md` in each of the four skill dirs). It is overlaid only when `BUILTIN_OFFICE_SKILLS_ENABLED` is `true` (default off; ledger `MP/docs/decisions/ledger.md:5414`).
- A second deck path is the browser export of one answer: `FES/features/chat/lib/chat-export/pptx.ts:1-80` builds PresentationML client-side (16:9, a slide per level-1/2 heading, 8 bullets / 700 chars per slide). It is untouched by this epic.
- No manifest, no slide state, no slide strip, no present mode, no template kind exists anywhere (grep of `MG`, `FES` for `deck manifest`, `DeckTheme`: only a recommendation line in `FE/apps/verevon-web/plans/system-audits/CHAT_GAP_CLOSURE_PLAN_2026-10-01.md:506` "Neutral deck theme with a DeckTheme seam"). Ledger DEC-37 entry says the OD18 deck theme item is "not decided here" (`ledger.md:5342-5343`).

**What WS19 built that this epic reuses (code wins over the audit).**
- Structural verifier: `check.py` handles pptx (`EC/office_verify/check.py:38` main part, `:168-172` slide count into `stats.slides`, warning on zero slides). It reports per file, not per slide. Render path: soffice to PDF, `pdfinfo` page count, fonts, up to `PREVIEW_PAGES = 3` thumbnails at 60 dpi, blank-page warning (`check.py:251-256,445-497,531-555`). The report contract is `mp_contracts::office_verification::OfficeVerification{name, kind, level, checks, stats}` (`MPC/office_verification.rs:110-125`; caps `MAX_FILES=4`, `MAX_CHECKS=40`, `MAX_STATS=16` at `:26-35`).
- Previews: `office_verify::Verification.previews` (`EC/office_verify.rs:81-98`) are wired into `CodeResult` (`EC/code_interpreter.rs:197-204,733-735`), so register D2-238 looks closed in code [verify at slice start]. The gateway turns them into `AttachmentPreview{mime, url, pages, fonts_substituted, thumbs}` with a 2 MiB PDF cap (`MG/tool_loop.rs:607-640`) on the `attachment` event (`MG/sse_events.rs:288-299,755-780`). The tonic client decode limit is still the 4 MiB default (register D2-949/D2-951 open).
- Enforce switch `CODE_INTERPRETER_VERIFY_ENFORCE` (hold a failed file once per name per turn) (`MG/tool_loop.rs:552-591`).
- FE: renderer registry `extraArtifactRenderers` (`FES/features/chat/components/artifact-renderers.ts:24-76`), the Office previewer registered at module load (`ChatArtifactPanel.tsx:39-40,80`; `ArtifactOfficePreview.tsx:24-85`, PDF frame with `#page=N`, thumbnail strip, font notice). `GeneratedFile.check?` and `preview?` (`FES/features/chat/components/chat-types.ts:150-188`); previews are documented as never kept with the thread.
- Skill bundles: capability-core migration `0021_skill_bundles` (one bundle per `(org_id, skill_id)`, <= 256 files, <= 8 MiB total, <= 1 MiB per file, blobs per org) (`CC/migrations/0021_skill_bundles.up.sql:16-58`); RPCs `ListSkillBundles`, `GetSkillBundleManifest`, `GetSkillBlob` (`MP/proto/model_plane/v1/capabilities.proto:39-45`); `SkillBundleSummary` fields 1-6 used, next free **7** (`capabilities.proto:330-338`); `SkillBundleFile.binary = 5` (`:350-358`). The read-only `/skills/<name>` mount is derived only from the turn's matched skills, at most `MAX_SKILL_MOUNTS`, never from a model-supplied name (`MG/tool_loop.rs:3150-3240`), and is off under ZDR or while `SKILL_BUNDLE_MOUNT_ENABLED` is off (`:3100,3188-3200`). `ExecuteStepRequest.skill_mounts = 14` (`execution.proto:545`; the plan said 13, the tree says 14).

**Artifacts and persistence.**
- `ArtifactKind` is a closed enum: code, document, html, spreadsheet, file, image (`MG/artifacts.rs:30-86`); only code/document/html are text-authored. `MAX_TEXT_ARTIFACT_CHARS = 200_000` (`:92`). The thread version store (`ArtifactVersionStore`, `fork_thread`, `seed_persisted`, `lock_commit`) lives in `MG/artifacts.rs:150-200`.
- What survives a reload server-side is `RecordedArtifact` for document/code/html only, persisted in message metadata under `artifacts` (`MG/sse_events.rs:1264-1300`; writer `MG/session_flow.rs:1939-1961`; rehydration `MG/sse.rs:5305-5307,6711-6747`). Generated binaries are **not** persisted by the server: they are `data:` URIs on the `attachment` event, and the SPA keeps them in its local cache (`FES/features/chat/components/chat-normalizers.ts:1034` merges `files` from server and cache). The plan's "Model Plane has no durable binary artifact store" holds.
- Tools in the Ask loop: `conversation_tool_allowed` lists `create_artifact | read_artifact | update_artifact | count_words | code_interpreter | reattach_context | skill_file` (`MG/tool_loop.rs:5282-5284`); `inline_tool_allowed` refuses side-effecting names (`:896-902`).

**ZDR.** A ZDR turn is refused for `generate_image`, `skill_ids`, `attachments`, `deep_research` and more (`MG/zdr_features.rs:98-140,315-365`); the ZDR path (`zdr_direct_stream`) has no tool loop, and `ExecuteStep` refuses ZDR callers (WS19 clause). Feature keys available for reuse (from `MP/api/contracts/zdr-refusals.json`): `skills`, `tool_execution`, `command_execution`, `image_generation` and 30 others. Enforcement rows in `MP/api/contracts/zdr-posture-matrix.json` use an `e_` prefix (e.g. `e_zdr_token_never_reaches_subscription`, line 541).

**Prerequisite status (queue `.plan-work/execution/queue.json`, register `docs/plans/model-plane-depth-register.md`).**

| WS slice | Queue status | Open register rows that matter here | Can EP-5 start anyway? |
|---|---|---|---|
| WS19-S0 LibreOffice spike | partial | D2-028..D2-035 (spike never run; D5 in-image vs sidecar undecided) | Yes for S1-S8 (structural only). S9 waits. |
| WS19-S1 renderer registry | done | D2-082 reactivity (does not affect module-load registration), D2-085/D2-089 extras beat built-ins | Yes |
| WS19-S2 structural verifier | partial | D2-100/D2-103 zip-bomb memory risk in `check.py` (P1), D2-101..D2-107 live proofs | Build yes; enabling verification on a proof org needs D2-103 fixed |
| WS19-S3 check reaches user | partial | D2-922..D2-927 (enforce policy, false-failed rate) | Yes; EP-5 keeps enforce off |
| WS19-S4 render layer | partial | D2-235 budget, D2-236 Calibri Light false warning, D2-239/240 image build | S1-S8 yes; S9 no |
| WS19-S5 Office previewer | partial | D2-949/D2-951 4 MiB decode limit, D2-952/953 e2e + live | S4/S5 build yes (they fall back without a preview); S9 no |
| WS19-S6 bundle store | partial | D2-379/D2-383 bundle RPCs readable by any org member (P1), D2-384 bundle ZDR posture (owner), D2-380/D2-385/D2-386 live/regen/mount | S6 code yes; S6 enable **no** until D2-383 and D2-384 close |
| WS19-S7 `skill_file` | partial | D2-556..D2-561 | Not required |
| WS19-S8 `/skills` mount | partial | D2-639 live, D2-640 model not told the mount path, D2-641 org-wide read token (owner) | S6 code yes; S6 enable needs D2-639/D2-640/D2-641 |
| WS19-S9 builtin Office skills | partial | D2-595 substring keyword matching ("deck", "slide" over-trigger) | S1 yes; S1 enable needs D2-595 tuned |
| WS19-S10 Do parity | optional-deferred | - | Decks stay Ask-only |
| WS05-S1 class vocabulary | done | - | Yes (S2 seed row) |
| WS12-S7.code SSE golden | done (external half blocked) | - | Yes |
| EP-2 S4 provenance, S6 ledger | design only, not built | EP-2 OD-1, OD-6 | S8 basic half yes; provenance half waits |

**Audit staleness summary:** (1) "no renderer in the image" is stale (WS19-S4 layer in code, unproven live); (2) "workspace deleted per call" is still true, which is why the manifest, not the pptx, must be the source of truth; (3) the cited line `tool_loop.rs:4320` has moved to `:5399`; (4) "reference files need Data Plane storage" is superseded by WS19's capability-core bundle store (WS19 D3, plan section 2.6 row 25); (5) the audit does not mention that generated binaries are not server-persisted, which drives D1.

## 3. Decisions and open questions

| # | Decision | Options | Recommendation | Gates | Proceed without? |
|---|---|---|---|---|---|
| D1 | Where the manifest lives | (a) a text artifact of new kind `deck` (manifest JSON) in the thread's artifact store, persisted in message metadata like document/code/html; (b) a new session-core table; (c) a Data Plane document | **(a).** The manifest is small JSON (cap 120,000 chars, under `MAX_TEXT_ARTIFACT_CHARS`), thread-owned, versioned by the existing counter, erased with the message. Overlap-space row 13 already rules "artifact revisions = Model Plane (thread-owned)". No migration. (b) duplicates the artifact store EP-3 will grow. (c) crosses into Data Plane for chat state. The pptx bytes stay unpersisted (rebuilt from the manifest on demand) | S3 | S1, S2, S4, S5 (in-turn only) |
| D2 | Tool-owned or skill-instructed manifest | (a) typed `deck` tool: the model submits operations, the gateway validates and owns the manifest; (b) the model writes manifest JSON itself and runs the helper | **(a)** for the product path (audit: "never hand-edited"), **(b)** stays as S1's fallback while the tool switch is off. With (a) a bad slide is refused before any sandbox call, and per-slide state is computed by the server, not claimed by the model | S2 | S1 |
| D3 | Who runs the build | (a) the gateway issues a server-authored `code_interpreter` ExecuteStep (fixed code calling the builder, manifest via `files_in`); (b) a new execution-core tool `deck.build` | **(a).** No new capability id, no new execution-core tool, same sandbox argv, same verifier, same `code_interpreter` class (`sandbox_compute`, `CC/migrations/0016...:137`). The model never supplies the code of a build call. Two loops stay separate (CLAUDE.md): the gateway only calls the executor it already calls | S2 | S1 |
| D4 | Where brand templates live | (a) WS19 skill bundles in capability-core (`templates/deck-theme.json` + optional `templates/master.pptx`); (b) Data Plane documents (audit text); (c) a new template table | **(a).** A template is org configuration, admin-uploaded, digest-verified, mounted read-only; WS19 D3 already put bundle bytes in capability-core. (b) would make Data Plane hold executable-adjacent config and needs a new read path into the sandbox. Record as a ledger entry that reverses the audit wording | S6, S7 | S1-S5, S8 |
| D5 | How a template is chosen | (a) explicit: picker sends `deck_template` (a skill id); the gateway checks it is an org bundle flagged as a deck template and adds it to the turn's mount set; (b) keyword matching like other skills | **(a).** Keyword matching already over-triggers on "deck"/"slide" (D2-595). The model cannot name a bundle: the template id comes from the request, is validated server-side, and the model only sees "template applied: <name>" | S6, S7 | S1-S5, S8 |
| D6 | Waived turns (DEC-32) | persist manifest like Ask artifacts / do not persist | **Do not persist.** A `deck` artifact kind is a new durable kind; DEC-32 says a waived run counts as ZDR for every new store. Streamed only, rebuilt from nothing on reload (the strip shows "not kept for this connection") | S3 enablement | Yes (default refuses) |
| D7 | Present gate | block on any warning / block only on `error` slides / never block | **Block only on `error`** (a slide that failed validation or failed to build); warnings show a chip. Present mode is a viewer, not a quality guarantee, and wording never says "verified in PowerPoint" | S5 | Yes |
| D8 | Bundle read authority and bundle ZDR posture (register D2-379, D2-383, D2-384, D2-641) | owner rulings | Add an explicit method rule in `CC/internal/authz/authz.go` (service principals with capability read scope only, user principals refused for blob reads) before any template is mounted; bundles are org config, not conversation content, so no ZDR refusal at rest, but a ZDR **turn** never mounts or applies a template (already true: mounts are off under ZDR). Needs the owner's acknowledgement | S6 enable, S7 enable | S6/S7 code yes |
| D9 | Template file size | keep 1 MiB per file / raise for `master.pptx` | **Keep 1 MiB.** A master with compressed logos fits; raising changes the 0021 CHECK constraint and the WS19 caps. Revisit only on a real template that does not fit [verify live with a real brand master] | S6 | Yes |
| D10 | Fidelity claim | structural fit only / render-based | **Structural fit always** (shape bounds, text budget per layout, notes present when a number is quoted), **render-based only when** `CODE_INTERPRETER_OFFICE_VERIFY=render` is on and WS19-S0's spike has decided D5. No new renderer (keeps 038 at L, not XL) | S1, S9 | Yes |
| D11 | Slide caps | defaults | 40 slides per deck, 6 bullets per slide, 90 chars per bullet, 120 chars per title, 2,000 chars of notes per slide, 4 images per slide, manifest <= 120,000 chars. Server-declared constants in `mp-contracts`, changed by a code change only | S1, S2 | Yes |

Ledger entries this epic adds (newest first, `MP/docs/decisions/ledger.md`, CRLF): "A deck's source of truth is a thread-owned manifest artifact; the pptx is rebuilt, never stored" (D1, D3); "Brand templates are skill bundles, not Data Plane documents" (D4, reverses the audit wording); "Templates are chosen explicitly, never by keyword" (D5). Nothing in the ledger is reversed. DEC-37's open "deck theme" item is closed by the D4/S6 entry (neutral default theme, `DeckTheme` contract).

No plan `[RECONCILE]` is assigned to EP-5. The nearest one, EP-3's two versioned-document stores, is not touched: S3 stays in the Model Plane artifact store that overlap row 13 already assigns to Model Plane.

## 4. Slices

Commands used below: Rust `cd "apps/Model Plane/rust" && cargo test -q -p <crate> --lib <filter>` (two cargo targets fail pre-existing on Windows, so use `--lib` there; Linux-gated sandbox tests run in a `rust:1` container); `cargo fmt --all -- --check`; SSE taxonomy `python3 "apps/Model Plane/scripts/gen-sse-event-taxonomy.py" --check`; ZDR contract `bash scripts/tests/zdr-contract-sync.test.sh`; Go `cd "apps/Model Plane/go/services/capability-core" && go test ./... && go vet ./...`; proto `buf lint` / `buf breaking --against proto/baseline.binpb` in `MP/proto` (lint has pre-existing debt; compare against the ratchet); FE from `FE`: `pnpm exec vitest run <files>`, `pnpm lint`, typecheck with the app config (`tsc -p tsconfig.app.json`, failing set diffed against HEAD per memory); BFF `cargo test --manifest-path "apps/Frontend Plane/verevonv3/apps/gateway/Cargo.toml" --all-targets`. Live proofs need rebuilt images and `.plan-work/execution/tools/recreate-with-live-env.sh`, and a second FE dev server (the `:5173` container serves the main checkout).

### S1 - Deck build helper and skill text [BUILD]
**Goal.** A deck the model builds is laid out by one fixed builder and comes back with a per-slide fit report, so slide quality is checked today, before any new tool exists. Cheapest user-visible value: better decks and honest per-slide findings in the existing chat.
**Files.**
- `MP/skills/office/pptx/scripts/build_deck.py` - NEW. Pure python-pptx 1.0.2, stdlib + python-pptx only. `python3 /opt/verevon/skills/office/pptx/scripts/build_deck.py manifest.json out.pptx` reads a manifest v1 (`{version:1, title, locale, theme?, slides:[{id, layout: title|title_bullets|title_only|section|two_column|image, title, bullets[], left[], right[], image?, notes?}]}`), validates it against the D11 caps, builds 16:9 slides from the default template layouts, and prints one JSON line `{deck:{slides, file}, slides:[{id, index, state: ok|warn|error, findings:[{id, message}]}]}`. Fit heuristics: every shape inside the slide box; estimated text height per placeholder from font size, line count and box width (chars-per-line table per layout); bullets over cap; title over cap; empty slide; number in a bullet with no notes ("source missing"). Image paths only relative inside the working directory (no `..`, no absolute, no symlink follow).
- `MP/skills/office/pptx/SKILL.md` - add a short "Manifest path" section (inside the 8,000-char skill budget, `tests/skill_budget_contract.rs`): write `manifest.json`, run the builder, read the slide report, fix slides with `error`, re-run. Keep python-pptx free-form as fallback for layouts the builder lacks.
- `MPR/services/execution-core/Dockerfile` - none: `COPY skills/office/ /opt/verevon/skills/office/` (`:121-123`) already copies `scripts/`. Mode 0555 on the script.
- `MPR/services/model-gateway/tests/builtin_skills_contract.rs` - extend: the embedded `SKILL.md` still matches; new assertion that `build_deck.py` exists beside it.
- `MPR/services/execution-core/tests/fixtures/deck/` - NEW manifests: `good_5_slides.json`, `overflow_bullets.json`, `bad_image_path.json`, `empty_deck.json`.
**Contract/proto/migration.** None. **Flag.** None new; reached only through `BUILTIN_OFFICE_SKILLS_ENABLED` (default off). Rollback: revert the files.
**Tests.** Python unit tests run in the `rust:1`-based image or any python3 with python-pptx: `MP/skills/office/pptx/scripts/test_build_deck.py` (`a_good_manifest_builds_and_reads_back`, `overflowing_bullets_mark_the_slide_warn`, `a_title_over_cap_is_error`, `an_absolute_image_path_is_refused`, `a_dotdot_image_path_is_refused`, `an_unknown_layout_is_error_not_crash`, `report_is_one_json_line`); run `python3 -m pytest "apps/Model Plane/skills/office/pptx/scripts" -q`. Rust: `cargo test -q -p model-gateway --test builtin_skills_contract --test skill_budget_contract`. Add the pytest call to `model-plane-rust-ci.yml` only if a python job exists there [verify at slice start]; otherwise document it in `MP/scripts/tests/README`.
**Live proof.** Gateway + execution-core rebuilt, `BUILTIN_OFFICE_SKILLS_ENABLED=true`, `CODE_INTERPRETER_OFFICE_VERIFY=structural` on the proof org: prompt "Lag en presentasjon på 6 lysbilder om ..." shows a `code_interpreter` call running the builder, a slide report with at least one `warn` fixed in a second call, the pptx card with "Kontrollert". Evidence: SSE transcript (curl -N via the BFF), execution-core log counts, the download opened in PowerPoint and LibreOffice (screenshots) [verify live].
**Effort.** M, 4-5 d. **Deps.** WS19-S9 (built, partial); D2-595 keyword tuning before the switch is turned on for real orgs. **Risks.** fit heuristics are estimates (false `warn`); the model may ignore the skill text (that is why S2 exists).

### S2 - Manifest contract and the `deck` tool [BUILD]
**Goal.** The model edits a deck only through typed operations; the gateway owns the manifest, builds the pptx through a server-authored sandbox call, and attaches per-slide state.
**Files.**
- `MPC/deck_manifest.rs` - NEW (`pub mod deck_manifest;` in `MPC/lib.rs`, after the last module line). `DeckManifest{version:u32, deck_id, title, locale, template:Option<DeckTemplateRef>, slides:Vec<Slide>, revision:u32}`, `Slide{id, layout:SlideLayout, title, bullets, left, right, image:Option<String>, notes:Option<String>, state:SlideState, findings:Vec<SlideFinding>}`, `SlideState{Draft, Ok, Warn, Error}`, caps (D11), `validate()`, `apply(op) -> Result<(), DeckOpError>`, `from_untrusted_json`, `canonical_json()` (stable key order, used for digests). Ops: `create{title, locale, slides}`, `set_slide{id, ...}`, `insert_slide{after_id, slide}`, `delete_slide{id}`, `move_slide{id, after_id}`, `build`. Slide ids are server-minted (`s1`, `s2`, ...), never reused within a deck.
- `MG/deck_tool.rs` - NEW. The `deck` dispatch: parse op, apply to the turn's manifest copy (held in the forked `ArtifactVersionStore`, kind `deck`), on `build` (or on any op when `auto_build` is set) call `crate::tools::handle_code_interpreter` with fixed code `python3 /opt/verevon/skills/office/pptx/scripts/build_deck.py manifest.json <slug>.pptx` and `files_in=[{name:"manifest.json", content_b64}]`; parse the builder's report from stdout (untrusted: through the same JSON scan as `extract_json_object`, capped findings, scrubbed), write per-slide `state`/`findings` back into the manifest, return a compact tool result (slide states, findings, file name). The WS19 verifier runs on the pptx as it does for any `code_interpreter` output; its `check` rides the attachment as today.
- `MG/tool_loop.rs` - `"deck"` arm beside `"code_interpreter"` (`:3310`); add `deck` to `conversation_tool_allowed` (`:5282-5284`) and to the tool list only when the switch is on; tool definition (operations schema, caps in the description); `code_interpreter_events` reuse for the build outcome so the attachment, check and preview events are unchanged; emit an `artifact` event of kind `deck` with the manifest JSON and version after each successful op. Add `deck` to `prompt_context::CONDITIONAL_TOOLS` (same reason as D2-562).
- `MG/artifacts.rs` - `ArtifactKind::Deck` (`"deck"`), `parse` accepts `"deck"` only from the tool path, `is_text_authored` stays false for `Deck` so `create_artifact` cannot forge one.
- `CC/migrations/0033_deck_tool_class.up.sql` / `.down.sql` - capability-core head is `0030_tool_policy_expiry`; seed row `('global','cap.command.sandbox','deck','sandbox_compute','none',NULL,'seed')` in the 0016/0029 format, re-asserted only while `source='seed'`. Integrator assigns the number (round A reserved no capability-core number).
- `MPR/services/execution-core/tests/tool_class_registry_parity.rs` - add `deck` [verify at slice start whether parity covers gateway tools; `skill_file` had the same gap, D2-561].
**Contract.** No proto change. SSE: no new event name; the `artifact` payload carries `kind:"deck"` (a new value of an existing string key). Regenerate the SSE golden if it pins kind values (`MP/api/contracts/sse-event-payloads.json`, then copy to the SPA and re-run `gen-chat-event-types.mjs --check`, and update the pinned digest in `scripts/tests/zdr-contract-sync.test.sh`, the same recipe as D2-956).
**Flag.** `MODEL_GATEWAY_DECK_TOOL` (gateway, strict literal `true`, default off; compose line `${MODEL_GATEWAY_DECK_TOOL:-false}`). Off: the tool is never offered and a hallucinated call is refused by the offered-tools re-check. Rollback: switch off.
**ZDR.** Not offered on a ZDR turn (no tool loop on `zdr_direct_stream`); `ExecuteStep` refuses ZDR anyway. Golden: `e_zdr_turn_deck_tool_not_offered`.
**Tests.** `mp-contracts`: `deck_manifest::tests` - `ops_apply_in_order_and_ids_never_repeat`, `over_cap_slide_is_refused_with_field_path`, `unknown_layout_is_refused`, `move_to_missing_anchor_is_refused`, `canonical_json_is_stable`, `from_untrusted_json_rejects_unknown_state`. model-gateway: `deck_tool::tests` - `build_call_code_is_fixed_and_carries_no_model_text`, `manifest_rides_files_in_as_base64`, `builder_report_is_capped_and_scrubbed`, `slide_state_comes_from_the_report_not_the_model`, `switch_off_never_offers_deck`, `create_artifact_cannot_make_a_deck_kind`, `deck_artifact_event_carries_version`. Commands: `cargo test -q -p mp-contracts --lib deck_manifest`, `cargo test -q -p model-gateway --lib deck_tool`, `cargo test -q -p model-gateway --test zdr_feature_table --test skill_budget_contract`; Go `go test ./...` in capability-core for the seed (migration re-run safety test in the existing pattern).
**Live proof.** Switch on, proof org: "Lag 5 lysbilder om kvartalstallene" -> `deck create` then `build`; edit "gjør lysbilde 3 kortere" -> one `set_slide` and a rebuild; slide 3 state goes `warn` -> `ok`. Evidence: SSE transcript with `tool_call deck`, the `artifact` frame (kind `deck`, version 1 then 2), the attachment with `check`, the execution-core log showing the fixed argv, and no `verevon-code-*` temp dir left [verify live].
**Effort.** L, 8-10 d. Reason: new contract module, a new dispatch arm that composes the existing code_interpreter path, the artifact kind, the class seed and the SSE golden recipe; splitting further ships a tool with no build or a build with no state. **Deps.** S1 (builder), WS19-S2 (verifier, built). **Risks.** `MG/tool_loop.rs` is a hot file (EP-2 S2/S3, EP-10 S8 also add arms: one slice at a time, CRLF kept); two sandbox calls per edit cost latency (mitigation: `build` is explicit, ops without `auto_build` do not build); builder output trusted too much (mitigation: scrubbed, capped, never shown as text the model authored).

### S3 - Manifest persistence and versions [BUILD]
**Goal.** A deck survives a reload and a later turn can continue it; the pptx is rebuilt from the stored manifest on demand.
**Files.**
- `MG/sse_events.rs` - `RecordedArtifact::from_event` (`:1286-1300`) accepts kind `deck` when content is JSON, not `data:`, and under `MAX_PERSISTED_CHARS`; a `deck` artifact is persisted with its version.
- `MG/session_flow.rs` (`turn_evidence_metadata`, `:1939-1961`) - unchanged shape (`artifacts` list); add the waived skip: when the turn is waived (DEC-32), drop `deck` kinds from the persisted list.
- `MG/sse.rs` - rehydration (`:5305-5307`, `:6711-6747`) seeds `deck` artifacts into the version store like documents, so `deck` ops on a later turn start from the stored manifest and bump the version. `read_artifact` on a deck returns the manifest JSON (bounded by `MAX_READ_ARTIFACT_CHARS`).
- `MG/deck_tool.rs` - op `rebuild` (no manifest change) so a reloaded thread can get its pptx again; the tool text tells the model a reload has no file until `rebuild`.
- `FES/features/chat/components/chat-normalizers.ts` - `isChatArtifact` accepts kind `deck` (content JSON, guard parses and checks `version === 1`); unknown versions render as a plain code block, never crash.
**Contract.** No proto, no migration (message metadata JSONB). Same SSE key set.
**Flag.** Same `MODEL_GATEWAY_DECK_TOOL`; persistence has no separate switch (a deck that cannot be kept would be worse than no deck). Rollback: switch off; stored `deck` artifacts render as code blocks.
**ZDR.** ZDR turns write no thread. Waived turns: manifest streamed, not persisted (D6). Erasure: message metadata goes with the message through the existing session-core GDPR path; no new erase statement [verify at slice start that `gdpr.rs` deletes messages with their metadata, as EP-2 S4 also assumes]. Golden: `e_waived_deck_manifest_not_persisted`.
**Tests.** model-gateway: `recorded_artifact_keeps_a_deck_manifest`, `recorded_artifact_drops_a_deck_over_the_cap`, `waived_turn_does_not_persist_deck`, `rehydrated_deck_continues_at_next_version`, `rebuild_uses_the_stored_manifest`. FE: `chat-normalizers.deck-artifact.test.ts` (accepts v1, unknown version falls back, malformed JSON refused). Commands: `cargo test -q -p model-gateway --lib recorded_artifact deck_tool`, `pnpm exec vitest run src/features/chat/components/chat-normalizers.deck-artifact.test.ts`.
**Live proof.** Build a deck, reload the page, ask "legg til et lysbilde om risiko" -> version 3 in session-core message metadata (`psql`, keys and versions only), `rebuild` gives a pptx with 6 slides. Waived-connection turn on a test org shows no `deck` in metadata [verify live; waiver minting is off, so this may only be provable in a unit test].
**Effort.** M, 4-5 d. **Deps.** S2, D1, D6. **Risks.** message metadata grows (bounded at 120,000 chars per deck); the SPA local cache may still hold an old pptx `data:` URI that no longer matches the manifest (card shows the version it was built from).

### S4 - Slide strip from the manifest [BUILD]
**Goal.** The artifact panel shows the deck as a strip of slide cards with per-slide state, drawn from the manifest, with or without a render.
**Files.**
- `FES/features/chat/components/ArtifactDeckView.tsx` - NEW. Registers `deckManifestRenderer` (`id:'deck-manifest'`, `match: artifact.kind === 'deck'`) in `extraArtifactRenderers` at module load, like `officePdfRenderer` (`ChatArtifactPanel.tsx:80`). Strip of 16:9 cards (title, first bullets, layout icon, state chip `OK` / `Advarsel` / `Feil` via `i18n.tr`), findings list per selected slide, notes toggle, a link to the matching pptx file card when the turn has one, and the `ArtifactOfficePreview` below the strip when that file has a `preview`. Large decks virtualised only past 40 cards (cap D11 means never) - no virtualisation.
- `FES/features/chat/components/deck-manifest.ts` - NEW pure parser/guard shared by S4/S5 (mirrors `MPC/deck_manifest.rs` v1; contract test against a JSON fixture exported by the Rust tests: `MPR/crates/mp-contracts/tests/fixtures/deck_manifest_v1.json` copied to `FES/shared/api/contracts/` with a `.sha256`, same pattern as other contracts).
- `FES/features/chat/components/ChatArtifactPanel.tsx` - import side effect only.
- `FES/shared/actions/action-registry.ts` (+ test) - `chat.open_deck_slide` (ownerPlane `frontend`, risk `low`, read-only), added before wiring (repo rule).
- `global.css` (or the chat CSS that holds `.verevon-chat-artifact-office`) - strip layout; no raw colours (WS12-S4 gate).
**Contract.** FE copy of the manifest fixture with sha. **Flag.** None (renders only when a `deck` artifact exists, which needs S2's switch). Rollback: revert.
**ZDR.** Nothing persisted by the FE beyond today's thread cache; ZDR turns have no deck.
**Tests.** `ArtifactDeckView.test.tsx` (strip renders N cards, chips by state, selecting a slide shows findings, preview shown only with a file preview, narrow width 375 px, keyboard focus order), `deck-manifest.test.ts` (fixture sha matches, guard refuses unknown version), `artifact-renderers.test.ts` (deck renderer does not claim document/html). `pnpm exec vitest run src/features/chat/components/ArtifactDeckView.test.tsx src/features/chat/components/deck-manifest.test.ts src/features/chat/components/artifact-renderers.test.ts`, `pnpm lint`.
**Live proof.** Second dev server against the rebuilt gateway: screenshots at 1280 and 375 px of a 6-slide deck with one `warn`, then after the fix [verify live].
**Effort.** M, 4-5 d. **Deps.** S2 (artifact kind); D2-085/D2-089 owner ruling on extras beating built-ins does not bite (kind `deck` has no built-in). **Risks.** strip is an approximation of the real slide; label it "Utkast fra manifest" when no render exists so it is not read as the final look.

### S5 - Present mode with a present gate [BUILD]
**Goal.** One click presents the deck full screen; slides with `error` block presenting until fixed.
**Files.**
- `FES/features/chat/components/DeckPresentMode.tsx` - NEW. Full-screen overlay (`role="dialog"`, `aria-modal`, focus trapped, Esc closes, arrows / PageUp / PageDown / Home / End, slide counter, speaker notes on `N`). Source: the preview PDF pages (`#page=N` in the sandboxed frame, as `ArtifactOfficePreview`) when the file has a preview and its `pages` equals the slide count; otherwise manifest HTML cards at 16:9. Respects `prefers-reduced-motion` (no transitions).
- `ArtifactDeckView.tsx` - "Presenter" button; disabled with a reason when any slide is `error` or a slide is `draft` (never built) (D7). Warnings show "N lysbilder har advarsler" and still allow presenting.
- `FES/shared/actions/action-registry.ts` - `chat.present_deck` (risk `low`).
**Contract/flag.** None. **ZDR.** None (viewer only).
**Tests.** `DeckPresentMode.test.tsx` (keyboard navigation, Esc restores focus to the button, notes toggle, PDF path chosen only when page count matches, falls back to cards otherwise), `ArtifactDeckView.present-gate.test.tsx` (error blocks, warn allows, draft blocks). Same vitest command.
**Live proof.** Present a built deck in Chrome and Firefox; with the render layer on, `#page=N` moves the PDF frame (WS19 D2-953 asks the same question) [verify live].
**Effort.** S-M, 3-4 d. **Deps.** S4. **Risks.** `data:` PDF `#page` support differs by browser (fallback to cards if the frame cannot navigate); full screen API blocked inside some embeds (overlay works without it).

### S6 - Brand templates as skill bundles [BUILD]
**Goal.** An org admin uploads a brand template as a skill bundle; a deck built with it uses the org's colours, fonts, logo and optional master.
**Files.**
- `MPC/deck_theme.rs` - NEW `DeckTheme{version:1, name, colors:{primary, secondary, text, background, accent[<=4]} (hex), fonts:{heading, body}, logo:Option<path under templates/>, master:Option<"templates/master.pptx">, footer_text:Option<String> (<=80)}`, `from_untrusted_json`, font names restricted to the installed families plus metric-compatible names (Carlito, Caladea, Liberation, DejaVu, Noto subset; DEC-37 fonts ruling) - any other name is accepted but reported as "substituted" in findings.
- `CC/internal/skillbundle/validate.go` - when `templates/deck-theme.json` exists: parse and validate the same schema (Go mirror, contract fixture shared with the Rust tests), `master.pptx` must be a zip with `ppt/presentation.xml`, logo must be PNG/JPEG. A bundle with a theme is a deck template.
- `CC/internal/skillbundle/store.go` + `CC/internal/server/server.go` (`ListSkillBundles`) - set the new summary flag.
- `MP/proto/model_plane/v1/capabilities.proto` - `SkillBundleSummary`: add `// True when the bundle carries templates/deck-theme.json (EP-5 S6). Additive.` `bool deck_template = 7;` (fields 1-6 used, verified). Regenerate Go bindings (`buf generate`, Go-only template recipe) and Rust (`mp-contracts` build).
- `CC/migrations/0035_skill_bundles_deck_template.up.sql` (0035 reserved by round B) - **only if** the flag is stored rather than derived. Recommendation: derive at read time from `skill_bundle_files` (`path = 'templates/deck-theme.json'`), so **no migration**.
- `MG/deck_tool.rs` - `create{template_id?}` and request field `deck_template` (S7) resolve to an org bundle with `deck_template=true` via `ListSkillBundles{skill_ids:[id]}`; the template skill is added to the turn's mount set through the existing `skill_mounts_from_bundles` path (`MG/tool_loop.rs:3155`), never from a model-supplied id. The manifest stores `template{skill_id, manifest_sha256, name}`. A template whose digest changed since the manifest was made rebuilds with the new one and says so in the tool result.
- `MP/skills/office/pptx/scripts/build_deck.py` - `--theme /skills/<name>/templates/deck-theme.json`: base presentation from `master.pptx` when present (layouts mapped by name, missing layout -> default layout + finding), colours and fonts on placeholders, logo on title and footer slides, footer text.
- `CC/internal/authz/authz.go` - explicit method rules for the three bundle RPCs (D8, closes D2-383) - this file is outside WS19's set; it is in this slice.
**Contract/proto/migration.** `SkillBundleSummary.deck_template = 7`. No migration (derived). `buf breaking` must stay green (additive).
**Flag.** `SKILL_DECK_TEMPLATES_ENABLED` (capability-core: report the flag; gateway: accept `template_id`), strict `true`, default off. Also requires `SKILL_BUNDLES_ENABLED` and `SKILL_BUNDLE_MOUNT_ENABLED`. Rollback: switch off; decks fall back to the neutral default theme with a finding "template unavailable".
**ZDR.** Bundles are org config, not conversation content (no refusal at rest, pending D8/D2-384 acknowledgement). A ZDR turn never mounts a bundle (`MG/tool_loop.rs:3200`) and never gets the tool. A ZDR request carrying `deck_template` is refused through the existing `skills` feature (`zdr_features.rs` table gains the field mapped to `refused_feature("skills")`; no new key, `zdr-refusals.json` unchanged). Golden: `e_zdr_token_deck_template_refused`.
**Tests.** Go: `validate_test.go` - `a_bundle_with_a_valid_theme_is_a_deck_template`, `a_theme_with_a_bad_colour_is_refused`, `a_master_that_is_not_a_presentation_is_refused`, `a_logo_that_is_not_an_image_is_refused`; `server_test.go` - `list_reports_deck_template`, `a_user_principal_cannot_read_a_blob` (D8); `go test ./... && go vet ./...`. Rust: `deck_theme::tests` (`unknown_font_is_reported_not_refused`, `colour_must_be_hex`), `deck_tool::tests` (`template_id_from_the_model_is_ignored`, `a_template_from_another_org_is_not_found`, `a_non_template_bundle_is_refused_as_template`, `changed_template_digest_is_reported`). Python: `theme_colours_and_fonts_are_applied`, `missing_master_layout_falls_back_with_a_finding`. ZDR: `cargo test -q -p model-gateway --test zdr_feature_table`.
**Live proof.** Upload a sample brand bundle (admin), pick it (S7, or a curl with `deck_template`), build: screenshots show brand colours and logo; bwrap argv shows `--ro-bind ... /skills`; an org-B user cannot list or read the bundle; a user principal gets PermissionDenied on `GetSkillBlob` [verify live].
**Effort.** L, 7-9 d. Reason: Go validator + proto + authz rule + gateway resolution + builder theming span three services; the authz fix is required and cannot be separated without shipping an unsafe read. **Deps.** S2, WS19-S6/S8 (built, partial), D4, D5, D8, D9; enable only after D2-383, D2-384, D2-639, D2-640, D2-641 close. **Risks.** a master's layouts do not match the builder's layout names (fallback + finding); bundle scripts are a supply-chain vector (templates need no scripts: S6 refuses an executable file under `templates/`).

### S7 - Template picker and list route [BUILD]
**Goal.** The user picks one of the org's deck templates before or after building; the choice reaches the server as an id, never as model text.
**Files.**
- `MG/http_routes.rs` - `GET /v1/deck-templates` (user bearer; org from claims; returns `[{skill_id, name, description, colors, fonts}]` read from `deck-theme.json` through capability-core with the gateway's service credential; no blob bytes). Additive block at the end of the router (integration report hot-file rule). Refuses ZDR tokens with the `skills` feature.
- `MG/http_routes.rs` `InvokeRequest` - `#[serde(default, alias="deckTemplate")] deck_template: Option<String>`; the turn validates it before the loop (S6 resolution).
- `BFF/domains/*` - pass-through route `/api/v1/deck-templates` in the chat domain module [verify which domain file owns `/api/v1/chat` style pass-throughs at slice start]; no secret or scoping header forwarded beyond today's pattern.
- `FES/shared/api/chat-client.ts` - `listDeckTemplates()`, `deckTemplate` on the send request.
- `FES/features/chat/components/DeckTemplatePicker.tsx` - NEW. Swatches and font names (no thumbnails in W3), "Standard" first. Shown in `ArtifactDeckView` ("Bytt mal" sends a turn with `deckTemplate` and a short user line "Bruk malen <navn>") and as a composer chip when the prompt mentions a deck [INF: trigger heuristic in FE only; server never infers].
- `FES/shared/actions/action-registry.ts` - `chat.list_deck_templates` (read), `chat.apply_deck_template` (risk `low`).
**Flag.** Same `SKILL_DECK_TEMPLATES_ENABLED`; the route answers 404 while off, the picker hides itself on 404. **ZDR.** Route refused for ZDR tokens; golden `e_zdr_token_deck_template_list_refused`.
**Tests.** MG `deck_templates_route_lists_only_this_org`, `route_404_when_switch_off`, `zdr_token_is_refused`; BFF route test (no forged headers, pattern of existing pass-through tests); FE `DeckTemplatePicker.test.tsx`, `chat-client.test.ts` (send carries `deckTemplate`).
**Live proof.** Picker lists the sample template, switching template rebuilds the deck in the brand; screenshots 1280/375 px [verify live].
**Effort.** M, 4-5 d. **Deps.** S6. **Risks.** `http_routes.rs` and `chat-client.ts` are shared by every Wave 3 design (one slice at a time).

### S8 - Research-to-deck [BUILD]
**Goal.** "Lag en presentasjon av rapporten" turns a document artifact (a deep research report or any document) into a deck whose notes name the sources.
**Files.**
- `MG/deck_tool.rs` - op `create_from_artifact{artifact_id, max_slides}`: reads the document from the turn's artifact store (same thread only, through the existing `read_artifact` bounds), returns its outline (headings, citation numbers) to the model as the tool result, and records `source{artifact_id, version, provenance_digest?}` in the manifest. The model then issues normal `create`/`set_slide` ops; the server does not write slide text itself (keeps the "model authors, server validates" split). Citation markers `[n]` in bullets are kept; the builder moves them to notes as "Kilde n: <title or url>" using the report's citations when present.
- When EP-2 S4 has shipped `provenance` on the report artifact: copy `sources[{number, url|document_id, title}]` and the `ledger_digest` into the manifest; a claim marked `unsupported` in EP-2's ledger cannot become a bullet without "(ikke verifisert)" (validation finding `warn`) [depends on EP-2 S6; until then this rule is inert].
- `MP/skills/office/pptx/SKILL.md` - one line pointing at `create_from_artifact`.
**Contract.** `DeckManifest.source` optional field (manifest v1 stays v1: optional key, fixture updated). **Flag.** `MODEL_GATEWAY_DECK_FROM_ARTIFACT` (default off).
**ZDR.** No new store beyond S3's manifest. Deep research is refused under ZDR (EP-2), and ZDR has no tool loop. Waived: as S3.
**Tests.** `create_from_artifact_reads_only_this_thread`, `missing_artifact_is_a_tool_error`, `citation_numbers_move_to_notes`, `provenance_is_copied_when_present`, `unsupported_claim_bullet_is_warned` (fixture ledger), `switch_off_refuses_the_op`.
**Live proof.** A deep research turn (EP-2 on the proof org) then "lag 8 lysbilder av dette": manifest `source` set, notes carry the source titles, present mode shows them on `N` [verify live].
**Effort.** M, 4-6 d. **Deps.** S2, S3; EP-2 S4 for provenance and S6 for the ledger rule (basic half startable without them). **Risks.** long reports exceed what 8 slides can carry (the model summarises; findings flag overflow); cross-thread sources are OUT (no recall into a deck in W3).

### S9 - Per-slide render thumbnails [BUILD, optional]
**Goal.** With the render layer on, every slide (not only the first 3 pages) gets a thumbnail, so the strip shows the real slide.
**Files.**
- `EC/office_verify/check.py` - `PREVIEW_PAGES` from an env `OFFICE_PREVIEW_PAGES` (default 3, today's value; max 40) passed through the checker argv, applied to pptx only; thumbnails at 48 dpi for decks.
- `EC/office_verify.rs` - `MAX_THUMBS` (`:67`) per kind; total thumbnail bytes capped (3 MiB) so the frame stays under the client decode limit.
- `MG/tool_loop.rs` - `PREVIEW_MAX_THUMBS` (`:607-608`) per kind; `MG/state.rs` client `max_decoding_message_size` raised (closes D2-949/D2-951) - shared edit.
- `ArtifactDeckView.tsx` - card image from `preview.thumbs[index]` when `preview.pages == slides.length`.
**Flag.** `OFFICE_PREVIEW_PAGES` (execution-core, default 3 = no change). **ZDR.** Previews are transient and never persisted (WS19 clause); unreachable under ZDR.
**Tests.** `pptx_preview_pages_follow_the_env_cap`, `thumb_bytes_over_cap_drop_the_thumbs_not_the_pdf` (Linux-gated, `rust:1`), gateway `deck_thumbs_cap`, FE strip uses thumbs only on a page-count match.
**Live proof.** 12-slide deck with `CODE_INTERPRETER_OFFICE_VERIFY=render`: 12 thumbnails, render time inside the 60 s budget [verify live].
**Effort.** M, 4-5 d. **Deps.** WS19-S0 spike result and D5 decision, WS19-S4/S5 live proofs, D2-235 (budget). **Risks.** render time grows with pages; SSE frame size.

### S10 - Flip, document, prove [FINISH]
**Goal.** Turn the switches on for the proof org in order, record decisions, collect evidence.
**Files.** `MP/deploy/docker-compose.yml` (compose lines for every new switch, default `false`); `MP/docs/decisions/ledger.md` (three entries, section 3); `MP/docs/ZDR.md` (rows for the `deck` artifact kind in message metadata and the template route); `MP/docs/capability-ownership-matrix.md` (row: "Deck manifest - model-gateway artifact store; templates - capability-core skill bundles"); plan section 6.6 list; depth register rows `D3-nnn` for anything not proven.
**Order.** `BUILTIN_OFFICE_SKILLS_ENABLED` (after D2-595) -> `MODEL_GATEWAY_DECK_TOOL` -> `MODEL_GATEWAY_DECK_FROM_ARTIFACT` -> `SKILL_BUNDLES_ENABLED` + `SKILL_BUNDLE_MOUNT_ENABLED` + `SKILL_DECK_TEMPLATES_ENABLED` (after D8 and the WS19-S6/S8 live proofs) -> `OFFICE_PREVIEW_PAGES` (after WS19-S0/S4 live).
**Effort.** S, 2-3 d. **Deps.** all above.

## 5. ZDR clause

**New stores and egress.**

| Slice | New store or egress | ZDR turn | Waived turn (DEC-32) |
|---|---|---|---|
| S1 | none (sandbox workspace, Drop-guarded, as today) | `code_interpreter` unreachable (no tool loop; `ExecuteStep` refuses ZDR) | as Ask today |
| S2 | manifest in the turn's in-memory artifact store; one more sandbox call per build | tool never offered | in-memory only until S3 |
| S3 | `deck` artifact kind in message metadata | no thread written | **not persisted** (D6) |
| S4, S5 | none (FE viewer; existing thread cache) | nothing to show | strip from the streamed manifest only |
| S6 | template bundle read by digest into a read-only mount (org config, existing store) | no mount; `deck_template` refused via `skills` | mount allowed (org config, not content); manifest not persisted |
| S7 | template list read (no store) | route refused via `skills` | allowed |
| S8 | manifest `source` key (inside S3's store) | no tool loop; deep research refused | not persisted |
| S9 | larger transient previews (never persisted) | unreachable | transient |

**Refusal behaviour.** No new refusal feature key: every refused path reuses `skills` (template field and route) or is structurally absent (no tool loop under ZDR). `MP/api/contracts/zdr-refusals.json` and its `.sha256` do not change. Under ZDR the `deck` tool is never offered, a request carrying `deck_template` is refused with `feature_not_supported: skills` before any read, and the template route answers the same typed refusal.

**Golden matrix cases** to add to `MP/api/contracts/zdr-posture-matrix.json` (enforcement rows, `e_` prefix) and the inventory tests (`MG/../tests/zdr_posture_matrix.rs`, `zdr_refusal_inventory.rs`, `zdr_feature_table.rs`):
- `e_zdr_turn_deck_tool_not_offered` (S2)
- `e_waived_deck_manifest_not_persisted` (S3, S8)
- `e_zdr_token_deck_template_refused` (S6)
- `e_zdr_token_deck_template_list_refused` (S7)
- `e_zdr_turn_skill_bundle_never_mounted` (S6; asserts the existing `skill_mounts_for_turn` ZDR short-circuit stays)

`MP/docs/ZDR.md` gains a row for the `deck` metadata kind (what, where, erased with the message) and one for template bundles (org config, no conversation content).

## 6. Proof plan

1. Per slice: the unit tests named above; `cargo fmt --all -- --check`; SSE taxonomy and ZDR contract checks; `buf lint`/`buf breaking` for S6; FE vitest, lint, typecheck diffed against HEAD's failing set; Go test/vet for S2 (seed) and S6.
2. Rebuilds for live proof: execution-core (S1 builder baked, S9), model-gateway (S2, S3, S6-S8), capability-core (S2 seed, S6), BFF (S7), FE on a second dev server (S4, S5, S7). After a worktree build use `.plan-work/execution/tools/recreate-with-live-env.sh` so the live env and NATS password are kept (memory).
3. Live flows on a non-ZDR proof org (owner's Anthropic key from the env file outside the repos; stop at the usage line and summarise):
   - F1 skill path (S1): deck via builder, a `warn` slide fixed, pptx opens in PowerPoint and LibreOffice.
   - F2 tool path (S2-S5): create, edit one slide, rebuild, reload, continue, present; `error` slide blocks present.
   - F3 template (S6, S7): admin upload, pick, rebuild in brand; org-B and user-principal reads refused.
   - F4 research-to-deck (S8): EP-2 report to 8 slides with sources in notes.
   - F5 render (S9, optional): 12 thumbnails within budget.
   - F6 ZDR org: no `deck` tool offered, template route and field refused with the typed refusal, zero session-core metadata change (row counts before/after).
4. Evidence: SSE transcripts (curl -N through the BFF), execution-core logs (argv and counts only, never manifest text), `psql` keys-and-versions reads of message metadata, screenshots at 1280 and 375 px, pptx files hashed against the sandbox output, all appended to the depth register as `D3-nnn` rows.
5. Regression guards (audit section 8): the two loops stay separate (the `deck` arm lives in the gateway loop and only calls the existing executor; `cross_service_loop_contract.rs` untouched); signed fail-closed capability gate unchanged (`code_interpreter_runs_ungated_while_shell_stays_gated` still passes; `deck` is classed); honest outcomes (chips never say "verified in PowerPoint", present gate wording tests); kernel-enforced sandbox (same bwrap argv, no new binds beyond WS19-S8's `/skills`); browser export `chat-export/pptx.test.ts` unchanged.

## 7. Risks and what the owner must decide

**Risks.**
1. Fit heuristics are estimates. A slide can pass and still overflow in PowerPoint with a substituted font. Mitigation: findings are advisory, render thumbnails (S9) where available, wording never claims fidelity.
2. Bundle authority: until D2-383 is fixed any org member can read bundle bodies through the RPCs; S6 carries the fix and S6/S7 stay off until it lands.
3. Hot files: `MG/tool_loop.rs`, `MG/http_routes.rs`, `MG/sse.rs`, `FES/shared/api/chat-client.ts`, `action-registry.ts`, `ChatArtifactPanel.tsx`, `capabilities.proto`. Follow the integration report's one-slice-at-a-time rule, CRLF preserved.
4. Latency and sandbox load: each build is one more `code_interpreter` call plus verification (and render when on). Builds are explicit; concurrency is the verifier's existing semaphore.
5. Message metadata size: up to 120,000 chars per deck version in the latest message only [INF: older versions stay in older messages; confirm metadata size limits in session-core at S3 start].
6. LibreOffice under bwrap is still unproven live (WS19-S0); everything before S9 works without it.
7. Template supply chain: templates carry no scripts (S6 refuses executables under `templates/`), are admin-only, digest-verified and read-only.

**Owner decisions (block only the slices named).**
- OD-1 (D1, D6): deck manifest as a thread-owned artifact kind in message metadata, not persisted on waived turns. Blocks S3 (and S8's persistence).
- OD-2 (D4): brand templates are capability-core skill bundles, not Data Plane documents (reverses the audit wording). Blocks S6, S7.
- OD-3 (D5): templates chosen explicitly by id, never by keyword. Blocks S6, S7.
- OD-4 (D8, register D2-383/D2-384/D2-641): bundle RPC read rule and bundle ZDR posture. Blocks S6/S7 enablement (code can be written).
- OD-5 (D7): present gate blocks only on `error`/`draft`. Blocks S5 merge wording only.
- OD-6 (D11, D9): slide caps and the 1 MiB template file cap. Defaults are proposed; S1/S2 build on them.
- WS19 D5 (in-image LibreOffice vs sidecar, register D2-034): blocks S9 only.

Not blocked by any decision: S1, S2, S4 (and S5 with the recommended gate), S8's basic half. Suggested order: S1, S2, S4, S3, S5, S8, S6, S7, S9, S10. S4/S5 (FE) can run in parallel with S3 (gateway) once S2 has fixed the manifest fixture.
