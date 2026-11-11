# Saved recipes (E4.2)

Status on 2026-11-11: the code is built dark. It has not been compiled, tested or run against a stack. Every switch defaults to off. This memo says what the code does in the tree on that date. Where the code and the plan differ, section 7 says so.

Units behind it: step postconditions (Q4-E4.2-1a to 1c); the canonical compile and prefix replay (Q4-E4.2-2a to 2c); the store, the runner and the gateway proxies (Q4-E4.2-3a to 3j); the Verevon gateway and the SPA save (Q4-E4.2-4a, 4b, 4c-c); and this memo with its runbook section (Q4-E4.2-6). The saved list with Run in the SPA (Q4-E4.2-4d-c) was an untracked component in the tree when this was written.

Plan: `docs/plans/quarry-browser-build-plan-2026-10-14.md`, section 3.4, E4.2.

## 1. Decisions

### 1.1 B2(b): capability-core owns the store

The saved-recipe store belongs to capability-core, with its own migration: `apps/Model Plane/go/services/capability-core/migrations/0039_procedures.up.sql`, with a down file beside it. The store is `internal/procedures`. The HTTP surface is `internal/api/procedures.go`. execution-core reads the store over gRPC.

This was taken at the plan's recommendation (item B2(b) of the W4 blocker list). This section is the ledger text for B2(b). The owner's ratification is recorded in the ledger, not here.

Why not EP-11 versioned skills. The session-core skill versions (`agent_skill_versions`, migrations 0108 and 0109) were checked and are not the store:
- their kinds are prompt text (`skill`, `template`, `preset`);
- a matched skill is injected into a turn by trigger keyword or file pattern, so it is not a replay;
- they carry no tags and no structured validation.

The store takes the pattern from them and nothing else: immutable numbered versions, a sha256 digest over the stored bytes, and deletion only under the `mp.version_erasure` setting.

Teach mode is complementary. The Model Plane teach-mode units (W5-081-a to d) produce a prose skill proposal and do not replay. A saved recipe is replayed step by step by the runner. The two do not share a store.

Naming. The stored artifact is a saved recipe. The wire type stays `BrowserProcedure`. Do not confuse either with the prompt procedure `browse/1`, which `QUARRY_BROWSER_AGENT_PROCEDURE` selects.

### 1.2 Scope

Scope is `user` only. A recipe belongs to one person in one organization, and nobody else can read it. The migration's CHECK admits no other value. Sharing is ADR 8 and is not built (section 6).

## 2. The contract

### 2.1 Step postconditions (`expect`)

A step may carry `expect`: a list of 1 to 8 conditions that must all hold once the step has run. The conditions are closed and declarative, and they are combined with AND (ADR 6).

| Kind | Holds when |
|---|---|
| `url_matches` | the URL matches `pattern`. `*` is the only metacharacter. Matching is case-sensitive, and the fragment is dropped first. |
| `element_present` | at least one element of the snapshot taken after the step matches `locator` |
| `element_absent` | no element of that snapshot matches `locator` |
| `text_present` | `text` appears in the page text or, with `within`, in the one element that `within` names |
| `text_absent` | `text` does not appear there |

Text comparison ignores case. `within` must match exactly one element. If it matches none or several, the verdict is unevaluable with the reason `within_unresolved`.

Bounds. Each one is refused with 400 `BAD_REQUEST`, and `details.field` names the path of the field, never its value:
- 1 to 8 conditions;
- a `pattern` of 1 to 512 bytes, and a `text` of 1 to 256 bytes;
- each locator string of 1 to 256 bytes;
- no locator of kind `nth`, whose match depends on page order.

Page text is built in memory from the observation, capped at 256 KiB, and is never stored, logged or put in a verdict.

Verdict: `met`; `not_met` with the zero-based indexes of the failing conditions (`unmet`); or `unevaluable` with a closed reason (`no_snapshot`, `no_page_text` or `within_unresolved`).

