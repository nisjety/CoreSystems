export const meta = {
  name: 'depth-fix',
  description: 'Run depth fix packets through code (haiku max), test (sonnet medium) and control (opus medium), with one fix round',
  phases: [
    { title: 'Code', detail: 'haiku max implements the fix and its regression test' },
    { title: 'Test', detail: 'sonnet medium runs the named tests and the compile checks' },
    { title: 'Control', detail: 'opus medium reviews the diff and gives a verdict' },
    { title: 'Fix round', detail: 'one round for a controller verdict of fix' },
  ],
}

// args: { packets: [{ key, title, brief, files, new_files?, tests, cwd?, rows? }], targetDir? }
// Owner order 2026-10-26: Haiku max codes, Sonnet medium tests, Opus medium controls; the lead commits.
const ROOT = 'C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b'
const TARGET = (args && args.targetDir) || 'C:/dev/mp-target-w2'
const RULES = `
Repository: ${ROOT} (a git worktree on branch model-plane). Work only inside it.
Hard rules:
- Never run git commit, git stash, git reset, git checkout, git clean or git push. The lead commits.
- Never run docker or docker compose. Never read, print or grep any .env file or credential value.
- Edit only the files this task names, plus the new test files it names. If another file must change, stop and report the exact lines under needs_other_files.
- Keep each file's line endings: measure CRLF vs LF before editing (python: open(p,'rb').read().count(b'\\r\\n')) and write the same style back. Many Rust and Go files here are CRLF.
- Rust: run cargo with CARGO_TARGET_DIR=${TARGET} and CARGO_INCREMENTAL=0 from "apps/Model Plane/rust". Go: run from the service directory with GOFLAGS unset. Frontend: run pnpm from "apps/Frontend Plane/verevonv3" (vitest may need --pool=threads on this machine).
- Switches stay default off and use the strict literal parser; behaviour must be identical with a switch off. Zero data retention must hold on every path you touch.
- Database-backed tests (ignored without DATABASE_URL, or behind a build tag) cannot run here: write them, make sure they compile, and list them under db_tests; the lead runs them through the harness.
- Code style: match the surrounding code, comments explain why, no new abstractions.`

const CODE = { type: 'object', properties: {
  files_changed: { type: 'array', items: { type: 'string' } },
  summary: { type: 'string' },
  tests_run: { type: 'array', items: { type: 'object', properties: { command: { type: 'string' }, outcome: { type: 'string' } }, required: ['command', 'outcome'] } },
  db_tests: { type: 'array', items: { type: 'string' } },
  needs_other_files: { type: 'array', items: { type: 'string' } },
  deploy_notes: { type: 'string' },
}, required: ['files_changed', 'summary', 'tests_run', 'needs_other_files'] }

const TEST = { type: 'object', properties: {
  all_passed: { type: 'boolean' },
  results: { type: 'array', items: { type: 'object', properties: { command: { type: 'string' }, outcome: { type: 'string' }, passed: { type: 'boolean' } }, required: ['command', 'outcome', 'passed'] } },
  notes: { type: 'string' },
}, required: ['all_passed', 'results'] }

const CONTROL = { type: 'object', properties: {
  verdict: { type: 'string', enum: ['pass', 'fix'] },
  required_fixes: { type: 'array', items: { type: 'string' } },
  residual_risks: { type: 'array', items: { type: 'string' } },
}, required: ['verdict', 'required_fixes', 'residual_risks'] }

const files = (p) => [...(p.files || []), ...(p.new_files || [])]
const fileList = (p) => files(p).map(f => '- ' + f).join('\n') || '- (none)'

const codePrompt = (p, fixes) => `You are the coder for one depth fix in the Model Plane build.
${RULES}

${fromFile(p)}Task: ${p.title}${p.rows && p.rows.length ? ' (register rows ' + p.rows.join(', ') + ')' : ''}
${p.brief}

Files you may edit or create:
${fileList(p)}

${fixes ? 'The controller reviewed your previous change and requires these fixes; apply them and nothing else:\n' + fixes.map(f => '- ' + f).join('\n') + '\n' : ''}
When done, run the task's own checks once and report them:
${(p.tests || []).map(t => '- ' + t).join('\n')}
${p.cwd ? '(run them from ' + p.cwd + ')' : ''}
Return the files you changed, a short summary, each test command with its outcome, any database-backed tests you wrote, and any lines for other files.`

