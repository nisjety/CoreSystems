// Builds build-dag-A4.workflow.js from build-dag-A2.workflow.js: a FRESH run (no resume cache: the cache is positional and
// diverges when cached results resolve instantly) that
//  - treats tasks already built AND reviewed 'ok' (23 in the earlier run, plus args.done from later passes) as statically
//    done: no agent call at all;
//  - runs everything else with Haiku 5.5 at effort max (implementer then adversarial reviewer), with a state-update notice
//    instead of the old RESUME NOTICE;
//  - for the tasks whose implementer finished but whose review never returned (QB-T04, Z-00, Z-12) runs only the reviewer
//    (the implementer report was lost with the interrupted run; the reviewer reconstructs from the committed tree);
//  - caps how many NEW tasks may start in a pass (args.maxNew); deferred tasks end gracefully, no agent is killed mid-edit.
// Edits use split/join only. Output is LF-only.
const fs = require('fs')
const S = process.argv[2]
let src = fs.readFileSync(S + '/build-dag-A2.workflow.js', 'utf8').replace(/\r\n/g, '\n')
const once = (needle) => { const n = src.split(needle).length - 1; if (n !== 1) throw new Error('anchor count ' + n + ' for ' + JSON.stringify(needle.slice(0, 60))) }
const swap = (a, b) => { once(a); src = src.split(a).join(b) }

swap('name: "build-dag-wave-a2"', 'name: "build-dag-wave-a4"')
swap('description: "Resumed dependency-aware build of the 53 runnable tasks', 'description: "Fresh pass (Haiku 5.5 at max, capped number of new tasks, finished tasks skipped) of the dependency-aware build of the 53 runnable tasks')
swap('const RULES = `Repository root:', 'const RULES_OLD = `Repository root:')
swap('const taskCmd = (id) =>', String.raw`const STATE_UPDATE = 'STATE UPDATE 2026-10-10: everything built in the earlier runs of this build, and the other sessions\' earlier uncommitted work, is now COMMITTED on the local branch main (HEAD 1d8ce7d5; main before the consolidation was 2f2c3e83; nothing is pushed), so git status shows a clean tree except for work done after that point. An interrupted earlier attempt of YOUR task may therefore sit inside those commits as partial or unfinished code: read what earlier attempts changed with git log --stat 2f2c3e83..HEAD -- <your files> and git diff 2f2c3e83 HEAD -- <your files>, decide hunk by hunk what belongs to YOUR task (new files named in your manifest entry, migrations, tests, fixtures, docs, hunks implementing your described behaviour) and what belongs to someone else (cost-core azurecost, billing-core, insight-core, conversation-core and other tasks files: never touch them), then complete, correct or remove your own partial work so the final state is complete, consistent and tested; never leave a half-finished hunk. Your own edits show as uncommitted changes (still do NOT commit). Other agents and other sessions may be editing the same tree at the same time. Say in your report what you found and what you did.'
const MODEL_NOTE = 'MODEL NOTE: you run as a small fast model at maximum effort. Be methodical: read the manifest entry and every file you touch completely, re-read before every edit, run the targeted tests after every change, never claim a test passed that you did not run, and report failures honestly. Before you report, run every check that CI runs for the files you touch (read .github/workflows for the path filters): when you touch contract goldens, their vendored copies or scripts/tests, that includes bash scripts/tests/zdr-contract-sync.test.sh AND bash scripts/tests/zdr-contract-sync.test.sh --self-test and node --test scripts/tests/zdr-contract-schema.test.mjs. A compile error that contradicts the source usually means a stale artifact from another agent building in the same target directory: run cargo clean -p <package> and rebuild before you conclude anything.'
const RULES_NEW = RULES_OLD.split('\n').map((l) => (l.startsWith('RESUME NOTICE:') ? STATE_UPDATE : l)).join('\n') + '\n' + MODEL_NOTE
const REVIEW_NOTE = 'STATE UPDATE 2026-10-10: all earlier work is committed on the local branch main (HEAD 1d8ce7d5; main before the consolidation was 2f2c3e83): judge this task with git diff 2f2c3e83 HEAD -- <files> plus any uncommitted edits (git status). Other sessions and agents may be editing the tree at the same time. You run as a small fast model at maximum effort: read the code yourself, run the targeted tests yourself and do not trust the implementer report. Also run every check that CI runs for the files this task touches (read .github/workflows): when contract goldens, vendored copies or scripts/tests changed, run bash scripts/tests/zdr-contract-sync.test.sh, the same with --self-test, and node --test scripts/tests/zdr-contract-schema.test.mjs; the previous reviewers missed a --self-test break that way.'

const taskCmd = (id) =>`)
swap('const implPrompt = (t) => RULES + `', 'const implPrompt = (t) => RULES_NEW + `')
swap('Prerequisite tasks already built and reviewed in this run:', 'Prerequisite tasks already built and reviewed (in earlier runs, now committed on main, or in this run):')

