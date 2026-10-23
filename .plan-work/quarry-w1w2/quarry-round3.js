export const meta = {
  name: 'quarry-round-3',
  description: 'Round 3 (Sonnet 5.5 high and Haiku 5.5 max only; continues the cut-off round 2 from the tree) on quarryv2-next: eight units close the real defects and leftovers of round 1 and build WS1.C, each reviewed by a panel; then doc reconciliation, rebuilt-edge evidence, full integration and a completeness critic. At most 4 agents at once',
  whenToUse: 'Continue the Quarry v2 and in-app browser build plan after round 1 (HEAD eb92d4ee)',
  phases: [
    { title: 'Implement', detail: 'eight units in parallel in the shared worktree' },
    { title: 'Review', detail: 'a panel of reviewers per unit' },
    { title: 'Fix', detail: 'implementer fixes the high and medium findings, a verifier checks' },
    { title: 'Stage 2', detail: 'documents reconciled with the final state' },
    { title: 'Evidence', detail: 'rebuilt-edge evidence runs' },
    { title: 'Integrate', detail: 'full test pass, hand-over notes, completeness critic' },
  ],
}

// ---------------------------------------------------------------- constants
const WT = 'C:/dev/CoresSystem/.claude/worktrees/vigorous-perlman-e7ae5b'
const Q = WT + '/apps/Ingestion Plane/Quarry-v2'
const S = 'C:/Users/IMAFER~1/AppData/Local/Temp/claude/C--dev-CoresSystem--claude-worktrees-vigorous-perlman-e7ae5b/562f6350-4d29-46dc-a04b-eee466d0c963/scratchpad'
const NOTES = 'C:/Users/ImaFernandesDaCosta/AppData/Roaming/Claude/side-session-notes/local_6a0cd694-160f-4e6d-9b45-da7a2400e1ec'
const PLAN = 'docs/plans/quarry-browser-build-plan-2026-10-14.md'
const ROUND3 = `ROUND 3 NOTICE. This is a re-run: round 2 was cut off by a usage limit at about 20:00 on 2026-10-18, and your unit may have UNCOMMITTED work in the tree plus a progress file ${NOTES}/wf-<unit id>-progress.md. FIRST read that progress file, then git status and git diff on your owned paths. Finish and COMMIT what is already there (after the tests it needs have passed) before you start any new item, in small steps, so that a second cut-off loses nothing. Only Sonnet and Haiku models run in this round. Keep tool use lean: do not re-read large files you do not need, and do not repeat a test run whose result is already in the progress file unless you changed code since.`

const HAIKU = { model: 'haiku', effort: 'max' }
const SONNET = { model: 'sonnet', effort: 'high' }
const OPUS = { model: 'sonnet', effort: 'high' } // round 3: Opus is not used; every former OPUS stage runs on Sonnet 5.5 high

// ------------------------------------------------- at most 8 agents at a time
const MAX_AGENTS = 4
let active = 0
const waiters = []
async function slot() {
  if (active < MAX_AGENTS) { active++; return }
  await new Promise((resolve) => waiters.push(resolve)) // the slot is handed over, active stays
}
function release() {
  const next = waiters.shift()
  if (next) next()
  else active--
}
async function A(prompt, opts) {
  await slot()
  try { return await agent(prompt, opts) } finally { release() }
}

// ---------------------------------------------------------------- schemas
const IMPL = {
  type: 'object',
  properties: {
    status: { type: 'string', enum: ['done', 'partial', 'blocked'] },
    commits: { type: 'array', items: { type: 'string' } },
    summary: { type: 'string' },
    verified: { type: 'array', items: { type: 'string' } },
    not_done: { type: 'array', items: { type: 'string' } },
    notes_file: { type: 'string' },
  },
  required: ['status', 'commits', 'summary', 'verified', 'not_done'],
}
const REVIEW = {
  type: 'object',
  properties: {
    verdict: { type: 'string', enum: ['approve', 'changes_needed'] },
    findings: {
      type: 'array',
      items: {
        type: 'object',
        properties: {
          severity: { type: 'string', enum: ['high', 'medium', 'low'] },
          file: { type: 'string' },
          issue: { type: 'string' },
          evidence: { type: 'string' },
          suggested_fix: { type: 'string' },
        },
        required: ['severity', 'file', 'issue', 'evidence'],
      },
    },
  },
  required: ['verdict', 'findings'],
}
const VERIFY = {
  type: 'object',
  properties: {
    closed: { type: 'array', items: { type: 'string' } },
    still_open: { type: 'array', items: { type: 'string' } },
    new_problems: { type: 'array', items: { type: 'string' } },
  },
  required: ['closed', 'still_open', 'new_problems'],
}