Effect on the step's outcome:
- `met` turns an unknown outcome into verified, with the reason code `expect_satisfied`. A known outcome is kept as it is.
- `not_met` makes the outcome failed, with the reason code `expect_not_met` and the unmet indexes in the detail, whatever the base outcome was.
- `unevaluable` on an unknown outcome keeps it unknown and sets the detail `expect_unevaluable: <reason>`.

Under ZDR a run may send `expect`. The verdict is returned and nothing is stored.

### 2.2 BrowserProcedure version 2

Quarry compiles version 2 when the compile request sets `canonical: true`. Version 1, the receipt-literal form, is unchanged.

A version 2 procedure has `version` 2, `procedure_id`, `source_receipt_ids`, `steps` (1 to 100) and `slots`. Each step has:
- `step`, `action` (the stored action, with its closed type), `expected_outcome` (`verified`, `failed` or `unknown`) and `redacted_input`;
- `target`, for ref and semantic steps only: the `locator` Quarry resolved, the `fingerprint_id` and the `tag`. The fingerprint's normalized text is never stored;
- `slot`, for a typed step;
- `expect`, stored only when the verdict was `met` when the step was recorded.

Typed text. Every typed step is bound to a slot named `slot_<step>`. The stored text is empty, and the value is supplied at replay. A slot is `secret` when the captured text carried a password, one-time-code, card, secret, token, key or credential marker, and `text` otherwise. The save answer and the store show names and kinds only.

The closed vocabulary has 12 stored action types:
`navigate`, `click`, `click_semantic`, `type`, `type_semantic`, `select_semantic`, `wait_for_semantic`, `press`, `scroll`, `wait`, `back`, `get_content`.

The ref forms compile to the semantic forms: `click_ref` to `click_semantic`, `type_ref` to `type_semantic`, `select_ref` to `select_semantic`, `wait_for_ref` to `wait_for_semantic`. Every other action is refused with 501 `UNSUPPORTED`, which names only the action's type. That includes `evaluate` (ADR 6 keeps Evaluate denied), uploads, downloads, frame actions, a click at a point, a mouse wheel, `forward`, `reload`, screenshots, PDFs, a dialog response, a one-time-code fill, and the raw select and wait_for forms.

Other rules:
- No approval ids. An action that carries an approval grant is outside the vocabulary, and the store refuses an `approval_grant_id` key anywhere in a body.
- No redaction markers. A key press or option value that carries one is refused at compile (501 with `details.field`). The store refuses any `[redacted` marker in a body.
- No typed text. The store refuses a typed step that has non-empty text, or no declared slot.

Stable targets. A ref or semantic step takes the first of these locators that matches exactly one element of the snapshot, and that element must be the target's ref: test id; role with name; label; placeholder; text. Each value must be 1 to 120 bytes, carry no redaction marker, and contain no `@` and no run of six or more digits. There is no positional fallback, and a child-frame target gets no locator.

TARGET_REPAIR_REQUIRED is the ambiguity rule. When a step has no storable locator (none was unique, or the observation named no target), the compile fails closed with 409 `TARGET_REPAIR_REQUIRED` and `details.step`.

A receipt that is not completed, or whose business outcome did not verify, makes the compile answer 409 `CONFLICT`.

### 2.3 Replay check and prefix mode

`POST /v1/agent/procedures/replay-check` takes a procedure and a list of actions. It answers `"exact"`, or an object that names a repair or a refusal. It has no switch of its own. It runs no browser action and stores nothing.

For a version 2 procedure the comparison sets aside two things and requires everything else to match:
- `snapshot_id` and `generation` of a semantic action, which are run-scoped handles;
- `text` of a slot step, which is bound at replay.

With `prefix: true` the actions may be a leading part of the procedure. An action beyond the end of the procedure is always a repair. A version 1 procedure with `prefix` is refused with 400 `prefix_needs_v2`. A version 1 procedure without `prefix` keeps the strict comparison and its reasons.

