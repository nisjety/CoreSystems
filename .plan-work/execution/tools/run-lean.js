export const meta = {
  name: 'run-lean',
  description: 'Code-first Wave 3 batch: one agent per slice or design (at most 8), no test runs, then one guard agent that applies shared edits and regeneration and runs the compile checks one at a time',
  phases: [
    { title: 'Build', detail: 'one agent per slice (haiku max for mechanical, opus medium for design-heavy) and per missing design (opus medium)' },
    { title: 'Guard', detail: 'shared edits, regeneration, then cargo check / go build / tsc6 one at a time, fixing compile errors' },
  ],
}

// args: { batchId, wave: 3, goal, argsFile, slices: [ {id, title, importance, strict, risk[], status, decisions[], notes} ], designs: [ {epic, file, rowsFile, brief} ] }
// Full slice rows (files, new_files, summary, flag, decisions_defaults, notes) are in args.argsFile, so the prompt stays small.
const REPO = 'C:\\dev\\CoresSystem\\.claude\\worktrees\\epic-shaw-4d613b'
const NOTES = REPO + '\\.plan-work\\execution'
const TARGET = 'C:/dev/mp-target-w2'

const RESULT = {
  type: 'object',
  properties: {
    id: { type: 'string' },
    status: { type: 'string', enum: ['done', 'partial', 'blocked'] },
    files_changed: { type: 'array', items: { type: 'string' } },
    lead_todo: { type: 'array', items: { type: 'string' }, description: "'shared-edit <file>: <exact lines and where>' for shared files, plus regeneration or lead-only steps" },
    deviations: { type: 'array', items: { type: 'string' } },
    needs_other_files: { type: 'array', items: { type: 'string' } },
    depth_findings: {
      type: 'array',
      items: {
        type: 'object',
        properties: {
          kind: { type: 'string', enum: ['live-proof', 'test-gap', 'risk', 'follow-up', 'decision', 'cleanup'] },
          severity: { type: 'string', enum: ['P0', 'P1', 'P2', 'P3'] },
          text: { type: 'string' },
        },
        required: ['kind', 'severity', 'text'],
      },
    },
    notes: { type: 'string' },
    worktree: { type: 'string', description: 'isolated mode only: the absolute path git rev-parse --show-toplevel printed' },
  },
  required: ['id', 'status', 'files_changed', 'notes'],
}

const DESIGN = {
  type: 'object',
  properties: {
    epic: { type: 'string' },
    design_file: { type: 'string' },
    rows_file: { type: 'string' },
    slices: { type: 'array', items: { type: 'string' } },
    reserved_numbers: { type: 'array', items: { type: 'string' } },
    owner_decisions: { type: 'array', items: { type: 'string' } },
    notes: { type: 'string' },
  },
  required: ['epic', 'design_file', 'rows_file', 'slices', 'notes'],
}

const GUARD = {
  type: 'object',
  properties: {
    shared_edits_applied: { type: 'array', items: { type: 'string' } },
    shared_edits_skipped: { type: 'array', items: { type: 'string' } },
    regenerated: { type: 'array', items: { type: 'string' } },
    checks: { type: 'array', items: { type: 'object', properties: { command: { type: 'string' }, outcome: { type: 'string' }, clean: { type: 'boolean' } }, required: ['command', 'outcome', 'clean'] } },
    fixes: { type: 'array', items: { type: 'object', properties: { file: { type: 'string' }, slice: { type: 'string' }, what: { type: 'string' } }, required: ['file', 'what'] } },
    still_broken: { type: 'array', items: { type: 'string' } },
    notes: { type: 'string' },
  },
  required: ['checks', 'fixes', 'still_broken', 'notes'],
}