// ------------------------------------------------------------ shared preamble
const PRE = `You are one of several agents working IN PARALLEL in ONE shared git worktree. Read this whole section before you do anything.

REPO, BRANCH, RULES
- Worktree (work only here): ${WT}   branch quarryv2-next. The Quarry v2 workspace is ${Q} (abbreviated Q below). Host is Windows: use the Bash tool (Git Bash) with forward-slash paths; the repo is a monorepo whose planes are described in CLAUDE.md (already loaded for you).
- Local commits only. No push, no pull request, no edits outside this worktree (never C:/dev/CoresSystem itself, never C:/dev/verevon-audit-2026-10-14), never --no-verify.
- FORBIDDEN git commands: stash, reset, checkout, restore, clean, rebase, merge, commit --amend, add -A, add . . Other agents have uncommitted edits in this same tree and those commands destroy them. To undo an experiment of your own, edit it back by hand.
- First read the history: run  git log -n 50 --format='%h %B' -- . | head -400  to see what shipped and each commit's "Not done" list. The plan is ${PLAN}; use grep -n "<slice id>" to find a slice and read its "Slice WSx.y detail" paragraph.
- Honesty: report only what you actually ran and saw. If a test did not run, say so. Never write a commit message claim you did not verify.

HOW TO BUILD AND TEST (important: Docker Desktop on this machine has crashed under parallel cargo containers)
- Rust in the Quarry workspace: ALWAYS through  bash ${S}/tools/qtest.sh '<shell commands>'  . It takes a global docker lock (other agents wait their turn; waiting up to 45 minutes is normal), runs a Linux container (image quarry-rust-env:1 = rust:1 plus cmake, clang, openssl, go, protoc and chromium; it starts in seconds, nothing is installed per run) with CARGO_BUILD_JOBS=2 over Q, and prints the command output. Put your filters inside the string, e.g.  bash ${S}/tools/qtest.sh 'cargo test -p quarry-edge --lib 2>&1 | grep -E "test result|FAILED|^error|panicked" -A8 | head -60'. For a different workspace set QROOT and a separate volume, e.g.  QROOT="${WT}/apps/Model Plane/rust" QTARGET=mp-target bash ${S}/tools/qtest.sh '...'. Live-Chromium tests: prefix the command with  CHROMIUMOXIDE_TEST=1 QUARRY_BROWSER_NO_SANDBOX=1 CHROME=/usr/bin/chromium  (chromium is in the image).
- Any other docker command that runs a container or builds an image: wrap it as  bash ${S}/tools/with-lock.sh docker 2700 -- <command> . Never start more than one container at a time yourself.
- If qtest prints "DOCKER IS NOT ANSWERING" or a docker command fails with a 500 from the engine: STOP, finish what needs no docker, and report it in not_done. Do not retry in a loop.
- Go and the frontend gateway (apps/Frontend Plane/verevonv3/apps/gateway) build natively on the host: cd into the crate and run cargo test / go test directly (no docker needed). Node tests run with node --test.
- Long commands (> ~2 minutes): start them with run_in_background writing to a log file, then wait with  bash ${S}/tools/qwait.sh <logfile> '<regex of the end marker>' 540  (ONE call that blocks up to 9 minutes, prints the match and the log tail). Never poll in many short calls and never re-arm Monitor watches for the same job: every poll costs the shared token budget, and a usage limit already stopped one run of this workflow. Make your commands print a clear end marker (for example  ; echo QTEST_DONE rc=$?  written AFTER the cargo pipeline, and check cargo's own exit with  set -o pipefail  or  \${PIPESTATUS[0]}  because a trailing grep/tail hides it).

HOW TO EDIT
- Many working-tree files use CRLF line endings; git normalises on commit. Do NOT rewrite whole files with the Write tool unless the file is new (new files: write them LF). For edits to existing files use the helper:  const {edit,sub}=require('${S}/lib-edit.js');  edit(path, s => sub(s, 'exact old text', 'new text'))  in a small node script (it preserves the file's EOL and fails loudly if the anchor is not unique). Prefer putting long Rust/JS text in a separate file and having the script read it. Heredocs with very long bodies fail in this Bash tool: write scripts with the Write tool, then run them.
- SHARED HOT FILES (other agents edit them too): crates/quarry-edge/src/{routes.rs,main.rs,lib.rs,state.rs}, crates/quarry-runtime/src/{pipeline.rs,lib.rs}, crates/quarry-browser/src/{lib.rs,chromiumoxide.rs}, any Cargo.toml, docs/RUNBOOKS.md, docs/ERROR_REASONS.md. Touch them only with small exact-anchor edits made right after re-reading the current text; never reformat them. Append-only for the two docs.
- Stay inside the "owned paths" of your unit. If you need a change outside them, make the smallest possible exact-anchor edit and say so in your report.
- Because the tree is shared, a build can break because ANOTHER agent is mid-edit in a file you do not own. If compile errors point at files outside your owned paths, do not touch them: wait a few minutes (re-run your qtest command; each re-run is queued anyway) up to 5 attempts, and only then report it.
- The log-field lint (crates/quarry-edge/tests/log_field_discipline.rs with tests/fixtures/log-field-baseline.json) fails if you add a tracing macro that logs a URL, query, title or raw error text. Fix your code to log counts/classes (zdr_log::class...), or add a one-line '// zdr-log-ok: <real reason>' comment where the value is provably not caller data. Do NOT regenerate the baseline unless your unit says so.

HOW TO COMMIT
- Write the commit message to a file under your scratch dir, then:  bash ${S}/tools/qcommit.sh <message-file> <path1> <path2> ...  It stages and commits ONLY those paths under a git lock (the pre-commit hook runs gitleaks via docker; that is normal). Commit in coherent steps (several commits are fine). Conventional Commits style: feat(scope): ..., fix(scope): ..., test(scope): ..., docs: ... . The body says what changed, what was verified (with counts), and a "Not done:" paragraph. End every message with EXACTLY this line, whatever model you are (it is the repository owner's attribution convention; do not write your own model name):
  Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>
- CONTINUATION RULE: at the start, look for ${NOTES}/wf-<your unit id>-progress.md. If it exists, an earlier agent of your unit was cut off: continue from it and from the git tree (git log, git status, git diff on your owned paths) instead of starting over.
- PROGRESS FILE: keep ${NOTES}/wf-<your unit id>-progress.md current after every milestone (what is committed, what is in progress, what is next, which uncommitted files are yours, the exact test commands and their results). If you are cut off, the next agent continues from that file and from the git tree.
- NOT OURS: apps/Model Plane/rust/services/model-gateway/src/grpc.rs and src/quarry.rs carry another session's in-flight diff (WS5.G) and .plan-work/ is not ours either: never stage or commit those paths. If your unit truly needs a change inside one of those two files, make a small exact-anchor edit but do not commit the file; list it in not_done.
- BEFORE you commit a file run git diff on it: if it contains hunks you did not make (another unit's uncommitted work), commit it only when those hunks are complete and compile, and say so in the message ("also carries X's hunk in ..."); otherwise leave the file and report it.
- Scratch directory for your temporary files: ${S}/<your unit id>/ (create it). Hand-over notes for the human go in ${NOTES}/wf-<unit id>.md (what you did, what is left, traps found).

YOUR FINAL ANSWER is a JSON object matching the schema you were given: status (done, partial or blocked), commits (short hashes), summary, verified (each entry = a command you ran and its result), not_done (each entry = something in scope you did not finish, and why), notes_file.
`

// --------------------------------------------------------------- unit prompts (round 2)
const LEFT = NOTES + '/round1-leftovers.md and ' + NOTES + '/round1-low-findings.md'
const UNITS = []

UNITS.push({
  id: 'R1-browser',
  tier: OPUS,
  lenses: ['correct', 'tests', 'concurrency', 'plan'],
  prompt: `UNIT R1-browser: finish the browser-driver work of slices WS3.F and WS3.D (a real flaky test, two known races, the /ready contract, two boot-time items).
CONTEXT. Round 1 shipped slots.rs, the driver caps, the idle reaper, the prober and /ready (commits 77749d34, b86a32c8, 5102b059, 37b7ebb0 and the review fixes c8c31a26, 60e32caf). Read ${LEFT} (search them for WS3.F, chromiumoxide, live_frame, do_fetch, ready) and ${NOTES}/wf-P1-ws3f.md, wf-P1-ws3f-design.md and wf-P1-ws3f-progress.md first.
OWNED PATHS: ${Q}/crates/quarry-browser/src/**, the BrowserDriverAdapter (find it: grep -rn 'struct BrowserDriverAdapter' ${Q}/crates) in quarry-runtime, ${Q}/crates/quarry-edge/tests/ready_browser.rs, the /ready handler region of crates/quarry-edge/src/routes.rs and the driver construction and boot-check region of crates/quarry-edge/src/main.rs (small exact-anchor edits; another unit, R5-zdr-logs, also edits main.rs), and in docs/openapi.yaml ONLY the /ready path (exact-anchor edit). Do not edit docs/RUNBOOKS.md or docs/ERROR_REASONS.md: unit R8b reconciles them at the end; if you change behaviour those docs describe, write the exact sentences to fix into ${NOTES}/wf-R1-browser-doc-fixes.md.
TASKS
1. FLAKY LIVE TESTS. In one of 8 full runs of the live lane, attach_timeout_cleans_up_partial_context and release_during_attach_leaves_no_page_or_context panicked at the helpers context_count / page_target_count: expect("the browser lock is free within 5 s"). Find the real cause with evidence (who holds the browser mutex for more than 5 s: the prober, a leaked attach, the reaper tick, a test that holds a guard?). Fix the code if it is a defect (a lock held across an await that can take long is the suspect); only if it is the test's own doing, fix the test and say why. Then run the whole live lane 10 times in ONE background qtest loop (live prefix from the preamble; features chromiumoxide) and report passes and failures of each run. Zero failures in 10 is the bar.
2. RACES. (a) invalidate_live_frame_cache / page_closed are keyed only by the page or session key: show whether a late page_closed for a reused key can invalidate a newer page's cache entry or whether a late frame insert can resurrect a released session's cache (ZDR relevance: frames must not outlive a session); fix with a generation or an owner check and add a test that fails without the fix. (b) BrowserDriverAdapter::do_fetch has no drop guard: a cancelled fetch (the HTTP request was dropped or timed out) must not leak the page, context or slot; add a guard and a test (a cancelled fetch releases everything: use the recording or fake driver of quarry-runtime tests).
3. WS3.F CONTRACT LEFTOVERS. (a) docs/openapi.yaml documents GET /ready as text/plain 'ready' but WS3.F made it JSON with a browser object and 503 browser_unhealthy: document the real response (200 and 503 bodies, the fields, the NotApplicable/Healthy/Unhealthy values) and add a quarry-edge test that decodes the real /ready body into a struct with exactly the documented keys; keep scripts/tests/openapi-route-coverage.test.mjs green (node --test in ${Q}). (b) R-26 boot check: read the plan ('R-26' and 'Slice WS3.F detail'): the session cap must exceed the concurrent-run cap plus the heavy lane; implement the check exactly as specified (warn or refuse as the text says) at boot, with a unit test of the pure function. (c) main.rs builds two ChromiumoxideDrivers (the scrape driver and the agent driver) but /ready reports only the agent driver: make /ready report the worst health of both, or if the plan says otherwise follow the plan and document it in the notes file.
TESTS. Through qtest: cargo test -p quarry-browser --features chromiumoxide (live prefix), cargo test -p quarry-edge --test ready_browser --test zdr_behavior, cargo test -p quarry-runtime --lib (the adapter), cargo check --workspace --all-targets, node --test scripts/tests/openapi-route-coverage.test.mjs scripts/tests/openapi-scrape-parity.test.mjs, and the log lint (cargo test -p quarry-edge --test log_field_discipline). Commit in steps: flaky fix; races; /ready contract and boot check.`,
})