The runner always checks in prefix mode, before it dispatches each step.

### 2.4 The store (capability-core)

Three tables, created by migration 0039:
- `procedures`: one row per recipe, with the owner, the organization, the scope (`user`), the name (1 to 120 characters), the description (at most 500), the source run id (at most 128 bytes) and the latest version.
- `procedure_versions`: one immutable row per version, with the canonical body (JSONB), a digest (lowercase sha256 hex over the body re-marshalled with sorted keys), the step count and the slot names.
- `procedure_tags`: a mutable pointer from a tag (`^[a-z0-9._-]{1,32}$`) to a version of the same recipe.

Rules:
- A version row is never updated. A delete is allowed only under `mp.version_erasure = 'on'`, or as the cascade of deleting the recipe itself.
- A body is at most 256 KiB, with 1 to 100 steps and at most 100 slots. A recipe holds at most 200 versions and 32 tags. A list page holds 20 by default and at most 100.
- Ownership comes from the verified token only. A user caller reads its own recipes. The execution-core runner is a service caller: it names the person with `for_user_id`, and a recipe that is not that person's answers 404.
- The migration is idempotent and runs at every boot. Its down file destroys every saved recipe, so it is not a way to switch the feature off (see RUNBOOKS.md).

Routes:
- HTTP, under `/api/v1/procedures`: create, list, get, get a version, add a version, set a tag, remove a tag and delete. Every route answers 404 `procedures_disabled` while `PROCEDURES_API` is off, before anything is read or written.
- gRPC `GetProcedureVersion`: answers Unavailable `procedures_disabled` while the switch is off. It serves the body in canonical form, and it refuses a body that no longer hashes to its digest.

### 2.5 Browser surface (Verevon gateway and SPA)

- Compile: `POST /api/v1/browser/sessions/{session_id}/procedure`. With `BROWSER_RECIPES` on, it asks Quarry for the canonical form, runs the quality check, and returns both.
- Save: `POST /api/v1/browser/sessions/{session_id}/recipes` with `{name, description?}` (unknown keys are refused), or the action `browser_evidence.save_recipe`. The gateway compiles again and checks promotion. A procedure that is not promotable answers 409 `recipe_not_promotable`, with up to 32 reasons. Otherwise the gateway posts to model-gateway `POST /v1/recipes`, which proxies to capability-core. The answer is 201 on the REST route and 200 on the action, with `recipeId`, `version`, `digest` and `slots` (names and kinds).
- List: `GET /api/v1/browser/recipes?limit&afterId` answers `{recipes, nextAfterId}`. Each item carries the id, name, description, source run, latest version and times. Items carry no slots (section 7).
- The save is human-only. `browser_evidence.save_recipe` is exempt from model access, so a model cannot save a recipe.

### 2.6 Starting a run from a recipe

`POST /api/v1/browser/sessions/{session_id}/ai-runs` accepts an optional `recipe`:

```
"recipe": { "id": "proc_<uuid>", "version": 1, "slots": { "slot_2": "<value>" } }
```

`version` and `tag` are exclusive. Before any Quarry or model call, the gateway refuses:
- an `id` that is not `proc_` followed by letters, digits, `_` or `-`, up to 64 bytes;
- a tag outside `^[a-z0-9._-]{1,32}$`;
- more than 32 slots, or a slot name outside `^[a-z0-9_]{1,40}$`;
- a slot value over 2000 characters.

No refusal echoes a slot value. A recipe start is also refused with an attached start (`recipe_attach_unsupported`) and with plan mode (`recipe_plan_mode_unsupported`).

The gateway sends the recipe to model-gateway `POST /v1/browser/runs`. model-gateway writes `tool_input.recipe` with `for_user_id` taken from the verified claims, never from the body. Slot values are never logged or put in run events. The goal still labels the run, but it does not steer a recipe.

