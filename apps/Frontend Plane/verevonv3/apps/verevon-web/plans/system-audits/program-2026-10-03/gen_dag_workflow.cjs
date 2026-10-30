// Generates the dependency-aware build workflow for the runnable tasks of build-manifest.json.
// Usage: node gen_dag_workflow.cjs <out.js> <EXCLUDE_ROOTS comma list> <name> <description>
const fs = require('fs')
const S = 'C:/Users/IMAFER~1/AppData/Local/Temp/claude/C--Users-ImaFernandesDaCosta-OneDrive---Aquatiq-Dokumenter-CoresSystem-apps-Frontend-Plane-verevonv3/b44cb88b-b6fc-4fa7-b484-b93c9017d9e9/scratchpad'
const [out, excludeCsv, name, description, onlyCsv] = process.argv.slice(2)
const m = JSON.parse(fs.readFileSync(S + '/build-manifest.json', 'utf8'))
const byId = Object.fromEntries(m.tasks.map(t => [t.id, t]))
const chainTasks = m.chains.flatMap(c => c.tasks.map(t => ({ ...t, chain: c.id })))
const allIds = new Set(chainTasks.map(t => t.id))
const exclude = new Set((excludeCsv || '').split(',').map(s => s.trim()).filter(Boolean))
const only = onlyCsv ? new Set(onlyCsv.split(',').map(s => s.trim()).filter(Boolean)) : null
const depsOf = t => [...new Set([...(byId[t.id]?.depends_on || t.depends_on || []), ...(t.cross_chain_waits || [])])].filter(d => allIds.has(d))
// blocked closure from the exclude roots
const blocked = new Set(exclude)
let changed = true
while (changed) {
  changed = false
  for (const t of chainTasks) {
    if (blocked.has(t.id)) continue
    if (depsOf(t).some(d => blocked.has(d))) { blocked.add(t.id); changed = true }
  }
}
let runnable = chainTasks.filter(t => !blocked.has(t.id))
if (only) runnable = runnable.filter(t => only.has(t.id))
const runIds = new Set(runnable.map(t => t.id))
// nearest previous runnable task in the same chain is a soft prerequisite (collision ordering)
const dag = []
for (const c of m.chains) {
  let prev = null
  for (const t of c.tasks) {
    if (!runIds.has(t.id)) continue
    const after = new Set(depsOf(t).filter(d => runIds.has(d)))
    if (prev) after.add(prev)
    dag.push({ id: t.id, chain: c.id, title: t.title, plane: byId[t.id]?.plane || '', size: t.size, design: byId[t.id]?.design || '', after: [...after] })
    prev = t.id
  }
}
// topological order
const order = []
const seen = new Set()
const byDag = Object.fromEntries(dag.map(d => [d.id, d]))
const visit = id => { if (seen.has(id)) return; seen.add(id); for (const a of byDag[id].after) visit(a); order.push(byDag[id]) }
dag.forEach(d => visit(d.id))