UNITS.push({
  id: 'R2-agent-routes',
  tier: SONNET,
  lenses: ['correct', 'tests', 'security', 'plan'],
  prompt: `UNIT R2-agent-routes: turn the four recorded source findings of the control-authority tests into fixes, add an in-flight guard for tab calls, and add the route-level grant tests for start_run (leftovers of WS2.C, WS2.D and WS1.B).
CONTEXT. Read ${LEFT} (search for F5-1, F5-2, F5-3, F5-4, takeover, start_run, grant), the notes ${NOTES}/wf-F5-agent-run-tests.md and its progress file, then the ignored tests: grep -n '#\\[ignore' ${Q}/crates/quarry-edge/tests/agent_run_control.rs ${Q}/crates/quarry-edge/tests/run_sweeper_integration.rs (each ignored test documents one finding). The plan slices are WS2.C and WS2.D (grep -n 'Slice WS2.C detail') and WS1.B (the Quarry start_run part).
OWNED PATHS: ${Q}/crates/quarry-edge/src/agent_routes/** , ${Q}/crates/quarry-edge/tests/agent_run_control.rs, run_sweeper_integration.rs, tests/support/agent_harness.rs (and a new tests/start_run_grants.rs), crates/quarry-edge/src/run_limits.rs; and the tab-call handlers of ${WT}/apps/Frontend Plane/verevonv3/apps/gateway/src/domains/browser.rs (small exact-anchor edits; unit R4-ws1b-left edits other parts of that file).
TASKS
1. F5-1: control timeline items serialise initiated_by where the contract says initiatedBy (or the reverse: decide from the OpenAPI schema, the SPA client types in apps/Frontend Plane/verevonv3/src/shared/api/browser-client.ts and the gateway, and report what you found); make the wire consistent, update the schema if needed, un-ignore the test.
2. F5-2: a resumed agent run is swept with the interactive class instead of the agent class: fix, un-ignore.
3. F5-3: a step or tab call queued behind an expiry gets run_not_live without resumable: fix, un-ignore.
4. F5-4 (HIGH): tab select and close take no initiator, so an agent's call made with a person's token is recorded as Human during a take-over and is not refused. Read how step determines initiated_by (token kind, a header, a body field) and give the tab select and close routes the same rule; the gateway must forward what Quarry needs without letting a browser client forge it (read the gateway's browser.rs tab handlers and how its step route forwards the initiator; follow that pattern). Un-ignore the test, add a gateway test for the forwarding.
5. Tab calls have no in-flight guard: give them the same guard the step path has (a second concurrent tab call on a run is refused or queued as the step path does) with a test.
6. Route-level start_run grant tests (WS1.B leftover): commit 6216f19f made start_run bind a broker grant to its owner, ZDR posture and hosts. Add tests/start_run_grants.rs through the real router with a fake grant validator: user_cannot_use_another_users_grant, zdr_grant_cannot_back_a_non_zdr_run, validated_grant_overrides_client_domains_in_every_mode (find the validator trait and how enforcement modes are set: grep -rn 'GrantValidator' crates/quarry-edge/src).
TESTS. Through qtest: cargo test -p quarry-edge (default features include browser-agent) --lib agent_routes and --test agent_run_control --test run_sweeper_integration --test start_run_grants plus the full quarry-edge suite at the end; gateway: cd '${WT}/apps/Frontend Plane/verevonv3/apps/gateway' && cargo test (native; note the gateway tree carries four other uncommitted WS5.G files that are not yours: do not stage them). Where a fix changes the OpenAPI file or an SDK doc, also run node --test scripts/tests/openapi-route-coverage.test.mjs. Commit in steps, one finding per commit where practical (fix and its un-ignored test together).`,
})

UNITS.push({
  id: 'R3-ws1c',
  maxLenses: 4,
  done: {"status":"done","commits":["26f1c679","7c5a564b","669fde75"],"summary":"WS1.C, the Postgres grant store of browser-broker, committed in three steps: migration 0001_browser_grants, PgStore (Derive in one transaction with a per-parent advisory lock, RevokeScoped locking the root FOR UPDATE then a recursive update, exact per-owner session cap), Routed{persistent, ephemeral} (ZDR ids never reach Postgres), main.go (DATABASE_URL required unless BROWSER_BROKER_ALLOW_EPHEMERAL_DEVELOPMENT, /readyz pings the database, 10-minute purge loop), Dockerfile and entrypoint applying the migrations. Behaviour change behind Routed: a ZDR-raised sensitive approval on a non-ZDR parent is refused (ErrZdrPostureChanged). Hand-over: wf-R3-ws1c.md, design wf-R3-ws1c-design.md, compose lines wf-R3-ws1c-compose.md.","verified":["GOWORK=off go vet ./... and go vet -tags integration ./... clean","GOWORK=off go test -count=1 ./... ok (authz, grant, server; TestDeriveContract_Memory 16 subtests, 4 TestRouted_*)","integration under with-lock.sh with testcontainers postgres:16-alpine: rc=0, 70 PASS lines, 0 FAIL, all 10 named TestPg_* tests","mutation 1: no per-parent advisory lock in Derive -> TestPg_PerOwnerCapIsExactUnderConcurrency FAILS","mutation 2: no FOR UPDATE root lock in RevokeScoped -> TestPg_DeriveRejectsRevokedParentRace FAILS in 0.9 s (\"run grant outlived its revoked parent\"); restored, suite green"],"not_done":["Compose wiring (DATABASE_URL and depends_on postgres in apps/Model Plane/deploy/docker-compose.yml): outside the owned paths, exact lines in wf-R3-ws1c-compose.md. A broker image rebuilt from 7c5a564b without them exits 1 at boot.",".github/workflows/browser-broker-ci.yml integration job not written.","No deployment evidence (image not built, broker not restarted, L2 legs not run).","Dedicated database and least-privilege role: hardening follow-up."],"notes_file":"C:/Users/ImaFernandesDaCosta/AppData/Roaming/Claude/side-session-notes/local_6a0cd694-160f-4e6d-9b45-da7a2400e1ec/wf-R3-ws1c.md"},
  tier: OPUS,
  lenses: ['correct', 'tests', 'security', 'concurrency', 'plan'],
  prompt: `UNIT R3-ws1c: slice WS1.C, the Postgres grant store of the browser broker.
OWNED PATHS: ${WT}/apps/Model Plane/go/services/browser-broker/** (migrations, internal/grant/postgres.go and router.go, cmd/main.go, internal/telemetry/metrics.go, docker-entrypoint.sh, Dockerfile, go.mod, go.sum, tests). Do not edit compose files or the Control Plane; if the slice needs env in a compose file, write the exact lines to ${NOTES}/wf-R3-ws1c-compose.md.
FIRST read the plan's 'Slice WS1.C detail' in full (grep -n 'Slice WS1.C detail' ${PLAN}; it is one long paragraph), the decision records X-2 and R-3 it cites, the WS1.B commits (0997b7c8, 73ad8767) and ${NOTES}/wf-P2-ws1b-design.md for what Derive guarantees. Write a design note (about 500 words: schema, transaction and locking plan for Derive, per-parent run cap of 8 under concurrency, revoke subtree, routing by id prefix, restart semantics table, what you will not do) to ${NOTES}/wf-R3-ws1c-design.md before coding.
SPEC SUMMARY (the plan has the exact text): migration 0001_browser_grants up and down (every statement IF NOT EXISTS, no zdr column, CHECKs as listed); PgStore (Derive in a transaction with SELECT ... FOR SHARE on the parent and ON CONFLICT DO NOTHING, RevokeScoped as one WITH RECURSIVE statement, advisory lock for an exact cap, PurgeExpired); Routed{persistent, ephemeral} routes creates by ZDR decision and Get/Revoke/Derive by id prefix so a z_ id never reaches Postgres; main.go reads DATABASE_URL (empty and BROWSER_BROKER_ALLOW_EPHEMERAL_DEVELOPMENT not true exits 1), purge loop, /readyz pings the database; GrantStoreErrorsTotal{op} without domain attributes; Dockerfile gets postgresql-client, migrations and an entrypoint copied from sandbox-manager (read apps/Model Plane/go/services/sandbox-manager/docker-entrypoint.sh and cmd/main.go).
DEPENDENCIES. github.com/jackc/pgx/v5 and, for tests, testcontainers-go with its postgres module: add them with GOWORK=off go get <module>@<version from the plan> run inside the broker directory (there is no go.work: build per service; commit go.mod and go.sum). Integration tests carry the build tag integration and use Docker: ANY docker use (testcontainers included) must run under  bash ${S}/tools/with-lock.sh docker 2700 -- <command>  so it never overlaps another container. If testcontainers cannot run on this Windows Docker Desktop (it needs the Docker socket and Ryuk), fall back to a throwaway postgres:16-alpine container on a private network started by a script in your scratch dir (see ${S}/tools/pg-test.sh for the pattern) and point the integration tests at it through an env var (TEST_DATABASE_URL) while keeping the tagged tests runnable with testcontainers where possible; say which you used.
TESTS (names in the plan): unit TestRouted_ZdrNeverTouchesPersistentRepo, TestRouted_ZPrefixIdNeverReachesPostgres, TestRouted_DeriveRoutesByParentPrefix; integration TestPg_CreateGetExpireRevoke, TestPg_RevokeCascadesWholeSubtree, TestPg_DeriveIsIdempotentUnderConcurrency, TestPg_DeriveRejectsRevokedParentRace, TestPg_PerOwnerCapIsExactUnderConcurrency (the per-parent run cap of 8 from WS1.B review fix 73ad8767: the plan text says per owner; reconcile the two explicitly: the WS1.B review decided runs are bounded per session; keep that and document it), TestPg_OwnerAndOrgScopingReturnNotFound, TestPg_ZdrCreateWritesZeroRows, TestPg_MigrationIsRerunnable (apply the up file twice), TestPg_SurvivesNewRepositoryInstanceOnSameDatabase, TestPg_PurgeRemovesExpiredAndCascadesChildren. The in-memory store tests of WS1.B (derive_test.go) must stay green and the Postgres store must pass the same Derive semantic cases (reuse them as a shared contract suite run against both stores if you can). Run: cd '${WT}/apps/Model Plane/go/services/browser-broker' && go vet ./... && go test -count=1 ./... and the tagged integration run.
Do NOT rebuild or restart the running model-plane-browser-broker container and do not touch the live Model Plane stack. Commit in steps (migration + PgStore; router + main wiring + Dockerfile; tests).`,
})

