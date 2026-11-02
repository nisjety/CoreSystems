// Prints the args of the LAST Workflow tool call in the main session transcript (jsonl) as JSON to a file, and a short summary, so that an
// interrupted pass can be relaunched with a fresh args.done (never resumeFromRunId: the cache is positional and re-runs finished agents).
// usage: node extract_last_workflow_args.cjs <transcript.jsonl> <output.json>
const fs = require('fs')
const [, , J, OUT] = process.argv
let last = null
for (const l of fs.readFileSync(J, 'utf8').split('\n')) {
  if (!l.includes('"name":"Workflow"')) continue
  try {
    const o = JSON.parse(l)
    const walk = (x) => {
      if (!x || typeof x !== 'object') return
      if (x.type === 'tool_use' && x.name === 'Workflow' && x.input && x.input.args) last = x.input.args
      for (const k of Object.keys(x)) walk(x[k])
    }
    walk(o)
  } catch (e) { /* skip lines that are not whole JSON */ }
}
if (!last) { console.log('no Workflow call with args found'); process.exit(1) }
fs.writeFileSync(OUT, JSON.stringify(last))
console.log('last Workflow args saved:', Object.keys(last).join(', '), '| done', (last.done || []).length, '| notes for', Object.keys(last.notes || {}).join(', ') || 'none', '| bytes', JSON.stringify(last).length)