const cut = src.indexOf('async function runTask(t) {')
if (cut < 0) throw new Error('runTask not found')
src = src.slice(0, cut) + String.raw`// Tasks built and reviewed 'ok' in run wf_0dd51c33-48a (Sonnet), plus any later pass listed in args.done: statically done.
const DONE_BASE = ['CR-T6','CR-N1','CR-T5','CR-T7','CR-T8','QB-M1b','CR-T11','CR-T10','CR-T9a','CR-T13','CR-T9b','CR-T12','QB-T03','CR-T14','CR-T3','Z-10','Z-08','QB-T02','Z-04','QB-T13a','CR-T16','Z-30','QB-T13b']
const DONE = new Set([...DONE_BASE, ...((args && args.done) || [])])
// Implementer finished in the earlier run, review never returned: review only.
const REVIEW_ONLY = new Set((args && args.reviewOnly) || ['QB-T04', 'Z-00', 'Z-12'])
const NO_REPORT = 'NOT ATTACHED: the implementer of this task finished in an earlier run but its report was lost when that run was interrupted. Reconstruct what was built from your manifest entry, the committed tree (git diff 2f2c3e83 HEAD -- <the files your entry lists>), the tests and any uncommitted edits, and judge it as if you had written the report yourself.'
const OPTS = (label) => ({ label, phase: 'Build', model: 'haiku', effort: 'max' })
const MAX_NEW = Number(args && args.maxNew != null ? args.maxNew : 8)
let started = 0

const reviewPrompt2 = (t, report) => reviewPrompt(t, report).split('\nIMPLEMENTER REPORT:\n').join('\n' + REVIEW_NOTE + '\nIMPLEMENTER REPORT:\n')

async function runTask(t) {
  let impl = NO_REPORT
  if (!REVIEW_ONLY.has(t.id)) {
    impl = await agent(implPrompt(t), OPTS('impl:' + t.id))
    if (!impl) return { id: t.id, status: 'failed', note: 'implementer returned nothing' }
  }
  const rev = await agent(reviewPrompt2(t, String(impl).slice(0, 13000)), OPTS('rev:' + t.id))
  if (!rev) return { id: t.id, status: 'unreviewed', note: 'reviewer returned nothing', impl: String(impl).slice(0, 800) }
  const verdict = verdictOf(rev)
  const ok = verdict && !/^do not ship/.test(verdict)
  return { id: t.id, status: ok ? 'ok' : 'needs_attention', verdict, impl: String(impl).slice(0, 700), review: String(rev).slice(0, 1500) }
}

phase('Build')
log('fresh pass: cap ' + MAX_NEW + ' new tasks; statically done: ' + DONE.size + '; review only: ' + [...REVIEW_ONLY].join(',') + '; every agent runs on haiku at effort max')
const promises = {}
for (const t of DAG) {
  const prereqs = t.after.map(id => promises[id]).filter(Boolean)
  promises[t.id] = Promise.all(prereqs).then(async (done) => {
    if (DONE.has(t.id)) return { id: t.id, status: 'ok', note: 'done in an earlier run' }
    const bad = done.filter(d => d.status !== 'ok')
    if (bad.length) return { id: t.id, status: 'blocked', blocked_by: bad.map(b => b.id + ':' + b.status) }
    if (started >= MAX_NEW) return { id: t.id, status: 'deferred', note: 'per-pass cap of new tasks reached' }
    started++
    log('starting ' + t.id + (REVIEW_ONLY.has(t.id) ? ' (review only)' : ''))
    return runTask(t)
  })
}
const all = await Promise.all(DAG.map(t => promises[t.id]))
const tally = all.reduce((a, r) => (a[r.status] = (a[r.status] || 0) + 1, a), {})
return { tally, started, tasks: all.filter(r => !(r.status === 'ok' && r.note === 'done in an earlier run')) }
`
fs.writeFileSync(S + '/build-dag-A4.workflow.js', Buffer.from(src, 'utf8'))
console.log('written build-dag-A4.workflow.js,', src.split('\n').length, 'lines, CR bytes:', (src.match(/\r/g) || []).length)