UNITS.push({
  id: 'R4-ws1b-left',
  tier: SONNET,
  lenses: ['correct', 'tests', 'security', 'plan'],
  prompt: `UNIT R4-ws1b-left: the remaining buildable parts of slice WS1.B plus the gateway mapping for the new WS3.F error codes.
CONTEXT. Read ${LEFT} (search for WS1.B, GrpcGrantValidator, idempotency, L1, renewal, browser_session_cap), ${NOTES}/wf-P2-ws1b.md, wf-P2-ws1b-design.md, wf-P2-ws1b-progress.md and the plan's 'Slice WS1.B detail'.
OWNED PATHS: ${WT}/apps/Model Plane/rust/services/model-gateway/src/browser_run.rs, browser_grants.rs and their tests; the L1 contract test location named in the plan (find it); ${WT}/apps/Frontend Plane/verevonv3/apps/gateway/src/domains/browser.rs and src/config.rs (unit R2-agent-routes edits the tab-call handlers of browser.rs: exact-anchor edits only, re-read before editing); the legacy validator in quarry-runtime or quarry-edge (find: grep -rn 'GrpcGrantValidator' ${Q}/crates). NOT YOURS: apps/Frontend Plane/verevonv3/src/shared/i18n/errors.ts, src/shared/api/knowledge-client.ts and gateway domains/knowledge/quarry.rs and search.rs (WS5.G's uncommitted hunks: never stage them), model-gateway grpc.rs and quarry.rs, the Control Plane.
TASKS
1. L1 contract test: pin the DeriveRunGrant request and response field numbers (the proto is apps/Model Plane/proto/model_plane/v1/browser.proto) and the gateway-side assertions the plan lists for L1; find where the existing L1 contract tests live (git ls-files | grep -i 'contract' and the plan) and extend them in the same style. If no L1 harness exists for this, write the smallest test that reads the generated Go descriptor and asserts the numbers, and say so.
2. model-gateway: a test proving a derive failure creates no thread and no run (the grant is resolved before prepare_managed_run_authenticated): drive the real handler with the test broker double returning an error from DeriveRunGrant and assert no thread/run was prepared.
3. Delete the legacy GrpcGrantValidator::acquire (and any dead code only it used) if grep shows no caller outside tests; update tests.
4. Client idempotency key on POST /v1/browser/runs: read the plan text for the intended header name and semantics (WS1.B 'run_key', 'Idempotency-Key'); implement in the gateway and model-gateway as specified so a client retry returns the same run and the same derived grant; tests for both.
5. Gateway mapping for the WS3.F driver error codes browser_session_cap, browser_page_cap, session_reaped, browser_process_lost and /ready's browser_unhealthy: find how domains/browser.rs maps other Quarry error codes to the SPA (and which retry hints), add the mapping with tests. The SPA i18n entry is NOT yours (errors.ts is held by WS5.G): list the codes in your notes file for the SPA owner.
6. Session-grant renewal: Quarry has no endpoint to swap a live run's grant id, so a session grant that expires ends the run. Write a short design note with options (renewal endpoint vs TTL policy vs fail clearly) to ${NOTES}/wf-R4-grant-renewal-design.md; do not implement.
7. Write into ${NOTES}/wf-R4-ws1b-left-compose.md the exact compose env lines needed for BROWSER_BROKER_MAX_RUN_DOMAINS and BROWSER_GRANT_ENFORCEMENT in the Model Plane and Frontend compose files (do not edit compose files).
TESTS. Broker: cd '${WT}/apps/Model Plane/go/services/browser-broker' && go test -count=1 ./... (another unit, R3-ws1c, works in that directory: do not edit it). model-gateway: the custom runner of the integrate agent: read ${S}/INTEGRATE/mp-test.sh and wf-INTEGRATE notes (the image lacks protobuf includes: it installs libprotobuf-dev and mounts the whole Model Plane folder and the Frontend Plane tree), run cargo test -p model-gateway --lib under the docker lock; expect exactly one known failure (zdr_precheck::the_zdr_stream_forwards_its_route_instead_of_dropping_it, a CRLF artifact of this Windows working copy; do not chase it). Gateway: native cargo test in the gateway directory. Commit in steps.`,
})

