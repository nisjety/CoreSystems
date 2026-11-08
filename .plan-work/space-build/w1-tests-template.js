export const meta = {
  name: 'space-w1-tests',
  description: 'Wave-end test phase for W1: Sonnet high test agents write the postponed tests of each slice group, run them time-boxed, triage failures into skips plus depth rows, commit',
  phases: [
    { title: 'Tests', detail: 'one agent per test cluster, one agent per lock group at a time, at most 4 concurrent', model: 'claude-sonnet-5-5' },
  ],
}

const MODEL = 'claude-sonnet-5-5'
const TUNITS = __TESTUNITS__
const LIMIT = (args && args.limit) ? args.limit : 4
const ONLY = (args && args.only) ? args.only : null

const RET = {
  type: 'object',
  properties: {
    agent: { type: 'string' },
    slices: { type: 'array', items: { type: 'string' } },
    written: { type: 'number' },
    run: { type: 'number' },
    passed: { type: 'number' },
    failed_fixed: { type: 'number' },
    skipped_deferred: { type: 'number' },
    depth_rows: { type: 'number' },
    commit: { type: 'string' },
    notes: { type: 'string' },
  },
  required: ['agent'],
}

function prompt(u) {
  return `Read .plan-work/00-test-rules.md first (current working directory = the build worktree) and follow it exactly. You are test agent ${u.id} (lock group ${u.group}). Write, run (time-boxed) and commit the postponed tests of these W1 slices: ${u.slices.join(', ')} (${u.count} tests owed in total). Each slice's unit result .plan-work/space-build/units/<slice>.json lists files and notes; the plan DoD/Tests paragraph of the slice (Grep the slice id in docs/plans/space-hybrid-build-plan-2026-10-14.md) says what must be asserted.
SKIP IF DONE: if .plan-work/space-build/tests/${u.id}.json exists, read it and return it unchanged. If tests for these slices are already partly written and committed (git log --grep for your slice ids, "test(space)"), continue with what is missing.

Owed tests (from the builders):
${u.spec}`
}

phase('Tests')
const units = ONLY ? TUNITS.filter(u => ONLY.includes(u.id)) : TUNITS
const state = {}
units.forEach(u => { state[u.id] = 'pending' })
const busy = new Set()
const running = new Map()
const info = {}

function launch(u) {
  state[u.id] = 'running'
  busy.add(u.group)
  log('start ' + u.id)
  const p = (async () => {
    let r = null
    try {
      r = await agent(prompt(u), { label: 'test-' + u.id, phase: 'Tests', model: MODEL, effort: 'high', schema: RET })
    } catch (e) {
      r = null
    }
    state[u.id] = r ? 'done' : 'failed'
    if (r) info[u.id] = { written: r.written, run: r.run, passed: r.passed, failed_fixed: r.failed_fixed, skipped_deferred: r.skipped_deferred, depth_rows: r.depth_rows, commit: r.commit }
    busy.delete(u.group)
    running.delete(u.id)
    log(u.id + ' -> ' + state[u.id] + ' ' + JSON.stringify(info[u.id] || {}))
    return u.id
  })()
  running.set(u.id, p)
}

while (true) {
  for (const u of units) {
    if (running.size >= LIMIT) break
    if (state[u.id] === 'pending' && !busy.has(u.group)) launch(u)
  }
  if (running.size === 0) break
  await Promise.race(Array.from(running.values()))
}
const counts = {}
Object.values(state).forEach(s => { counts[s] = (counts[s] || 0) + 1 })
log('W1 test phase finished: ' + JSON.stringify(counts))
return { counts, info }