const script = `export const meta = {
  name: ${JSON.stringify(name)},
  description: ${JSON.stringify(description)},
  phases: [{ title: 'Build' }],
}
const DAG = ${JSON.stringify(order)}
const M = ${JSON.stringify(S + '/build-manifest.json')}
const SCR = ${JSON.stringify(S)}

const RULES = \`Repository root: C:/dev/CoresSystem (Windows; Git Bash and PowerShell available). Always use absolute paths; the session working directory is NOT the repo.
FIRST READ: \${SCR}/owner-rulings-and-defaults.md (binding owner rulings and defaults), the integrator plan \${SCR}/design-plan.txt, and the critique of your design (\${SCR}/critic-contextual.txt, critic-quarry.txt or critic-zdr.txt: your manifest entry says which design and folds in the critic corrections that apply to you: re-check them in the code). The full original designs are \${SCR}/design-contextual.json, design-quarry.json, design-zdr.json (fields: tasks, current_state, defects_found, design) if you need the reasoning behind a task.
OWNER RULES: no shortcuts, best long-term design, honest data-protection claims, zero-retention (ZDR) is per org and respected, a per-run ZDR downgrade only through the authoritative consent path never client-controllable, reranking is always paid and never the subscription, contextual retrieval stays on and is made perfect. Contract tests must apply the REAL rules (real policy file, real middleware, real session-core credential checks, real provider configuration), not permissive doubles: the 2026-10-01 live proof found six defects stubs had hidden.
HARD RULES: do NOT git commit, stash, reset, checkout or revert anything. The tree is SHARED with other sessions and with other agents building other chains at the same time: edit only the lines your task needs, re-read a file immediately before every Edit, use the Edit tool (it keeps CRLF or LF), never rewrite or reformat a whole file (rustfmt or gofmt on a single file you own is fine only if you then restore that file's original line endings), never restore a file from a backup copy, never run a repo-wide formatter, and confirm no file has mixed line endings (count CRLF against bare LF with node; .ts/.tsx/.js/.json/.yml/.sh are LF; .rs/.go/.md and Python are CRLF in the working tree). Files that already carry another session's uncommitted changes (your manifest entry lists them as dirty_files_collision) must be edited by hunk, preserving those changes. If a build or test fails ONLY because of errors in files outside your task (another agent may be mid-edit in the same crate or package), wait about two minutes and retry; never edit or revert their files. Do NOT build or tag images for deployment, run the launcher, restart or recreate containers, or run docker compose: the orchestrator deploys stage by stage (a throwaway docker build --target build compile check is allowed ONLY for Quarry-v2, which cannot compile on this host: see the Quarry note in your entry; run nothing else in docker). Make NO model or inference call yourself and no paid-provider call; where your entry has a live proof, build the harness and runbook with exact commands and pass criteria and leave the live run to the orchestrator. Never print secrets (no env values, tokens, keys, .env or .env.generated-secrets contents, password hashes): filter any command that might. Read-only live checks are allowed (docker logs, psql SELECT aggregates as user postgres on the database containers, curl health endpoints, config files). Tests first: they must fail on the defect. Run targeted tests only (cargo test -p <crate> <filter>, go test ./path/..., npx vitest run <file>, jest <spec>, pytest <file>); do not run whole-workspace suites; the execution-core test a_hung_child_is_killed_at_the_timeout hangs on Windows (skip it). Write temporary files only under \${SCR}. New behaviour must be dark or safe by default unless your entry says otherwise, and every feature switch you add needs a default, a boot-time check and a documented runbook line.
RESUME NOTICE: the first run of this build was cut off by a usage limit while several tasks were in progress, so an EARLIER ATTEMPT OF YOUR TASK may have left PARTIAL edits in the tree: half-written hunks, new files (migrations, tests, fixtures, docs), or failing tests, in the files your manifest entry lists. Other sessions and other tasks also have edits in the same tree. Before writing code: run git status and git diff on the files your entry lists and look for files modified after 2026-10-03 16:20; decide hunk by hunk what belongs to YOUR task (new files named in your entry, new migration or test files, hunks implementing your described behaviour) and what belongs to someone else (cost-core azurecost, billing-core, insight-core, conversation-core and other tasks' files: never touch them); then keep, complete or remove your own partial work so the final state is complete, consistent and tested; never leave a half-finished hunk. Say in your report what you found and what you did.
FINAL REPORT (plain text): files changed (with functions), tests added, the exact commands run with pass and fail counts, what you verified live and what you could not, deviations from your manifest entry (with reasons), the deploy note (which image or config change is needed, in which stage, and what must be deployed before it), the exact live-proof commands and pass criteria for the orchestrator, and follow-ups. Be honest about failures.\`

const taskCmd = (id) => \`node -e 'const m=JSON.parse(require("fs").readFileSync(process.argv[1],"utf8"));const t=m.tasks.find(x=>x.id===process.argv[2]);console.log(JSON.stringify(t,null,1))' "\${M}" \${id}\`
const ruleCmd = \`node -e 'const m=JSON.parse(require("fs").readFileSync(process.argv[1],"utf8"));console.log(JSON.stringify({rules_for_every_implementer:m.rules_for_every_implementer,shared_file_policy:m.shared_file_policy,crate_ownership_exceptions:m.crate_ownership_exceptions,id_convention:m.id_convention,cp_wave:m.cp_wave},null,1))' "\${M}"\`

const SECURITY = (t) => /^Z-/.test(t.id) || ['CR-T3','CR-T5','CR-T7','CR-T10','CR-T12','QB-T03','QB-T04','QB-T05a','QB-T05b','QB-T09','QB-T09a','QB-T10','QB-M','QB-M1a','QB-M1b','CR-T8','CR-T9a','CR-T9b'].includes(t.id)

const implPrompt = (t) => RULES + \`

YOUR TASK: \${t.id} (design \${t.design}) - \${t.title}. Chain \${t.chain}, plane \${t.plane}, size \${t.size}. Prerequisite tasks already built and reviewed in this run: \${t.after.join(', ') || 'none'} (read what they changed in the tree before you edit the same files: git status and git diff on your files).
Read your manifest entry with this Bash command (it prints the full entry: files, depends_on, corrections, critic corrections, notes for the implementer, dirty-file collisions, deploy note, live proof):
\${taskCmd(t.id)}
Read the shared rules and policies with:
\${ruleCmd}
Build it completely and correctly, tests first, following the entry. If the entry is wrong or stale where it meets the code, do the correct thing and say so. If a prerequisite you depend on is NOT actually present in the tree, stop and report exactly what is missing instead of inventing it.\`

const reviewPrompt = (t, report) => \`You are the ADVERSARIAL REVIEWER of task \${t.id} (\${t.title}), design \${t.design}, chain \${t.chain}. Repository root C:/dev/CoresSystem. Read \${SCR}/owner-rulings-and-defaults.md and your manifest entry with: \${taskCmd(t.id)}
Then read the implementer report below, open every changed file and read the changed hunks (the tree holds other sessions' and other agents' uncommitted edits: judge only this task's hunks, and verify that other people's hunks in the same files are intact: git diff). \${SECURITY(t) ? 'THIS TASK TOUCHES AUTHORITY, TENANCY, ZERO-RETENTION, METERING OR EXTERNAL PROCESSORS: be hostile. Hunt for: any path that sends tenant content to a processor that is not approved for that org (ZDR orgs especially), any silent downgrade or silent drop, a client-controllable flag that changes retention or authority, an authorisation check that only a permissive test double exercises, cross-tenant or cross-user leaks, a credential or scope wider than needed, missing or wrong org attribution, replay or idempotency holes, secrets or content in logs, metering that double counts or drops usage.' : 'Hunt for: wrong or stale assumptions about the code, edge-case panics, tests that pass vacuously, behaviour that is on by default but should be dark, missing migrations or rollbacks, config without defaults or boot-time checks, mixed line endings, edits outside the task scope.'} For every test the implementer relies on, do the mutation: apply the opposite behaviour in a TEMP COPY under \${SCR} (never in the real tree) or reason precisely from the code, and state whether the test fails on the defect. Verify live read-only what you can. You MAY fix real defects yourself with small Edit calls (re-read before editing; no commit, stash or revert; never restore from a backup copy; no image builds or tags; no restarts; no model calls; never print secrets) and re-run the targeted tests; if a build fails only because another agent is mid-edit elsewhere, wait two minutes and retry. Do not expand scope. Return a plain-text report that STARTS with one line: VERDICT: ship | ship after the fixes I made | do not ship. Then: each defect and whether fixed, tests run with results, what you verified live, deploy-order warnings, anything left open.
IMPLEMENTER REPORT:
\${report}\`

const verdictOf = (text) => {
  const m = String(text || '').match(/VERDICT:\\s*\\**\\s*(ship after the fixes I made|ship after the fixes|ship after fixes|ship|do not ship)/i)
  return m ? m[1].toLowerCase() : null
}

async function runTask(t) {
  const impl = await agent(implPrompt(t), { label: 'impl:' + t.id, phase: 'Build', model: 'sonnet' })
  if (!impl) return { id: t.id, status: 'failed', note: 'implementer returned nothing' }
  const rev = await agent(reviewPrompt(t, String(impl).slice(0, 13000)), { label: 'rev:' + t.id, phase: 'Build', model: 'sonnet' })
  if (!rev) return { id: t.id, status: 'unreviewed', note: 'reviewer returned nothing', impl: String(impl).slice(0, 800) }
  const verdict = verdictOf(rev)
  const ok = verdict && !/^do not ship/.test(verdict)
  return { id: t.id, status: ok ? 'ok' : 'needs_attention', verdict, impl: String(impl).slice(0, 700), review: String(rev).slice(0, 900) }
}

phase('Build')
const promises = {}
for (const t of DAG) {
  const prereqs = t.after.map(id => promises[id]).filter(Boolean)
  promises[t.id] = Promise.all(prereqs).then(async (done) => {
    const bad = done.filter(d => d.status !== 'ok')
    if (bad.length) return { id: t.id, status: 'blocked', blocked_by: bad.map(b => b.id + ':' + b.status) }
    log('starting ' + t.id)
    return runTask(t)
  })
}
const all = await Promise.all(DAG.map(t => promises[t.id]))
const tally = all.reduce((a, r) => (a[r.status] = (a[r.status] || 0) + 1, a), {})
return { tally, tasks: all }
`
fs.writeFileSync(out, script)
console.log('written', out, 'runnable', order.length, 'blocked-by-closure', blocked.size, 'bytes', script.length)
console.log('first tasks (no prerequisites):', order.filter(d => !d.after.length).map(d => d.id).join(', '))