UNITS.push({
  id: 'R5-zdr-logs',
  tier: SONNET,
  lenses: ['correct', 'tests', 'plan'],
  prompt: `UNIT R5-zdr-logs: the open items of WS4.E, WS4.F and WS4.G.
CONTEXT. Read ${LEFT} (search for WS4, telemetry, EnvFilter, log lint, page_renderer, canary, catalogue), ${NOTES}/wf-F6-error-classes.md and progress, wf-F2-gateway-zdr-log*.md, wf-F3-matrix-gap-tests-progress.md, and the plan's 'Slice WS4.E detail', 'Slice WS4.F detail' and 'Slice WS4.G detail'.
OWNED PATHS: ${Q}/crates/quarry-edge/src/telemetry.rs, zdr_log.rs, the log-macro sites it must change (exact-anchor edits; main.rs is shared with unit R1-browser), tests/log_field_discipline.rs, tests/fixtures/log-field-baseline.json (shrink only), tests/zdr_log_capture.rs, tests/support/zdr_harness.rs, tests/zdr_feature_matrix.rs, tests/zdr_gap_rows.rs; ${Q}/crates/quarry-core/src/zdr_matrix.rs with docs/ZDR_MATRIX.md and crates/quarry-runtime/contracts/zdr-feature-matrix.json(.sha256) (regenerate with QUARRY_UPDATE_GOLDEN=1); crates/quarry-runtime/src/page_renderer.rs (the ZDR-reachable log site only); and the gateway's src/zdr_log.rs and tests/log_field_discipline.rs under ${WT}/apps/Frontend Plane/verevonv3/apps/gateway (not its other files).
TASKS
1. Quarry telemetry.rs: round 1 found and fixed in the gateway (commit d8a9708d) that the ZDR mark and caps depended on RUST_LOG because the EnvFilter sat at registry level. Check quarry-edge telemetry.rs for the same weakness (does a RUST_LOG=off or a per-target filter hide the mark layer or lift the third-party caps?) and fix with a test that fails without the fix.
2. The Quarry log lint stops scanning crates/quarry-edge/src/main.rs at line 64 (it treats something as the start of test code; about 46 macro sites in the rest of the file, with url fields near lines 531, 592 and 615, are unscanned). Fix the scanner (production code ends at the first #[cfg(test)] mod, not at an earlier cfg(test) item) with a scanner unit test, then deal with every newly visible site: convert to counts or classes, or add '// zdr-log-ok: <reason>' only where the value is provably not caller data. The baseline may only shrink: if it would grow, you must convert the sites instead.
3. crates/quarry-runtime/src/page_renderer.rs near line 191 logs on a path reachable under ZDR (render_zdr) and was annotated rather than converted: convert it to a class or count, or prove with a test that the event is dropped for ZDR requests.
4. WS4.G tests that exist only in the plan: every_row_has_a_log_canary_or_is_marked_log_unverified and every_code_and_feature_the_matrix_names_is_in_the_closed_catalogue_list (read the WS4.G Tests paragraph for their exact meaning; the closed catalogue is the Model Plane zdr-refusals catalogue or the list in the plan: find it; if the catalogue lives in a sibling's tree that is not committed, implement against the plan's list and say so).
5. Remaining R1/R2/R4 baseline sites in quarry-runtime (15 R2, 26 R1) and the gateway (22 R2, 1 R4): convert the ones on request paths of scrape, search, extract, answer and agent steps; leave background or boot sites annotated with a true reason. Report the baseline numbers before and after (it must go down, not up).
TESTS. Through qtest: cargo test -p quarry-edge --test log_field_discipline --test zdr_log_capture --test zdr_feature_matrix --test zdr_gap_rows --test zdr_claim_floor, cargo test -p quarry-core zdr_matrix, cargo test -p quarry-runtime --lib, cargo check --workspace --all-targets; gateway: native cargo test in its directory (four WS5.G files there are not yours). Commit in steps.`,
})

UNITS.push({
  id: 'R6-challenge',
  tier: SONNET,
  lenses: ['correct', 'tests', 'plan'],
  prompt: `UNIT R6-challenge: the open items of WS3.H and the Quarry-owned half of WS3.J.
CONTEXT. Read ${LEFT} (search for WS3.H, WS3.J, challenge, reject_if_exhausted, QUARRY_CHALLENGE), ${NOTES}/wf-P3-ws3h.md and progress, and the plan's 'Slice WS3.H detail' and 'Slice WS3.J detail' (the Quarry half is the sentence about calling challenge::classify on the scrape path and supplying a block_reason; the Model Plane, SPA and planner parts belong to sibling sessions and are NOT yours).
OWNED PATHS: ${Q}/crates/quarry-runtime/src/challenge.rs, observation.rs, host_scheduler.rs, the scrape path of crates/quarry-runtime (PageRunner: grep -rn 'struct PageRunner' ${Q}/crates), crates/quarry-core/src/contracts.rs and tests/contracts.rs, error.rs (challenge constructor only), docs/openapi.yaml (the challenge schemas and the scrape error details only; exact-anchor edits), docs/SOVEREIGN_SCRAPING_RELIABILITY.md, scripts/tests/openapi-challenge-parity.test.mjs (new), sdks/typescript/docs/ChallengeSignal.md (regenerate, do not hand edit), docs/RUNBOOKS.md (append-only). NOT YOURS: gateway domains/knowledge/quarry.rs (WS5.G hunks), Model Plane files.
TASKS
1. RUNBOOKS: add the QUARRY_CHALLENGE_* entries (CLASSIFIER v2|legacy, ENFORCE) as an appended section: defaults, effect, how to roll back.
2. QUARRY_CHALLENGE_ENFORCE: the code defaults to enforce while the plan text says log during corpus validation. Read the plan and the ADR it cites. If the plan is explicit, align the code default and the tests with the plan; if it leaves the default open, do NOT flip it: keep the code, and write the open decision (options, consequence of each) to ${NOTES}/wf-R6-challenge-decision.md.
3. reject_if_exhausted still lets Evaluate, MouseWheel, RespondDialog and Back through on an exhausted host. Decide from the spec what 'one recovery then refuse' must cover (the point is to stop repeated attempts against a wall without blocking the person's legitimate take-over or passive actions); implement what the spec says, with table tests per action, and write the reasoning into the commit message. Do not widen refusal beyond the spec.
4. An OpenAPI parity test (scripts/tests/openapi-challenge-parity.test.mjs, same style as scripts/tests/openapi-scrape-parity.test.mjs) that the enum values of ChallengeKind, ChallengeStrength, BlockReason and ChallengeRecovery in docs/openapi.yaml equal the Rust enums (derive the Rust values from a golden generated by a quarry-core test with UPDATE env, the way scrape_schema.rs does, or from the jsonschema feature). Add the CI step to .github/workflows/quarry-v2-ci.yml next to the existing openapi steps (small exact-anchor edit; unit R8a-ci edits the same file).
5. Regenerate sdks/typescript/docs/ChallengeSignal.md with sdks/generate.sh the way round 1 did (read ${NOTES}/wf-F1-docs.md and the scripts in ${S}/F1-docs/ for the recipe: docker under the lock, deterministic, only commit the pages that changed because of the challenge schemas).
6. WS3.J Quarry half: where the scrape path (PageRunner) sees a block, call challenge::classify and expose ONLY the typed result the spec names (a strong block_reason in the error details of the scrape failure; never page text or URLs; ZDR runs count by reason and never log the host). Keep it additive and behind QUARRY_CHALLENGE_CLASSIFIER. If the spec leaves the exact field open, choose the smallest additive shape, document it in openapi.yaml and CONTRACTS.md (9.x), pin it in the scrape schema golden if it is part of a request/response type, and note it for the gateway owner in ${NOTES}/wf-R6-challenge-gateway-contract.md.
TESTS. Through qtest: cargo test -p quarry-runtime challenge observation host_scheduler, cargo test -p quarry-core --test contracts and (features jsonschema) the scrape_schema test if you touch types, cargo test -p quarry-edge, cargo check --workspace --all-targets, the log lint; node --test scripts/tests/openapi-*.test.mjs. Commit in steps.`,
})

