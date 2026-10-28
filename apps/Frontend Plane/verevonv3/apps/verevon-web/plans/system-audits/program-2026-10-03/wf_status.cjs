// One compact status line for a workflow run from its journal: counts, running agents, finished agents (with reviewer verdicts).
// usage: node wf_status.cjs <run dir>
const fs = require('fs')
const D = process.argv[2]
let L = []
try { L = fs.readFileSync(D + '/journal.jsonl', 'utf8').trim().split('\n').filter(Boolean).map((l) => JSON.parse(l)) } catch (e) { console.log('no journal yet'); process.exit(0) }
const lab = {}, t0 = {}, res = {}, failed = new Set()
for (const o of L) {
  if (o.type === 'started') { lab[o.agentId] = o.label; t0[o.agentId] = true }
  if (o.type === 'result') res[o.agentId] = String(o.result || '')
  if (o.type === 'failed') failed.add(o.agentId)
}
const running = Object.keys(lab).filter((a) => !(a in res) && !failed.has(a)).map((a) => lab[a])
const done = Object.keys(res).map((a) => {
  const t = res[a]
  const v = /VERDICT:\s*\**\s*([a-zA-Z ]+?)(\s{2,}|\n|$)/.exec(t)
  return lab[a] + (lab[a].startsWith('rev:') ? '=' + (v ? v[1].trim().replace('ship after the fixes I made', 'ship+fixes') : '?') : '')
})
const now = new Date()
const hh = String(now.getHours()).padStart(2, '0') + ':' + String(now.getMinutes()).padStart(2, '0')
console.log(`[${hh}] started=${Object.keys(lab).length} finished=${Object.keys(res).length} failed=${failed.size} | running: ${running.join(' ') || '-'} | finished: ${done.join(' ') || '-'}`)