const RULES = `Hard rules (repository CLAUDE.md files and the owner):
- Work only inside ${REPO} (git worktree, branch model-plane). Never touch C:/dev/CoresSystem (the live tree) or another worktree. Never commit, stage, push, open a PR, or run git reset/checkout/clean/stash/merge. The lead commits.
- CODE FIRST (owner order 2026-11-03): write the code and the unit tests the slice names, but do NOT run any test, build, lint, typecheck, cargo, go, pnpm, node, python test or docker command. One guard agent compiles the whole batch after you. Read-only git (status, diff, log, show) is fine.
- Never read, print, write or change a credential, token, key or .env value; never edit .env.example (the owner does).
- Every new switch defaults OFF and is read with the repo's strict parser (only the literal "true" enables it); with every switch off, behaviour is byte-identical to before. Proto changes are additive only, every new field and message commented. Migrations ship a revert file.
- Architecture: no direct database access across planes; one owner per capability (apps/Model Plane/docs/capability-ownership-matrix.md); ZDR must propagate through every content-persisting boundary; error bodies use the typed envelope {error:{code,message,details}} with details an object (a list goes under a named key: the SPA client drops array details).
- Ownership: edit ONLY your slice's files and new files (plus test files beside what you change). Another agent owns every other file in this batch and edits it at the same time. If the slice needs a change elsewhere, put it in lead_todo as 'shared-edit <file>: <exact lines and where>' (the guard applies it after you) or in needs_other_files.
- zdr-posture-matrix.json rows are keyed on resolver inputs only (bearers, route): never add a row for a feature refusal (it duplicates an existing row and fails zdr-contract-schema "rows must differ"); put feature refusals in zdr_features.rs, the feature table and docs/ZDR.md.
- Line endings: measure each file (python: open(p,'rb').read().count(b'\\r\\n') vs count(b'\\n')) and keep its style; edit with the Edit tool; never reformat a whole file.
- Match the surrounding code's naming, comment density and idiom; comments say why; no dead code; no emojis.`

const sliceBlock = (s) => `Slice ${s.id} - ${s.title}. Queue status: ${s.status || 'todo'}. Importance: ${s.importance}. Risk: ${(s.risk || []).join(', ') || 'none'}.
Your full assignment is entry "${s.id}" of the JSON file ${args.argsFile} (key "slices"): read it first. It holds the summary, the remaining work, your owned files ("files", "new_files"), the switch ("flag"), decisions taken by default ("decisions_defaults"), dependencies and the lead's notes (design file and section, reserved migration/proto numbers, shared files that are not yours).
Read the design named in the notes: its section 2 (verified state), section 3 (decisions) and your slice's section in full, and docs/plans/wave3/00-integration-round-a.md and 00-integration-round-b.md for reserved numbers (round B's session-core range 0090+ and its renumbering win). Find code by name; line numbers drift.
${s.status === 'blocked-decision' ? `This row is BLOCKED ON OWNER DECISIONS (${(s.decisions || []).join(', ')}). The owner ordered code first and "stop asking": for each of those decisions take the design's recommended option (section 3), build it dark behind the slice's default-off switch (add one if the design names none), and record each choice as a deviation "Assumed <id> = <recommendation>" plus a depth finding of kind decision, P1. If a decision has no recommendation and no safe dark build exists, build what does not depend on it and set status partial.` : ''}
${s.notes ? 'Lead notes: ' + s.notes : ''}`

// Isolated mode (args.isolate): each builder edits its own fresh git worktree, so slices that share files can run
// side by side; it hands its work back as one patch, and the guard merges the patches into the lead tree 3-way.
const ISO = !!(args && args.isolate)
const PATCHDIR = REPO + '\\.plan-work\\execution\\patches\\' + ((args && args.batchId) || 'batch')
const isoBlock = (s) => ISO ? `\n\nISOLATED WORKTREE (this overrides the "Work only inside" rule above): you run in your own fresh git worktree, a checkout of branch model-plane's HEAD; your current directory is its root. FIRST run "git rev-parse --show-toplevel": if it prints ${REPO.replace(/\\/g, '/')} (the lead's tree), stop at once and return status blocked with notes "not isolated". The isolation cuts worktrees from the repository's default branch, NOT from model-plane, so SECOND, in your worktree only: if "git status --porcelain" is empty and "git rev-parse HEAD" differs from "git rev-parse model-plane", run "git checkout -q --detach model-plane" (this one checkout is allowed, in your own fresh worktree only; it never moves the model-plane branch) and confirm HEAD now equals model-plane. Edit only inside your worktree; read the args file at its absolute path. Never run cargo, go build, pnpm install, npm or any build in it (no target/ or node_modules/ may appear: the disk filled up twice from builder build output); only the guard compiles. Report that top-level path in the worktree field. Other builders change other copies of the same files at the same time; keep your edits to what your slice needs so the 3-way merge stays clean. WHEN DONE: from your worktree root run "git add -A", then mkdir -p "${PATCHDIR.replace(/\\/g, '/')}" and "git diff --cached --binary > '${PATCHDIR.replace(/\\/g, '/')}/${s.id}.patch'"; check the patch is not empty; do not commit, reset or clean. Report files_changed as repo-relative paths.` : ''