UNITS.push({
  id: 'R7-transform-store',
  tier: SONNET,
  lenses: ['correct', 'tests', 'plan'],
  prompt: `UNIT R7-transform-store: three defects the rebuilt-edge evidence run found (slices WS5.E, WS5.F, WS5.C).
CONTEXT. Read ${NOTES}/wf-F7-evidence.md in full first (the evidence run at 89c67974) and the 'Evidence' section at the end of ${Q}/docs/RUNBOOKS.md, then ${LEFT} (search for padded, FilesystemStore, flate, 504).
OWNED PATHS: ${Q}/crates/quarry-transform/** (markdown rendering and its tests), ${Q}/crates/quarry-runtime/src/artifact_store.rs (find the FilesystemStore: grep -rn 'FilesystemStore' ${Q}/crates) with its tests, ${Q}/crates/quarry-runtime/src/content_route.rs and the document parser child (src/doc_parse_main.rs, tests/doc_parse_sandbox.rs), crates/quarry-edge/tests/scrape_records_durable.rs and a new tests/artifacts_fs_list.rs. docs/RUNBOOKS.md append-only.
TASKS
1. MARKDOWN TABLE PADDING. A 1.05 MB Wikipedia page produced 30.9 MB of markdown with only_main_content=false because table lines are padded to over 1M characters. Find the cause (the markdown library's table rendering pads every row to the widest cell: a single huge cell makes every row huge). Reproduce it with a synthetic fixture first (a table with one very wide cell and many rows), then fix: compact tables (no padding to column width) or a cap on padding, whichever keeps existing golden tests and the semantic content intact. Add a regression test asserting the output size stays within a small multiple of the input size for a pathological table and that normal tables still render. Check clean_html's other outputs for the same blow-up (nested lists, pre blocks) with one more pathological fixture, and report.
2. FilesystemStore::list. The filesystem artifact store does not override list, so GET /v1/artifacts is always empty on the durable fs backend. Read the ArtifactStore trait and the in-memory store's list semantics (org scoping, ordering, limit and cursor, ZDR: nothing stored), implement the same for the filesystem store (walk the directory layout it already writes; the org id is part of the key: confirm), with tests including: records written, store dropped, a new store on the same directory lists them; another organization sees none; ordering and limit match the memory store (run the same contract test function against both stores).
3. FLATE BOMB RETURNS 504, NOT 415. In the evidence run a flate-bomb PDF through the real edge image hit the 15 s deadline (504) instead of a typed 415 pdf_parse_failed: the parser inflates slowly enough that the deadline fires before the 768 MiB RLIMIT_AS does. Without weakening the limits or the deadline: add an inflate budget so the parser fails fast and typed (for example a cap on the total decompressed bytes lopdf may produce, by running a bounded pre-scan of /FlateDecode streams or by using lopdf's own limits if it has them: check the version in Cargo.lock), return 415 pdf_parse_failed with details.reason pdf_decompression_limit (or the closest existing reason in docs/ERROR_REASONS.md: reuse, do not invent a new code), and add a test with the DEFAULT child limits (mark it #[ignore] if it takes more than 10 s and say how to run it). Keep the existing doc_parse_sandbox tests green and add a normal PDF case that still parses.
TESTS. Through qtest: cargo test -p quarry-transform, cargo test -p quarry-runtime --lib --test doc_parse_sandbox, cargo test -p quarry-edge --test scrape_records_durable --test artifacts_fs_list, cargo check --workspace --all-targets, the log lint. Commit in steps (one defect per commit).`,
})

UNITS.push({
  id: 'R8a-ci',
  tier: HAIKU,
  lenses: ['correct', 'tests'],
  rounds: 1,
  prompt: `UNIT R8a-ci: CI coverage for the Redis-backed cache gates and the stale progress rows.
OWNED PATHS: ${WT}/.github/workflows/quarry-v2-ci.yml (small exact-anchor edits: units R1 and R6 may add a step each), ${Q}/docs/PROGRESS.md (find it: git ls-files | grep -i progress), ${Q}/docs/TESTS*.md if a tests snapshot exists (commit dedf5f51 mentions one).
TASKS
1. CI never runs the page-cache and ZDR-cache gates: quarry-v2-ci.yml has no Redis service and does not set REQUIRE_CONTRACT, so crates/quarry-edge/tests/cache_policy.rs (9 cases) and the Redis case of tests/zdr_claim_floor.rs skip. Read both tests to find the exact env names (REQUIRE_CONTRACT, the Redis URL variable and its default address), add a Redis service container to the right job(s) of the workflow, set REQUIRE_CONTRACT=1 and the URL for the test step(s) that run those binaries, and make sure the job that runs them runs the edge integration tests at all (read the job). Validate the YAML: parse it with node (the js-yaml package may not be installed: use python -c 'import yaml' if python exists, else a small structural check) and, if you can, run actionlint from its docker image under the docker lock (bash ${S}/tools/with-lock.sh docker 2700 -- docker run --rm -v ...). Prove the gate works locally: start a throwaway Redis container under the lock on a private network, run cargo test -p quarry-edge --test cache_policy --test zdr_claim_floor with REQUIRE_CONTRACT=1 and the URL through qtest (the qtest container must reach the redis container: use --network via a custom docker run modelled on ${S}/tools/pg-test.sh), and then run once WITHOUT Redis and REQUIRE_CONTRACT=1 to see the tests FAIL (that is the point of the flag); report both.
2. PROGRESS.md and any tests snapshot: correct rows that are stale after round 1: cache_policy (9 cases, skip behaviour and REQUIRE_CONTRACT), the new test binaries (agent_run_control, run_sweeper_integration, scrape_records_durable, zdr_gap_rows, zdr_feature_matrix, ready_browser, log_field_discipline, clean_html_selectors, clean_html_cost), counts from a real run (cargo test -p quarry-edge ... --no-run is not enough: use the numbers printed by the runs), the gateway lint. Every number you write must come from a command you ran; say so in the commit message.
Commit in two steps.`,
})

// ------------------------------------------------------------ review machinery
const LENS = {
  correct: {
    ...SONNET,
    text: 'CORRECTNESS. Read the diff of the commits (git show <hash> for each) and the code around it. Look for logic errors, regressions of existing behaviour, concurrency and locking mistakes, error handling gaps, tenant-isolation or ZDR leaks (anything that could store, log or return caller data it should not), unsafe edge cases, and shared-file edits that could break other units. Run the unit tests of the touched crates through qtest where a doubt can be settled by running.',
  },
  tests: {
    ...HAIKU,
    text: 'TEST QUALITY. For every new or changed test: would it FAIL if the behaviour regressed (look for vacuous assertions, tests that skip without an environment variable and so never run in CI, wrong names, copy-paste duplicates)? Check every claim in the commit messages (counts, "checked", "verified") against what you can reproduce by running the named tests through qtest. List claims that are not true.',
  },
  plan: {
    ...SONNET,
    text: 'PLAN CONFORMANCE. Compare the implementation with the plan slice it implements (the Slice detail paragraph in docs/plans/quarry-browser-build-plan-2026-10-14.md and the decision records it cites). List spec items that are missing, contradicted or weakened, and contract/field names that differ from the spec.',
  },
  concurrency: {
    ...OPUS,
    text: 'CONCURRENCY AND FAILURE MODES. Think adversarially about locking order, deadlocks, cancellation safety (futures dropped mid-way), resource leaks (pages, contexts, processes, files), races between reaper, prober, acquire and release, and what happens when Chromium dies at the worst moment. Cite code lines.',
  },
  security: {
    ...OPUS,
    text: 'SECURITY. Try to break the derived-grant rules: widening domains, extending lifetime, flipping ZDR posture, cross-tenant parents, replay with different domains, confused-deputy through the service principal, TOCTOU between parent check and insert. Check authz.go method rules and that errors do not leak other tenants\' data. Cite code lines and propose a failing test for each real hole.',
  },
}

