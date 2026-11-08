export const meta = {
  name: 'space-w2-build',
  description: 'Build wave W2 (M2.1 + M2.2 + M2.3) of the approved Space plan unit by unit: Sonnet high builders, lock groups keep concurrent builders on disjoint files, compile-check only, tests and depth come after the wave',
  phases: [
    { title: 'Build', detail: 'one builder per slice, at most 5 at a time, dependency order, commit per slice', model: 'claude-sonnet-5-5' },
  ],
}

const MODEL = 'claude-sonnet-5-5'
const UNITS = __UNITS__
const LIMIT = (args && args.limit) ? args.limit : 4
const ONLY = (args && args.only) ? args.only : null
const ASSUME_DONE = new Set((args && args.assumeDone) ? args.assumeDone : [])
const PLAN = 'docs/plans/space-hybrid-build-plan-2026-10-14.md'

const RET = {
  type: 'object',
  properties: {
    slice: { type: 'string' },
    status: { type: 'string' },
    commit: { type: 'string' },
    files: { type: 'array', items: { type: 'string' } },
    compile: { type: 'string' },
    migrations: { type: 'array', items: { type: 'string' } },
    flags: { type: 'array', items: { type: 'string' } },
    tests_owed: { type: 'array', items: { type: 'string' } },
    notes: { type: 'string' },
  },
  required: ['slice', 'status'],
}

const GROUP_HINT = {
  plat: 'You own repo-level files for M0: scripts/, .github/workflows/, and test fixture files under apps/Frontend Plane/verevonv3/tests/e2e/. You do not touch product source.',
  sc: 'Your lock group is the Model Plane transcript and run stack: apps/Model Plane/rust/services/session-core, model-gateway, mp-contracts and the Model Plane proto files. Other builders (control, gateway, SPA, identity) run at the same time in other directories.',
  run: 'Your lock group is the Model Plane runtime: execution-core, capability-core, sandbox-manager, browser-broker and their compose/config files. Other builders run at the same time in other directories. mp-contracts/proto belong to the session-core group: if you need a proto field, add it minimally and say so.',
  ctl: 'Your lock group is Control Plane user-core (apps/Control Plane/user-core) and Control config. Other builders run at the same time in other directories.',
  idn: 'Your lock group is Control identity services: org-core, audit-core, auth-core and integration-corev2 where named. Other builders run at the same time in other directories.',
  app: 'Your lock group is the Application Plane: apps/Application Plane/convex-core, notification-core, conversation-core. Other builders run at the same time in other directories.',
  gw: 'Your lock group is the Frontend gateway BFF: apps/Frontend Plane/verevonv3/apps/gateway and its nginx/compose entries. It holds no durable Space state. Other builders run at the same time in other directories.',
  'spa-room': 'Your lock group is the SPA room surfaces: apps/Frontend Plane/verevonv3/src/features/spaces, src/shared/api (spaces-client, chat-client), src/shared/actions, src/shared/i18n. The SPA shell group builds settings, app shell and nginx at the same time.',
  'spa-shell': 'Your lock group is the SPA shell and settings: apps/Frontend Plane/verevonv3/src/app, src/features/settings, src/features/core, nginx.conf, index/vite config. The SPA room group builds src/features/spaces at the same time.',
  dat: 'Your lock group is the Data/Ingestion plane files named by your slice.',
}

function prompt(u) {
  const where = u.ws
    ? `Plan subsection: Grep '#### ${u.ws} ' in ${PLAN} (currently lines ${u.planStart}-${u.planEnd}); design file .plan-work/design/${u.ws}.md.`
    : `This is a seam/baseline row, defined in plan sections 4.1.1 (M0 exit criteria), 4.4.2 (prep refactors) and 4.5-4.6: Grep '${u.id}' in ${PLAN}; related workstream slices that consume it name it in their tables.`
  const deps = u.deps.length ? `Dependencies already built (read .plan-work/space-build/units/<id>.json for each): ${u.deps.join(', ')}.` : 'No slice dependencies.'
  return `Read C:/dev/CoresSystem/.claude/worktrees/space-main-merge/.plan-work/00-build-rules-w2.md first and follow it exactly; your first shell command is cd "C:/dev/CoresSystem/.claude/worktrees/space-main-merge". You build slice ${u.id}: ${u.what} (lane ${u.lane}, size ${u.size}, milestone ${u.ms} of wave W2).
${where}
${deps}
${GROUP_HINT[u.group] || ''}
SKIP IF DONE: if .plan-work/space-build/units/${u.id}.json exists with status built or already-present, read it and return it without changing anything. If it exists with status partial or blocked, or the working tree already contains uncommitted edits for this slice (git status), continue from there instead of starting over.
Build the slice to the plan's scope (not beyond), compile-check once, commit your files only, write the two result files, and return the result object. Depth findings go to the depth file, not into the code. Tests are NOT written now (list them under tests_owed).`
}

phase('Build')
const units = ONLY ? UNITS.filter(u => ONLY.includes(u.id)) : UNITS
const state = {}
const info = {}
units.forEach(u => { state[u.id] = ASSUME_DONE.has(u.id) ? 'built' : 'pending' })
const OKSTATES = ['built', 'partial', 'already-present']
const FAILSTATES = ['blocked', 'failed', 'skipped']
const busy = new Set()
const running = new Map()

function depsOk(u) {
  return u.deps.every(d => !(d in state) || OKSTATES.includes(state[d]))
}
function depsFailed(u) {
  return u.deps.some(d => (d in state) && FAILSTATES.includes(state[d]))
}

function launch(u) {
  state[u.id] = 'running'
  busy.add(u.group)
  log('start ' + u.id + ' (' + u.group + ')')
  const p = (async () => {
    let r = null
    try {
      r = await agent(prompt(u), { label: 'build-' + u.id, phase: 'Build', model: MODEL, effort: 'high', schema: RET })
    } catch (e) {
      r = null
    }
    if (!r) {
      state[u.id] = 'failed'
    } else {
      const s = (r.status || '').toLowerCase()
      state[u.id] = OKSTATES.includes(s) ? s : (s === 'blocked' ? 'blocked' : 'partial')
      info[u.id] = { status: r.status, commit: r.commit, compile: r.compile, notes: (r.notes || '').slice(0, 300) }
    }
    busy.delete(u.group)
    running.delete(u.id)
    const done = Object.values(state).filter(x => OKSTATES.includes(x)).length
    log(u.id + ' -> ' + state[u.id] + ' (' + done + '/' + units.length + ' built or partial)')
    return u.id
  })()
  running.set(u.id, p)
}

while (true) {
  for (const u of units) {
    if (state[u.id] === 'pending' && depsFailed(u)) state[u.id] = 'skipped'
  }
  for (const u of units) {
    if (running.size >= LIMIT) break
    if (state[u.id] === 'pending' && !busy.has(u.group) && depsOk(u)) launch(u)
  }
  if (running.size === 0) break
  await Promise.race(Array.from(running.values()))
}

const counts = {}
Object.values(state).forEach(s => { counts[s] = (counts[s] || 0) + 1 })
log('W2 batch finished: ' + JSON.stringify(counts))
return { counts, state, info }