const buildPrompt = (s) => `You are an engineer implementing one slice of the Wave 3 build of the Model Plane chat plan in this repository, code first. YOUR TASK IS TO BUILD THIS SLICE: the owner ordered the Wave 3 code build (2026-11-03, and on 2026-11-04 "continue and speed up code build on w3"). A status question or any other message elsewhere in the conversation is not your task and never a reason to stop or only report.\n${RULES}${isoBlock(s)}\n\n${sliceBlock(s)}\n\nFIRST check git status and git diff for your owned files: an earlier attempt may have left partial edits; keep what is right and finish it. Then build the slice: the code and the unit tests its row names (write them, do not run them), with every new switch off. When the slice needs a rebuilt image, a deploy, a live proof, a regenerated golden or generated code, do not do it: say exactly what in lead_todo (the guard regenerates proto bindings, the agent-action-admission golden and the prompt-tool manifest). Record anything for later (live proofs, risks, decisions, edge cases) in depth_findings with kind, severity (P0 a hole in authority/ZDR/money/data visible in code; P1 needed before switch-on; P2 quality; P3 cleanup) and one precise sentence. Keep the report short and exact.`

const designPrompt = (d) => `You are a staff engineer writing the missing Wave 3 design for ${d.epic} of the Model Plane chat plan, in this repository. Write documents only: no source edits, no builds, no tests.\n${RULES}\n\nBrief: ${d.brief}\n\nRead first: the plan section for ${d.epic} in docs/plans/model-plane-chat-build-plan-2026-10-14.md (Grep "### ${d.epic} "), the epic table row and the trunk/dependency tables near it; docs/plans/wave3/EP-5-deck-pipeline.md as the TEMPLATE for structure and depth (sections 1 Scope/items/owner, 2 Verified current state with file:line evidence you actually checked in this tree, 3 Decisions and open questions with a recommended option for each, 4 Slices "### Sn - <title> [BUILD|FINISH]" each with owned files, new files, proto/migration numbers, switch, tests and definition of done, 5 ZDR clause, 6 Proof plan, 7 Risks and what the owner must decide); docs/plans/wave3/00-integration-round-a.md and 00-integration-round-b.md (numbering rules, hot files). Verify the current state by reading the code (Grep/Read), not from memory. Reserve numbers that do not collide with this tree: session-core migrations take the next free number at or above 0105 (list apps/Model Plane/rust/services/session-core/migrations), capability-core at or above 0035, proto tags the next free tag in each message (read the .proto), and say them in the design. Channel Plane is docs-only: do not design runtime against it. Slice the epic so each slice is buildable by one agent in one pass with a disjoint file set, the first slices buildable now with switches off, decisions taken at their recommended default.\n\nWrite the design to ${d.file}. Then write the queue rows to ${d.rowsFile}: a JSON array, one object per slice, with exactly the keys and value types of this example row (copy the shape, not the content): read the row "EP-5-S1" in ${NOTES}\\queue.json (python: json.load, pick id EP-5-S1). Use ids "${d.epic}-S1", "${d.epic}-S2", ...; status "todo" for slices buildable now with defaults, "blocked-external" only for flips/live proofs needing the owner; depends_on with real ids (W1/W2 ids like WS05-S3 or EP ids); importance "opus" for authority/ZDR/design-heavy slices, "sonnet" otherwise; files and new_files as repo-relative paths; lead_shared_files for compose, ledger, ownership matrices, register; decision_defaults_applied mapping decision id to the recommendation you build. Return the slice ids, the numbers you reserved and the owner decisions.`