function reviewPrompt(u, impl, lensName) {
  return `${PRE}

YOU ARE A REVIEWER. You do not edit repository files and you do not commit. You may create scratch files under ${S}/review-${u.id}/ and run tests through qtest/native cargo/go.
Unit under review: ${u.id}. The implementer reported: status ${impl.status}; commits ${JSON.stringify(impl.commits)}; summary: ${impl.summary}; not done: ${JSON.stringify(impl.not_done)}.
Review the committed work (git show --stat <hash>, git show <hash>) and, if the implementer left uncommitted changes in its owned paths, those too (git diff on those paths only).
YOUR LENS: ${LENS[lensName].text}
Known and accepted, do NOT report: the Co-Authored-By trailer naming Claude Haiku 5.5 on some earlier commits, LF/CRLF warnings, the uncommitted WS5.G files of another session, findings already listed as open in ${NOTES}/round1-leftovers.md that are not in this unit's scope. Report findings only when you have EVIDENCE (a file and line, a failing command, a contradiction you can quote). severity high = breaks behaviour, security or data handling, or a claim in a commit message is false; medium = a real defect or an untested claimed behaviour; low = minor. verdict approve only if there is no high or medium finding. Answer with the JSON object of your schema.`
}

function fixPrompt(u, impl, findings) {
  return `${PRE}

You are the implementer of unit ${u.id} again. (${ROUND3}) Reviewers found problems with your work. Fix every HIGH and MEDIUM finding below that is real (if one is wrong, say why in your summary and show the evidence). Stay inside your owned paths as stated in the unit description, re-run the relevant tests through qtest/native tools, and commit with qcommit.sh (new commits; never amend). Your earlier commits: ${JSON.stringify(impl.commits)}.
Original unit description:
${u.prompt}

FINDINGS:
${JSON.stringify(findings, null, 2)}`
}

function verifyPrompt(u, findings, fixCommits) {
  return `${PRE}

YOU ARE A VERIFIER (read-only for repository files; you may run tests). Unit ${u.id}: reviewers raised the findings below and the implementer says it fixed them in ${JSON.stringify(fixCommits)}. For each finding decide whether it is now closed (evidence: the code or a test run) or still open, and report any NEW problem the fix introduced.
FINDINGS:
${JSON.stringify(findings, null, 2)}`
}

// ---------------------------------------------------------------- unit runner
async function runUnit(u) {
  const impl = u.done ? u.done : await A(`${PRE}\n${ROUND3}\n${u.prompt}`, { label: 'impl:' + u.id, phase: 'Implement', schema: IMPL, ...u.tier })
  if (!impl) { log(u.id + ': implementer returned nothing'); return { unit: u.id, impl: null } }
  log(u.id + ': ' + impl.status + ', ' + impl.commits.length + ' commit(s)')
  if (impl.status === 'blocked' || impl.commits.length === 0) return { unit: u.id, impl, reviews: [], fixes: [] }

  let reviews = (await parallel(u.lenses.slice(0, u.maxLenses || 3).map((l) => () =>
    A(reviewPrompt(u, impl, l), { label: 'review:' + u.id + ':' + l, phase: 'Review', schema: REVIEW, model: LENS[l].model, effort: LENS[l].effort })
  ))).filter(Boolean)
  let open = reviews.flatMap((r) => r.findings.filter((f) => f.severity !== 'low'))
  const fixes = []
  for (let round = 1; round <= (u.rounds || 2) && open.length > 0; round++) {
    log(u.id + ': round ' + round + ', ' + open.length + ' finding(s) to fix')
    const fix = await A(fixPrompt(u, impl, open), { label: 'fix:' + u.id + ':' + round, phase: 'Fix', schema: IMPL, ...u.tier })
    if (!fix) break
    fixes.push(fix)
    const verdict = await A(verifyPrompt(u, open, fix.commits), { label: 'verify:' + u.id + ':' + round, phase: 'Fix', schema: VERIFY, ...SONNET })
    if (!verdict) break
    open = open.filter((f) => verdict.still_open.some((s) => s.includes(f.issue.slice(0, 40)) || s.includes(f.file)))
    const realNew = verdict.new_problems.filter((p) => !/^(none|no new|nothing|n\/a)\b/i.test(String(p).trim()))
    if (realNew.length) open.push(...realNew.map((p) => ({ severity: 'medium', file: u.id, issue: p, evidence: 'reported by the verifier' })))
  }
  const low = reviews.flatMap((r) => r.findings.filter((f) => f.severity === 'low'))
  return { unit: u.id, impl, reviews: reviews.map((r) => r.verdict), unresolved: open, low, fixes }
}

// ================================================================ STAGE 1
phase('Implement')
const stage1 = await Promise.all(UNITS.map((u) => runUnit(u).catch((e) => ({ unit: u.id, error: String(e) }))))
const stage1Dead = stage1.filter((r) => !r.impl).length
log('stage 1 finished: ' + stage1.map((r) => r.unit + '=' + (r.impl ? r.impl.status : (r.error ? 'error' : 'no-result'))).join(', '))
if (stage1Dead >= 4) {
  log('STOPPING: ' + stage1Dead + ' implementers returned nothing (most likely a usage limit). Re-run this workflow later: each unit continues from its progress file and the tree.')
  return { stopped: true, stage1 }
}

// ================================================================ STAGE 2: reconcile the docs with the final state
phase('Stage 2')
const R8B = {
  id: 'R8b-doc-reconcile',
  tier: SONNET,
  lenses: ['correct', 'tests'],
  rounds: 1,
  prompt: `UNIT R8b-doc-reconcile: you run alone now (all other units of this round are finished and committed). Make the documents tell the truth about the final state.
OWNED PATHS: ${WT}/${PLAN}, ${Q}/docs/RUNBOOKS.md, ${Q}/docs/ERROR_REASONS.md, ${Q}/docs/PROGRESS.md (only rows about the units of this round), ${WT}/apps/STALE_DOC_DELETION_REGISTER.md.
INPUTS. git log --oneline 39386dc5..HEAD (about 100 commits across two rounds; read the messages: several are errata of earlier ones), ${NOTES}/round1-leftovers.md, ${NOTES}/wf-*.md of the units of both rounds (R1-browser, R2-agent-routes, R3-ws1c, R4-ws1b-left, R5-zdr-logs, R6-challenge, R7-transform-store, R8a-ci), and the doc-fix lists the units left (${NOTES}/wf-R1-browser-doc-fixes.md if present).
TASKS
1. The build ledger of ${PLAN} (the table near the top, about lines 14 to 60, and the table of files that exist only as uncommitted work): it has one row for WS1.B and nothing for the rest. Add accurate rows for every slice built on this branch (WS2.B to WS2.E, WS3.B to WS3.F, WS3.H, WS4.B to WS4.G, WS5.C to WS5.H, WS1.B, WS1.C if committed, the WS3.J Quarry half if committed) with the commit hashes, what is verified (counts from the notes or from a command you run), and what is open; update the 'uncommitted work' table to the real baseline (git status --short shows what is still uncommitted: only WS5.G files of another session and .plan-work should remain). Keep the document's existing style and do not rewrite plan text that is not a ledger. Fix the one plan line that names the old test a_service_principal_with_body_zdr_gets_the_same_refusals (split by 264832dd): grep for the real names in crates/quarry-edge/tests.
2. ERROR_REASONS.md line about health_probe (around line 43) still lists it as a value of the 504 op field while line 39 says no response body carries it: make them consistent with the code (crates/quarry-browser deadlines.rs, crates/quarry-core/src/error.rs).
3. RUNBOOKS.md: delete or correct statements round 1 or 2 made stale: the 'Known gap: a tab attach still holds the driver's browser lock' sentence (fixed by WS3.F), the 'Correction to c2b3d2e9' section (mark superseded once the cleaning-cost and cache-bypass sections it corrects exist: read it), anything the R1/R2/R5/R6/R7 notes say changed (flaky fix, /ready body, challenge env, markdown padding, fs list, flate budget). Append-only means: do not delete other units' valid text; minimal exact-anchor edits to stale sentences are allowed.
4. Commit-message errata. Commits cannot be amended. Add a short 'Errata' subsection to the build ledger listing the claims in commit messages that were false or inaccurate, from ${NOTES}/round1-leftovers.md (section 'unconfirmed_claims') and any you find in round 2 commits (verify each against git show before listing it), one line each: hash, what it claimed, what is true.
5. Register: add rows to apps/STALE_DOC_DELETION_REGISTER.md for any document statement deleted or replaced by this round.
Every statement must be true of the code or git history today: check by reading code, running grep and git show. Commit in steps with qcommit.sh.`,
}
const stage2 = await runUnit(R8B).catch((e) => ({ unit: R8B.id, error: String(e) }))

