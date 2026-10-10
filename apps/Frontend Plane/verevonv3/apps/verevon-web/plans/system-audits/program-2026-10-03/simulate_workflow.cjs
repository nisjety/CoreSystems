// Dry-run of a workflow script with MOCK agents (no model is called): proves the orchestration logic (static skips, review-only
// tasks, gating cap, dependency order, model/effort opts) before any real agent is spent.
// usage: node simulate_workflow.cjs <script> '<argsJson>' [failIds comma list: tasks whose reviewer says "do not ship"]
const fs = require('fs')
const file = process.argv[2]
const args = JSON.parse(process.argv[3] || '{}')
const failRev = new Set((process.argv[4] || '').split(',').filter(Boolean))
const text = fs.readFileSync(file, 'utf8')
const m = /^export const meta = (\{[\s\S]*?\n\})\n/.exec(text)
const body = text.slice(m[0].length)
const AF = Object.getPrototypeOf(async function () {}).constructor
const calls = []
const logs = []
let t0 = 0
const agent = async (prompt, opts) => {
  const id = (opts.label || '').split(':')[1]
  calls.push({ n: calls.length + 1, label: opts.label, model: opts.model, effort: opts.effort, promptChars: prompt.length, hasState: prompt.includes('STATE UPDATE 2026-10-10'), hasOldResume: prompt.includes('RESUME NOTICE'), attached: prompt.includes('NOT ATTACHED') })
  await new Promise((r) => setTimeout(r, 5 + (calls.length % 7)))
  if ((opts.label || '').startsWith('impl:')) return 'IMPLEMENTER REPORT for ' + id + ': built and tested.'
  return (failRev.has(id) ? 'VERDICT: do not ship\nmock' : 'VERDICT: ship\nmock')
}
;(async () => {
  const run = new AF('agent', 'log', 'phase', 'args', 'budget', body)
  const result = await run(agent, (s) => logs.push(s), () => {}, args, { total: null, spent: () => 0, remaining: () => Infinity })
  console.log('logs:'); logs.forEach((l) => console.log('  ' + l.slice(0, 200)))
  console.log('agent calls:', calls.length, ' models used:', JSON.stringify([...new Set(calls.map((c) => c.model + '/' + c.effort))]))
  console.log('any call without state update notice:', calls.filter((c) => !c.hasState).length, ' any with old RESUME NOTICE:', calls.filter((c) => c.hasOldResume).length)
  console.log('calls (label, attached-report-missing flag):'); console.log('  ' + calls.map((c) => c.label + (c.attached ? '[noreport]' : '')).join(' '))
  console.log('tally:', JSON.stringify(result.tally), ' started:', result.started)
  for (const t of result.tasks) console.log('  ', t.id.padEnd(8), t.status.padEnd(10), t.blocked_by ? 'blocked_by=' + t.blocked_by.join(',') : (t.note || ''))
})().catch((e) => { console.log('SIM ERROR', e.stack.split('\n').slice(0, 4).join(' | ')); process.exit(1) })