## 3. The chain

1. Record. A browser session's steps become step receipts in Quarry. Only completed steps whose outcome verified can compile. A ZDR session writes no receipts.
2. Compile. Quarry's `POST /v1/agent/runs/{run_id}/procedure` with `canonical: true` builds version 2. The quality check, `POST /v1/agent/procedures/quality-check`, decides whether the procedure is promotable. The gateway makes both calls.
3. Save. The gateway compiles and checks again, and model-gateway's `POST /v1/recipes` proxies to capability-core's `POST /api/v1/procedures`. The store validates the body, computes the digest and writes version 1.
4. Run. The gateway starts a browser run with `recipe`. model-gateway checks the request and writes `tool_input.recipe`. execution-core's runner then:
   - reads the procedure from capability-core `GetProcedureVersion`, with a `capability:read` token minted for the run's organization;
   - refuses an answer that is not the procedure asked for, or whose body does not hash to its digest;
   - parses the procedure and binds the slots;
   - for each step, asks Quarry's replay check in prefix mode, proposes the step only on `exact`, and dispatches it.
   Quarry executes every step under the run's own broker grant. No stored approval is reused. The loop applies its usual gates (risk, approval, budget and takeover) to every action.
5. Hand back. A mismatch ends the run with a fixed reason, and no later step is dispatched. The planner never re-proposes a step, never asks a model and never substitutes another action. The reasons are in RUNBOOKS.md, section "Saved recipes".

The model chooses nothing in a recipe run. The person starts it.

## 4. Switches

Every switch defaults to off. A value other than an accepted one leaves the switch off.

| Switch | Read by | Accepted values, and when read | Default, and where set | Turns on | Off means |
|---|---|---|---|---|---|
| `QUARRY_EDGE__STEP_EXPECT` | quarry-edge, `step_expect_enabled` (`crates/quarry-runtime/src/step_expect.rs`), admitted in `crates/quarry-edge/src/agent_routes/step.rs` | `1`, `true`, `yes`, `on`; trimmed, any case; read per call | off; set in the edge's environment, which no compose file in this tree sets | `expect` on a step | a step that carries `expect` is refused 400 `unsupported_option` |
| `QUARRY_EDGE__PROCEDURE_CANONICAL` | quarry-edge, `procedure_canonical_enabled` (`crates/quarry-runtime/src/browser_procedure.rs`), used in `crates/quarry-edge/src/agent_routes/receipts.rs` | `1`, `true`, `yes`, `on`; trimmed, any case; read per call | off; set in the edge's environment, which no compose file in this tree sets | a `canonical` compile (version 2) | a `canonical` compile is refused 400 `procedure_canonical_disabled`, before any receipt is read |
| `PROCEDURES_API` | capability-core, `ProceduresAPIFromLookup` (`apps/Model Plane/go/services/capability-core/internal/api/procedures.go`), wired in `cmd/main.go` | the literal `true` only; read at start | off; `apps/Model Plane/deploy/docker-compose.yml` (capability-core) | the `/api/v1/procedures` routes and the gRPC read | 404 `procedures_disabled`; the stored rows stay, and nothing reads them |
| `EXECUTION_CORE_RECIPE_RUNNER` | execution-core, `recipe_runner_enabled` (`apps/Model Plane/rust/services/execution-core/src/recipe_store.rs`), checked per run in `tool_bridge/mod.rs` | the literal `true` only; read per run | off; `deploy/docker-compose.yml` (execution-core) | recipe runs | a run that names a recipe fails at start with "recipe runner is not enabled" |
| `MODEL_GATEWAY_BROWSER_RECIPE` | model-gateway, `recipe_flag` (`apps/Model Plane/rust/services/model-gateway/src/browser_run.rs`), read per start | the literal `true` only | off; `deploy/docker-compose.yml` (model-gateway) | a browser-run start may name a recipe | 400 `browser_recipe_unavailable`, before any grant, thread or run is written |
| `BROWSER_RECIPES` | the Verevon gateway, `apps/Frontend Plane/verevonv3/apps/gateway/src/config.rs`, read once at start-up into the gateway's config | `1`, `true`, `yes`, `on`; trimmed, any case; read at start | off; the gateway's environment, which no compose file in this tree sets | the save and list routes, the canonical compile, and recipe starts | the HTTP routes are absent; the save action answers 404 `browser_recipes_unavailable`; a recipe start answers 400 `browser_recipes_unavailable` |
| `VITE_BROWSER_RECIPES` | the SPA: `ProcedurePanel.tsx`, `RecipesPanel.tsx` and `recipesEnabled` in `use-browser-evidence.ts`; typed in `src/vite-env.d.ts` | the literal `true` only; read at build time, since Vite inlines it | off; the SPA's build or dev environment (it is not in `.env.example`) | the Save control and the saved list with Run | neither control is shown |