const guardPrompt = (results, designs) => `You are the batch guard for Wave 3 batch ${args.batchId} in ${REPO} (branch model-plane). The builders have finished; nobody else edits the tree now. Your job: make the batch compile. Do NOT run tests (no cargo test except the one generation step below, no vitest except the golden generation below, no go test), no docker, no commits.\n${RULES.replace('do NOT run any test, build, lint, typecheck, cargo, go, pnpm, node, python test or docker command. One guard agent compiles the whole batch after you.', 'you are the one agent that compiles; run the checks below one at a time, never two cargo commands at once.')}\n\nBuilder reports (JSON): ${JSON.stringify(results.map(r => ({ id: r.id, status: r.status, files_changed: r.files_changed, lead_todo: r.lead_todo, needs_other_files: r.needs_other_files })))}\n\nSteps, in order:${ISO ? `
0. MERGE THE ISOLATED BUILDERS' PATCHES FIRST. Each builder worked in its own worktree and left one patch in ${PATCHDIR.replace(/\\/g, '/')}/<slice id>.patch. From ${REPO.replace(/\\/g, '/')}, apply them in this order: ${results.map(r => r.id).join(', ')}, each with: git apply --3way --whitespace=nowarn "<patch>". A conflict leaves markers in the file: resolve it by keeping BOTH slices' changes (they are different features in the same file), keep the file's line endings, and continue with the next patch. If a patch will not apply at all, run git apply --3way --reject on it and merge every .rej hunk by hand, then delete the .rej files. A missing or empty patch: list it in still_broken. When all are in, run "git reset -q" (unstage only; the working tree keeps every change) so the lead's per-slice commits see the changes, and confirm with "git diff --stat" that each slice's files changed. This merge and that one reset are the only git writes you make.` : ''}
1. Apply every 'shared-edit' item in the reports EXCEPT edits to apps/Model Plane/docs/decisions/ledger.md, docs/plans/model-plane-depth-register.md and any .env.example (the lead books those). Shared files are compose files, mod/lib lines, struct literals in files no builder owns, ownership matrices, CI lists. Keep each file's line endings. List what you applied and skipped.
2. Regenerate generated code if a builder changed a .proto: Go: from "apps/Model Plane/proto" run buf generate --template ../../../.plan-work/buf.gen.local2.yaml (output .plan-work/gen-out), then copy only the model_plane files whose content differs ignoring CR into apps/Model Plane/go/gen/model_plane/v1/ (PATH needs $HOME/go/bin); then bash "apps/Model Plane/scripts/tests/go-gen-drift.sh". Python: buf generate --template ../../../.plan-work/buf.gen.py-remote.yaml (output .plan-work/gen-py) and copy only files whose content differs from the tree after removing the "# Protobuf Python Version" line and the ValidateProtobufRuntimeVersion(...) call (never use raw protoc). Rust bindings are built by mp-contracts' build.rs.
3. If src/shared/actions/action-registry.ts changed: from "apps/Frontend Plane/verevonv3" run pnpm_config_verify_deps_before_run=false VEREVON_UPDATE_GOLDEN=1 node node_modules/vitest/vitest.mjs run --pool=forks src/shared/actions/admission-contract.test.ts; copy the written src/shared/api/contracts/agent-action-admission.json byte-identically over apps/Model Plane/api/contracts/agent-action-admission.json and the copies in apps/Model Plane/go/services/capability-core/internal/api/testdata/, apps/Model Plane/rust/services/execution-core/tests/fixtures/ and apps/Model Plane/rust/services/model-gateway/tests/fixtures/; write its sha256 as "<hex> *agent-action-admission.json" into apps/Model Plane/api/contracts/agent-action-admission.json.sha256 and replace the old digest on the agent-action-admission line of scripts/tests/zdr-contract-sync.test.sh.
4. If a platform prompt or a model tool definition changed: from "apps/Model Plane/rust" run MP_UPDATE_PROMPT_MANIFEST=1 CARGO_TARGET_DIR=${TARGET} CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=4 cargo test -q -p mp-contracts --test prompt_tool_manifest (a generation step). If an SSE event or its payload changed (sse_events.rs, contracts/sse-event-payloads.json): MP_UPDATE_GOLDEN=1 with the same env, cargo test -q -p model-gateway --lib sse_event_payload_golden (also a generation step), then copy the regenerated api/contracts/sse-event-payloads.json and its .sha256 byte for byte to the SPA copy under src/shared/api/contracts/ and run node scripts/gen-chat-event-types.mjs from apps/Frontend Plane/verevonv3.
4b. If apps/Model Plane/api/contracts/zdr-posture-matrix.json changed: run node scripts/tests/zdr-contract-schema.mjs; a row whose inputs equal another row must be removed (restore the matrix, its vendored copies, their .sha256, the ENFORCEMENT_ROWS constants and the zdr-contract-sync pin), and say so.
5. Compile checks, one at a time, only for what the batch touched: (a) Rust Model Plane: from "apps/Model Plane/rust", CARGO_TARGET_DIR=${TARGET} CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=4 cargo check --workspace --all-targets --message-format short; (b) verevonv3 gateway (apps/Frontend Plane/verevonv3/apps/gateway): cargo check --manifest-path "apps/Frontend Plane/verevonv3/apps/gateway/Cargo.toml" --all-targets with the same env; (c) Go: go build ./... and go vet ./... in each touched Go module (Model Plane go services use the go.work in apps/Model Plane/go; modules elsewhere use GOWORK=off); (d) frontend: from "apps/Frontend Plane/verevonv3", node node_modules/typescript/bin/tsc6 -p tsconfig.app.json --noEmit (baseline: exactly 1 pre-existing error in src/shared/api/schedule-contract.test.ts).
6. Fix every compile error with the smallest correct edit (missing struct fields, mod lines, imports, type mismatches, wrong names), preferring the batch's own files; repeat the failing check until clean. Do not redesign a slice; if an error needs real design, leave it and list it in still_broken with file:line.
Return what you applied, regenerated, ran (with outcome) and fixed (file, slice it belongs to, what).`

phase('Build')
const jobs = [
  ...(args.slices || []).map(s => () =>
    agent(buildPrompt(s), {
      label: `build:${s.id}`, phase: 'Build', schema: RESULT,
      model: (s.importance === 'opus' || s.strict) ? 'opus' : 'haiku',
      effort: (s.importance === 'opus' || s.strict) ? 'medium' : 'max',
      ...(ISO ? { isolation: 'worktree' } : {}),
    }).then(r => r ? { ...r, id: s.id } : { id: s.id, status: 'blocked', files_changed: [], notes: 'agent returned nothing' })),
  ...(args.designs || []).map(d => () =>
    agent(designPrompt(d), { label: `design:${d.epic}`, phase: 'Build', schema: DESIGN, model: 'opus', effort: 'medium' })
      .then(r => ({ design: true, ...(r || { epic: d.epic, notes: 'agent returned nothing' }) }))),
]
const done = (await parallel(jobs)).filter(Boolean)
const results = done.filter(r => !r.design).map(r => ({
  ...r,
  outcome: r.status === 'done' ? 'ready' : r.status === 'partial' ? 'ready-partial' : 'needs-attention',
  tests_run: [],
}))
const designs = done.filter(r => r.design)

phase('Guard')
const guard = results.length
  ? await agent(guardPrompt(results, designs), { label: 'guard', phase: 'Guard', schema: GUARD, model: 'opus', effort: 'medium' })
  : null
return { batchId: args.batchId, wave: 3, results, designs, guard }