const testPrompt = (p, code) => `You are the tester for one depth fix in the Model Plane build. Do not edit any file.
${RULES}

${fromFile(p)}Task under test: ${p.title}
The coder reports: ${code ? code.summary : '(no report)'}
Files changed: ${(code && code.files_changed || []).join(', ')}

Run each test command of the task (from the record above when it comes from the file) from ${p.cwd ? p.cwd : 'the cwd the task names (the repository root when empty; cargo from apps/Model Plane/rust)'} and report exact pass/fail counts or the first failing assertion or compile error:
${(p.tests || []).map(t => '- ' + t).join('\n')}
Then run 'git status --short' from the repository root and say whether any file outside the task's list looks changed by this task. A command that cannot run (missing tool) is passed=false with the reason.`

const controlPrompt = (p, code, test) => `You are the controller for one depth fix in the Model Plane build. Do not edit any file. Review only.
${RULES}

${fromFile(p)}Task: ${p.title}${p.rows && p.rows.length ? ' (register rows ' + p.rows.join(', ') + ')' : ''}
Intent: ${p.brief}

Coder report: ${JSON.stringify(code || {}).slice(0, 4000)}
Tester report: ${JSON.stringify(test || {}).slice(0, 4000)}

Read the diff yourself: from the repository root run git diff on the task's files (${(p.files || []).map(f => '"' + f + '"').join(' ') || 'the files listed in the task record'}) and read its new files.
Decide: does the change close the finding completely and correctly, without weakening any other check, without leaking content, keeping switches default off, and with a regression test that would fail on the old code? Verdict 'pass' or 'fix'. For 'fix', list concrete, minimal required fixes (file, place, what). Anything that is not a blocker goes under residual_risks. Do not ask for scope beyond the finding.`

const FILE = args && args.packetsFile
const packets = FILE ? (args.keys || []).map(k => ({ key: k, title: k, fromFile: true, brief: '', files: [], tests: [] })) : ((args && args.packets) || [])
const fromFile = (p) => p.fromFile ? `Your task record is the packet whose key is "${p.key}" in the JSON file ${FILE} (field packets[]). Read it first: its title, rows, brief, files (the files you may edit), new_files (new files you may create), tests (the commands that prove it) and cwd (where to run them) are your instructions.
` : ''
const results = await pipeline(packets,
  async (p) => ({ p, code: await agent(codePrompt(p, null), { label: 'code:' + p.key, phase: 'Code', schema: CODE, model: 'haiku', effort: 'max' }) }),
  async ({ p, code }) => ({ p, code, test: await agent(testPrompt(p, code), { label: 'test:' + p.key, phase: 'Test', schema: TEST, model: 'sonnet', effort: 'medium' }) }),
  async ({ p, code, test }) => ({ p, code, test, control: await agent(controlPrompt(p, code, test), { label: 'control:' + p.key, phase: 'Control', schema: CONTROL, model: 'opus', effort: 'medium' }) }),
  async (r) => {
    if (!r.control || r.control.verdict !== 'fix') return r
    const code2 = await agent(codePrompt(r.p, r.control.required_fixes), { label: 'fix:' + r.p.key, phase: 'Fix round', schema: CODE, model: 'haiku', effort: 'max' })
    const test2 = await agent(testPrompt(r.p, code2), { label: 'retest:' + r.p.key, phase: 'Fix round', schema: TEST, model: 'sonnet', effort: 'medium' })
    const control2 = await agent(controlPrompt(r.p, code2, test2), { label: 'recontrol:' + r.p.key, phase: 'Fix round', schema: CONTROL, model: 'opus', effort: 'medium' })
    return { ...r, code2, test2, control2 }
  },
)

return results.map((r, i) => {
  const p = packets[i]
  if (!r) return { key: p.key, verdict: 'failed', reason: 'an agent died or returned nothing' }
  const last = r.control2 || r.control || {}
  const lastTest = r.test2 || r.test || {}
  return {
    key: p.key,
    rows: p.rows || [],
    title: p.title,
    files: [...new Set([...((r.code && r.code.files_changed) || []), ...((r.code2 && r.code2.files_changed) || [])])],
    verdict: last.verdict,
    tests_passed: lastTest.all_passed,
    test_results: lastTest.results,
    required_fixes_open: (r.control2 && r.control2.verdict === 'fix') ? r.control2.required_fixes : [],
    residual_risks: last.residual_risks || [],
    db_tests: [...((r.code && r.code.db_tests) || []), ...((r.code2 && r.code2.db_tests) || [])],
    needs_other_files: [...((r.code && r.code.needs_other_files) || []), ...((r.code2 && r.code2.needs_other_files) || [])],
    deploy_notes: (r.code2 && r.code2.deploy_notes) || (r.code && r.code.deploy_notes) || '',
    summary: (r.code2 || r.code || {}).summary,
  }
})