Notes on the table:
- "Read at start" and "read at build time" mean that a change needs a restart or a rebuild. A dev server must be restarted. A built image must be rebuilt.
- The Control Plane flag registry (`apps/Control Plane/flag-registry/flag-registry.json`) lists `PROCEDURES_API`, `EXECUTION_CORE_RECIPE_RUNNER` and `MODEL_GATEWAY_BROWSER_RECIPE`. It does not list the Quarry switches, the gateway switch or the SPA switch.
- Of the seven, three are wired with default false in the Model Plane compose file. The other four are not wired in any compose file or env example in this tree, so an operator sets them on the process.
- Dependencies. Saving needs `QUARRY_EDGE__PROCEDURE_CANONICAL`, `BROWSER_RECIPES` and `PROCEDURES_API`. Running needs `PROCEDURES_API`, `EXECUTION_CORE_RECIPE_RUNNER`, `MODEL_GATEWAY_BROWSER_RECIPE` and `BROWSER_RECIPES`, since the gateway refuses a recipe start without the last. A recipe run also needs execution-core's Quarry agent client (`QUARRY_BROWSER_AGENT_ENABLED=1` and `QUARRY_EDGE_URL`). `QUARRY_EDGE__STEP_EXPECT` matters only for `expect`. The SPA controls need `VITE_BROWSER_RECIPES`.

## 5. ZDR

ZDR runs write no receipts, and a ZDR session cannot be saved.
- Quarry: a compile for a run with no durable receipts answers 404 "agent run has no durable receipts".
- Verevon gateway: compile and save answer 409 `browser_receipts_unavailable_zdr` before the compile is asked. The SPA disables Save and Run for a ZDR session.
- model-gateway: a browser-run start for a ZDR organization or request answers 412 `zdr_feature_not_supported` (feature `browser_run`), before any recipe is read. The runner never sees a ZDR run.

What the store holds: the stored actions (their types, the URL of a `navigate`, keys, and scroll and wait values), semantic locators (role and name, label, text, placeholder and test id), slot names and kinds, and outcome labels. It holds no typed value, no approval id and no redaction marker. The store is content-bearing, because locators and URLs can carry page text. capability-core describes the body as org configuration rather than conversation content, and the code puts the ZDR refusal at the gateway.

Erasure. Organization erasure in session-core (`apps/Model Plane/rust/services/session-core/src/gdpr.rs`, Q4-E4.2-3j) deletes the organization's `procedure_tags`, `procedure_versions` and `procedures`, in that order, inside the organization-erasure transaction, on the shared database, when migration 0039 is present. A per-person erasure does not remove that person's recipes. The per-owner path is not built, as for skills.

## 6. Out of scope