// ================================================================ STAGE 3: evidence from a rebuilt edge
phase('Evidence')
const evidence = await A(`${PRE}
UNIT R9-evidence: rebuilt-edge evidence for round 2 (you run alone; nobody else edits the tree). Everything docker goes under  bash ${S}/tools/with-lock.sh docker 2700 -- <cmd> . Private network, containers named qv2e-*, remove everything at the end (check docker ps -a). Do NOT touch the live stack (containers named quarry-edge, quarry-*, model-plane-*, verevon-*, auth-*, etc.): build and run only your own.
First read ${NOTES}/wf-F7-evidence.md: it is the recipe of the previous evidence run (image build with deploy/compose/Dockerfile.edge, standalone run with the dev bypass and the traps it hit, the SSRF workaround for a local bomb server). Reuse its working commands.
BUILD the image from the CURRENT HEAD (git rev-parse --short HEAD) and run it. CHECKS (PASS / FAIL / NOT-RUN with the exact command and the evidence line each; never PASS without having seen the output):
1 /ready: GET /ready returns the JSON documented in docs/openapi.yaml (read the /ready path) with a browser field; with QUARRY_BROWSER_REAPER and the driver caps at their defaults it is 200.
2 Markdown size: scrape a large public page with only_main_content=false (for example https://en.wikipedia.org/wiki/List_of_countries_by_GDP_(nominal)) with formats [markdown]: the markdown is within a small multiple (say under 5x) of the page size; report both sizes; compare with the 30.9 MB from 1.05 MB of the previous run.
3 Filesystem artifact list: with the fs artifact backend on a docker volume, scrape with formats [markdown,html] so an artifact is written (non-ZDR), GET /v1/artifacts lists it; restart the container; it is still listed; a ZDR scrape adds nothing.
4 Flate bomb: serve the flate-bomb PDF the way the previous run did and show the response is now 415 pdf_parse_failed (or the typed reason R7 chose: read docs/ERROR_REASONS.md) well before 15 s, and that docker stats shows memory under the limit.
5 Agent tab calls during a take-over (R2): start an agent run with a service-like bearer if the dev bypass allows two identities (read crates/quarry-edge/src/auth.rs for the dev bypass), take control as the person, then an agent tab select must be refused with 409 USER_TOOK_OVER when enforcement is on (QUARRY_EDGE__TAKEOVER_ENFORCE or the name in run_limits.rs) and allowed when off; NOT-RUN with the reason if the dev bypass cannot express two identities.
6 Run caps: QUARRY_EDGE__MAX_CONCURRENT_RUNS=1: the second start is 429 with a Retry-After header.
7 Challenge classification (R6): if a scrape of a recorded challenge fixture is possible (serve a local page with a captcha widget marker from a container on your network, SSRF workaround as before), the scrape failure carries the typed block_reason and no page text; otherwise NOT-RUN.
Write the table of checks to ${NOTES}/wf-R9-evidence.md AND append a short 'Evidence, round 2' section to ${Q}/docs/RUNBOOKS.md (append-only; commit with qcommit.sh). Clean up containers, network, volume and the image tag at the end.`, { label: 'evidence2', phase: 'Evidence', schema: IMPL, ...SONNET })

// ================================================================ STAGE 4: integrate
phase('Integrate')
const integrate = await A(`${PRE}
UNIT INTEGRATE-2 (you run alone; nobody else edits the tree). Several units edited the shared tree in parallel. Make sure the branch as a whole is sound.
1. git status --short and git log --oneline eb92d4ee..HEAD. Anything uncommitted that belongs to a finished unit: commit it with qcommit.sh and explicit paths after checking it. The only legitimately uncommitted paths are the WS5.G files of another session (model-gateway grpc.rs and quarry.rs; gateway domains/knowledge/quarry.rs and search.rs; verevonv3 src/shared/api/knowledge-client.ts and src/shared/i18n/errors.ts) and .plan-work/.
2. Full test pass, one qtest call at a time, each started in the background with a log and waited on with qwait.sh: (a) cargo check --workspace --all-targets; (b) cargo test -p quarry-core -p quarry-transform -p quarry-security; (c) cargo test -p quarry-runtime (default features); (d) cargo test -p quarry-edge (all binaries; the Redis ones need REQUIRE_CONTRACT unset locally unless you start the throwaway redis the way unit R8a-ci did: do it, it is the CI shape); (e) cargo test -p quarry-core --features jsonschema --test scrape_schema; (f) postgres: ${S}/tools/pg-test.sh pattern with cargo test -p quarry-runtime --features postgres-queue --lib postgres_ -- --test-threads=1, wrapped in the docker lock; (g) cargo test -p quarry-browser --features chromiumoxide with the live prefix, run 3 times (the flaky test was supposedly fixed by unit R1-browser: report any failure); (h) node --test scripts/tests/*.test.mjs in ${Q}; (i) the Verevon gateway: native cargo test in apps/Frontend Plane/verevonv3/apps/gateway (the four WS5.G files are present in the tree and compile); (j) the browser broker: go vet ./... && go test -count=1 ./... natively, and the tagged integration run of unit R3-ws1c under the docker lock (read its notes for the command); (k) model-gateway --lib through ${S}/INTEGRATE/mp-test.sh under the docker lock (expect exactly the one known CRLF failure zdr_precheck::the_zdr_stream_forwards_its_route_instead_of_dropping_it).
3. For every failure decide: caused by this branch (fix with a minimal, tested change and commit) or pre-existing (prove it with git log -3 --format=%h -- <file> on the failing code, and note it).
4. Append a dated section to ${NOTES}/quarryv2-next-status-2026-10-17.md (commits since eb92d4ee, what was verified with counts, what was not, traps) and one short paragraph to C:/Users/ImaFernandesDaCosta/.claude/projects/C--dev-CoresSystem/memory/quarry-browser-build-plan.md.
Report: verified = each command and its result; not_done = every failure you could not resolve, with evidence.`, { label: 'integrate2', phase: 'Integrate', schema: IMPL, ...SONNET })

const critic = await A(`${PRE}
YOU ARE THE COMPLETENESS CRITIC (read-only; no commits). Inputs: this round's unit reports below. Read git log of the branch (git log --oneline eb92d4ee..HEAD), ${NOTES}/round1-leftovers.md, and the plan (${PLAN}). Answer: (1) which items of round1-leftovers.md are still undone, with evidence (grep or a command); (2) which plan slices are now unblocked and not started, in plan order, and which are blocked and by what (decision id, sibling session, missing credential); (3) any claim in a commit message or report of this round that you could not confirm; (4) risks: anything the round changed that needs an owner decision before the branch is shared (squash candidates, trailer conflicts, CRLF artifacts, anything that would break a deploy). Be specific and short.
UNIT REPORTS: ${JSON.stringify({ stage1, stage2, evidence, integrate })}`, { label: 'critic2', phase: 'Integrate', schema: { type: 'object', properties: { undone: { type: 'array', items: { type: 'string' } }, unblocked_next: { type: 'array', items: { type: 'string' } }, blocked: { type: 'array', items: { type: 'string' } }, unconfirmed_claims: { type: 'array', items: { type: 'string' } }, risks: { type: 'array', items: { type: 'string' } } }, required: ['undone', 'unblocked_next', 'blocked', 'unconfirmed_claims', 'risks'] }, ...OPUS })

return { stage1, stage2, evidence, integrate, critic }