- Sharing (ADR 8). Scope is `user`. There is no share route and no share action.
- Routines and unattended runs (ADR 9). A recipe run starts on a person's click. No schedule or routine starts one.
- Script execution (ADR 6, the code-mode question). Actions stay a closed, declarative list. Evaluate is refused at every layer: the compile, the store and the runner.
- A model choosing or saving a recipe. The save is human-only, the start is user-initiated, and the planner is deterministic.
- Attached and plan-mode recipe runs. They are refused by the gateway, by model-gateway and by execution-core.
- Running a recipe with a secret slot. The runner refuses it in this build (section 7).
- Deleting a recipe from the browser. The store and model-gateway have delete routes. The Verevon gateway exposes none, and no action removes a recipe.

## 7. Known gaps (2026-11-11)

1. Nothing produces `expect`. No caller in this tree sends `expect` on a step request, except the recipe runner, which copies it from a stored recipe. Nothing on the record path sends one, so a recipe recorded from an ordinary session has no `expect` steps, and its replay checks targets, slots and outcomes only. The switch enables the mechanism. The first producer is not built.
2. The saved list has no slots. The store's list items and the gateway's list view both omit `slots`. The SPA's Run control reads a recipe with no slots as unknown, so Run is disabled for every recipe. Run cannot start a recipe from the SPA until the list carries the slot names, or the panel reads the version's body.
3. The save is not idempotent at the gateway. The action surface supplies an idempotency key, but the save dispatcher does not read it, so a timed-out retry can create a second recipe. The disabled button stops only a double click.
4. Secret slots are saved but never run. A run of a recipe with a secret slot is refused with `recipe refused: secret_slot`.
5. The browser does not see Quarry's reason. A canonical compile that fails closed reaches the browser as 409 `browser_procedure_not_compilable`. `TARGET_REPAIR_REQUIRED` and its `details.step` are not passed through.
6. Per-person erasure is not built (section 5).
7. Four switches are not wired in any compose file or env example in this tree (section 4).
8. Selector steps are kept. The version 2 compile keeps a `click` or `type` step that was recorded with a CSS selector, as it was recorded. Such a step has no semantic target, and the runner replays it by its selector.
9. Nothing here is compiled, tested or run against a stack. The first live check is in RUNBOOKS.md, section "Saved recipes".

## 8. Where the code is

| Part | File |
|---|---|
| Step postcondition contract | `apps/Ingestion Plane/Quarry-v2/crates/quarry-core/src/step_expect.rs` |
| Step postcondition evaluator and reader | `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/step_expect.rs`, `observation.rs` |
| Step admission (`expect`) | `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/agent_routes/step.rs` |
| Canonical compile, the version 2 vocabulary and the replay | `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/browser_procedure.rs` |
| Procedure routes | `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/agent_routes/receipts.rs`, `types.rs`, `router.rs` |
| Store migration and package | `apps/Model Plane/go/services/capability-core/migrations/0039_procedures.up.sql`, `0039_procedures.down.sql`, `internal/procedures/` |
| Store HTTP and gRPC | `apps/Model Plane/go/services/capability-core/internal/api/procedures.go`, `internal/server/server.go`, `cmd/main.go` |
| model-gateway proxies and run start | `apps/Model Plane/rust/services/model-gateway/src/http_routes.rs`, `browser_run.rs` |
| Runner reader, the recipe planner and the tool bridge | `apps/Model Plane/rust/services/execution-core/src/recipe_store.rs`, `browser_agent/recipe.rs`, `browser_agent/recipe_planner.rs`, `tool_bridge/mod.rs` |
| Verevon gateway | `apps/Frontend Plane/verevonv3/apps/gateway/src/config.rs`, `domains/browser_evidence.rs`, `domains/browser.rs` |
| SPA | `apps/Frontend Plane/verevonv3/src/features/browser/evidence/ProcedurePanel.tsx`, `RecipesPanel.tsx`, `use-browser-evidence.ts`; `src/shared/actions/action-registry.ts` |
| Organization erasure | `apps/Model Plane/rust/services/session-core/src/gdpr.rs` |
