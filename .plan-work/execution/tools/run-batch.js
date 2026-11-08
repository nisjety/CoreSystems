export const meta = {
  name: 'run-batch',
  description: 'Implement, verify and (for risky slices) adversarially review one batch of build-plan slices in parallel, with disjoint file ownership; no docker, no commits',
  phases: [
    { title: 'Implement', detail: 'one agent per slice, model by importance' },
    { title: 'Verify', detail: 'independent sonnet high check, fix loop up to 2 rounds' },
    { title: 'Review', detail: 'opus adversarial review for authority, audit, ZDR, money slices' },
  ],
}

// args: { batchId: string, goal: string, slices: [ { id, title, tag, summary, files[], new_files[], test_commands[], flag, risk[], importance, depends_on[], decisions_defaults, remaining, notes } ] }
const REPO = 'C:\\dev\\CoresSystem\\.claude\\worktrees\\epic-shaw-4d613b'
const PLAN = REPO + '\\docs\\plans\\model-plane-chat-build-plan-2026-10-14.md'
const NOTES = REPO + '\\.plan-work\\execution'

const WS_LINES = {
  WS01: [338, 381], WS02: [382, 422], WS03: [423, 461], WS04: [462, 509], WS05: [510, 546], WS06: [547, 586],
  WS07: [587, 626], WS08: [627, 671], WS09: [672, 715], WS10: [716, 755], WS11: [756, 797], WS12: [798, 846],
  WS13: [847, 886], WS14: [887, 923], WS15: [924, 961], WS16: [962, 1004], WS17: [1005, 1052], WS18: [1053, 1090],
  WS19: [1091, 1134], WS20: [1135, 1183], WS21: [1184, 1225], WS22: [1226, 1265], WS23: [1266, 1306],
  WS24: [1307, 1352], WS25: [1353, 1398], WS26: [1399, 1440], WS27: [1441, 1485],
}

const TIER = {
  haiku: { model: 'haiku', effort: 'max' },
  sonnet: { model: 'sonnet', effort: 'high' },
  opus: { model: 'opus', effort: 'high' },
}

const RESULT = {
  type: 'object',
  properties: {
    id: { type: 'string' },
    status: { type: 'string', enum: ['done', 'partial', 'blocked'] },
    files_changed: { type: 'array', items: { type: 'string' } },
    tests_run: { type: 'array', items: { type: 'object', properties: { command: { type: 'string' }, outcome: { type: 'string' } }, required: ['command', 'outcome'] } },
    db_tests_written: { type: 'array', items: { type: 'string' }, description: 'ignored DB tests the lead must run through the harness, as filters' },
    lead_todo: { type: 'array', items: { type: 'string' }, description: 'what only the lead can do: docker builds, deploys, live proofs, buf regeneration, harness count update' },
    deviations: { type: 'array', items: { type: 'string' }, description: 'where you departed from the plan row and why' },
    needs_other_files: { type: 'array', items: { type: 'string' }, description: 'files outside your ownership you would have had to edit' },
    notes: { type: 'string' },
  },
  required: ['id', 'status', 'files_changed', 'tests_run', 'notes'],
}
const VERDICT = {
  type: 'object',
  properties: {
    id: { type: 'string' },
    verdict: { type: 'string', enum: ['pass', 'fix'] },
    required_fixes: { type: 'array', items: { type: 'string' } },
    dod_gaps: { type: 'array', items: { type: 'string' } },
    tests_rerun: { type: 'array', items: { type: 'object', properties: { command: { type: 'string' }, outcome: { type: 'string' } }, required: ['command', 'outcome'] } },
    notes: { type: 'string' },
  },
  required: ['id', 'verdict', 'required_fixes'],
}
const REVIEW = {
  type: 'object',
  properties: {
    id: { type: 'string' },
    defects: { type: 'array', items: { type: 'object', properties: { severity: { type: 'string', enum: ['critical', 'high', 'medium', 'low'] }, where: { type: 'string' }, scenario: { type: 'string' }, fix: { type: 'string' } }, required: ['severity', 'where', 'scenario'] } },
    notes: { type: 'string' },
  },
  required: ['id', 'defects'],
}

const RULES = `Rules that always apply (they come from the repository CLAUDE.md files and the owner):
- Work only inside ${REPO} (a git worktree on branch model-plane). Never touch C:/dev/CoresSystem (the live tree). The C: drive is nearly full: never create a separate CARGO_TARGET_DIR or a copy of the Rust workspace (agents' scratch targets reached 30 GB); use the shared target directory and set CARGO_INCREMENTAL=0 on every cargo command (the shared incremental cache reached 36 GB), and never build a copy of HEAD or of the workspace to compare failing sets (say which failures are in files you did not touch instead), and if you truly need an isolated copy delete it before you finish. Never push, never open a pull request, never commit, never stage, never run git reset/checkout/clean/stash. The lead commits.
- NEVER run docker, docker compose, compose.sh, the DB test harness (scripts/tests/session-core-db-tests.sh) or any live proof: the lead runs those. Write DB tests in the repository's existing ignored-test pattern and list them in db_tests_written. You MAY run cargo, go, pnpm and python test commands. Windows: run cargo as "cargo test -q -p <crate> --lib" (inference-core, model-gateway and execution-core have pre-existing non-lib target failures); session-core is a binary crate: "cargo test -q -p session-core --bin session-core <filter>". Go: from the service directory, "go test ./..." and "go vet ./..." with GOFLAGS unset (build per service, no go.work at the root).
- Never read, print, write or change a credential, token, key or .env value. Never add a secret to a file. Never use a ChatGPT or other provider connection.
- Every new switch defaults OFF and is read through the repo's strict parser pattern (only the literal "true" enables); with every switch off behaviour must be byte-identical to before. Proto changes are ADDITIVE ONLY, at the tag the plan's section 4.3 reserves; migrations take the number section 4.4 reserves and ship a revert file.
- Architecture: no direct database access across planes; Rust for hot path, Go for durable control; one owner per capability (apps/Model Plane/docs/capability-ownership-matrix.md); check apps/Model Plane/docs/decisions/ledger.md before adding a registry or merging two things. ZDR must propagate through every content-persisting boundary: read the slice's ZDR clause in the plan and implement it.
- Edit existing source files with the Edit tool. Many files are CRLF in the working copy and some are LF: measure with python3 (open(p,'rb').read().count(b'\\r\\n') versus count(b'\\n')) and keep each file's style. Never run rustfmt or cargo fmt on a whole file that is CRLF without converting it back; never use shell heredocs with backslashes for edits (write a script file with the Write tool if you must script an edit).
- Match the surrounding code's naming, comment density and idiom; comments say why, not what; no emojis; no dead code; the smallest correct abstraction.
- Ownership: you may edit ONLY the files listed as owned plus new files the slice names plus test files beside what you change. If the slice needs any other file, do not edit it: list it in needs_other_files and do the rest.
- Keep your final report short and exact. Do not claim a test passed unless you ran it and saw it pass.`

const sliceBlock = (s) => {
  const ws = s.id.split('-')[0]
  const [a, b] = WS_LINES[ws] || [1, 1]
  return `Slice ${s.id} - ${s.title} (${s.tag}). Importance tier: ${s.importance}. Risk classes: ${(s.risk || []).join(', ') || 'none'}.
Summary: ${s.summary}
Remaining work (if the slice is partly built): ${s.remaining || 'the whole slice'}
Owned files (edit only these, plus the new files and test files named in the plan row):
${(s.files || []).map((f) => '  - ' + f).join('\n')}
New files the slice creates: ${(s.new_files || []).join(', ') || 'none'}
Flag the slice adds: ${s.flag || 'none'}
Plan tests to write and run: ${(s.test_commands || []).join(' ; ') || 'see the plan row'}
Decisions already taken by default (do not ask the owner): ${JSON.stringify(s.decisions_defaults || {})}
Depends on (already done or in an earlier batch): ${(s.depends_on || []).join(', ') || 'nothing'}
The authoritative description is the slice row in the plan: read ${PLAN} lines ${a}-${b} (the ${ws} section: header, "Verified starting point", the slice table row for ${s.id}, "Provides / consumes", "ZDR clause and regression guards", "Top risks"), the Appendix B amendments (Grep for "Appendix B"; where a row there conflicts with the section the row wins), section 4.3 (proto tags) and 4.4 (migration numbers) if you touch a proto or a migration, and section 6.4 for the exact commands. Line numbers in the plan drift: find the code by its name. The ledger of what is already built is the "Build ledger" at the end of the plan. Notes folder (read-only for you): ${NOTES}.
${s.notes ? 'Extra instruction from the lead: ' + s.notes : ''}`
}

const implementPrompt = (s) =>
  `You are an engineer implementing one slice of the Model Plane and chat build plan, end to end, in this repository.\n${RULES}\n\n${sliceBlock(s)}\n\nFIRST check git diff and git status for your owned files: a previous attempt at this slice may have been cut off by a usage limit and left partial, possibly non-compiling edits there. If so, read what is there, keep what is correct, finish and fix it, and never revert or discard it wholesale. Then do the slice fully: the code, the tests the plan names (write them first where practical), run them, fix failures. When the slice's definition of done needs a rebuilt image, a deploy or a live proof, do not fake it: do the code and tests, and put the exact lead-run steps in lead_todo. If the slice cannot be built without an external input (a credential, a key ceremony, an external contract, a decision only the owner can take), build everything that does not need it, set status "partial" or "blocked" and say exactly what is missing.`

const verifyPrompt = (s, impl) =>
  `You are an independent verifier of one slice, with no stake in the implementation. Repository ${REPO}. Never edit source files (you may only write nothing at all); you may run cargo, go, pnpm and python and read-only git commands (git status --short, git diff, git diff --stat, git log). Never run docker. Work only inside the worktree; never commit, push or touch the live tree; never read or print a credential.\n\n${sliceBlock(s)}\n\nThe implementer reports: ${JSON.stringify(impl)}\n\nVerify against the plan row's definition of done: (1) read the actual diff of the owned files and the new files (git status --short shows untracked files) and judge it against the slice row; (2) re-run the plan's test commands yourself and report real outcomes; (3) check every item of the rules above that applies: switches default off and use the strict parser, behaviour identical with switches off, additive proto at the reserved tag, migration number and revert file, ZDR clause implemented, no cross-plane database access, no secret, no edit outside the ownership set (compare git status with the owned list), line endings preserved (no whole-file CRLF to LF flips: git diff --stat must not show a rewrite of a file for a small change), no dead code; (4) look for correctness bugs and missing tests, not style. Verdict "pass" only if the slice is complete and correct at the level the plan demands; otherwise "fix" with a precise, minimal list of required_fixes (file, what, why). List dod_gaps for things only the lead can close (docker, live proof).`

const reviewPrompt = (s, impl) =>
  `You are a hostile reviewer. Slice ${s.id} touches ${(s.risk || []).join(', ')}. Your job is to BREAK it: find a way an attacker, a buggy caller, a concurrent request, a retry, a crash at the worst moment, or a tenant or user other than the owner could violate the guarantee the slice is supposed to give (authority, audit integrity, zero data retention, money, data loss). Repository ${REPO}. Read-only: never edit, never run docker; you may run cargo, go, pnpm and read-only git. Work only inside the worktree; never commit, push or touch the live tree; never read or print a credential.\n\n${sliceBlock(s)}\n\nImplementation report: ${JSON.stringify(impl)}\n\nRead the diff and the code around it (git status --short, git diff). Report only defects you can defend with a file, a line and a concrete failing scenario; an empty list is a valid answer. Severity critical or high only for a real violation of the slice's guarantee.`

const fixPrompt = (s, impl, fixes) =>
  `You previously implemented slice ${s.id}. An independent reviewer found problems. Fix exactly these, with the same rules, without widening the change, then re-run the tests.\n${RULES}\n\n${sliceBlock(s)}\n\nPrevious report: ${JSON.stringify(impl)}\n\nRequired fixes:\n${fixes.map((f, i) => (i + 1) + '. ' + f).join('\n')}\n\nReturn the same report shape (the full list of files changed by the slice, not only this round).`

// the implementer tier: an explicit s.implementer wins; otherwise haiku for mechanical slices and sonnet high for the rest (opus implements only when the lead says so)
const tierOf = (s) => TIER[s.implementer || (s.importance === 'haiku' ? 'haiku' : 'sonnet')] || TIER.sonnet

const slices = (args && args.slices) || []
phase('Implement')
const results = await pipeline(
  slices,
  // 1. implement
  async (s) => {
    const tier = tierOf(s)
    const impl = await agent(implementPrompt(s), { label: 'impl:' + s.id, phase: 'Implement', schema: RESULT, ...tier })
    return impl ? { s, impl } : null
  },
  // 2. verify with up to two fix rounds
  async (cur, s) => {
    if (!cur) return null
    let impl = cur.impl
    let verdict = null
    for (let round = 0; round < 3; round++) {
      verdict = await agent(verifyPrompt(s, impl), { label: 'verify:' + s.id + ':' + round, phase: 'Verify', schema: VERDICT, ...TIER.sonnet })
      if (!verdict || verdict.verdict === 'pass' || round === 2) break
      const tier = tierOf(s)
      const fixed = await agent(fixPrompt(s, impl, verdict.required_fixes), { label: 'fix:' + s.id + ':' + round, phase: 'Verify', schema: RESULT, ...tier })
      if (fixed) impl = fixed
    }
    return { s, impl, verdict }
  },
  // 3. hostile review for the risky slices only (opus)
  async (cur, s) => {
    if (!cur) return null
    const risky = s.review === 'opus'
    if (!risky) return { ...cur, review: null }
    const review = await agent(reviewPrompt(s, cur.impl), { label: 'review:' + s.id, phase: 'Review', schema: REVIEW, ...TIER.opus })
    let impl = cur.impl
    let verdict = cur.verdict
    const serious = review ? review.defects.filter((d) => d.severity === 'critical' || d.severity === 'high') : []
    if (serious.length) {
      const fixed = await agent(
        fixPrompt(s, impl, serious.map((d) => `${d.severity} at ${d.where}: ${d.scenario}${d.fix ? ' Fix: ' + d.fix : ''}`)),
        { label: 'fix-review:' + s.id, phase: 'Review', schema: RESULT, ...tierOf(s) },
      )
      if (fixed) {
        impl = fixed
        verdict = await agent(verifyPrompt(s, impl), { label: 'reverify:' + s.id, phase: 'Review', schema: VERDICT, ...TIER.sonnet })
      }
    }
    return { s, impl, verdict, review }
  },
)

const out = results.map((r, i) => {
  const s = slices[i]
  if (!r) return { id: s.id, outcome: 'failed', reason: 'an agent died or the slice returned nothing' }
  const passed = r.verdict && r.verdict.verdict === 'pass'
  return {
    id: s.id,
    outcome: passed && r.impl.status === 'done' ? 'ready' : (passed ? 'ready-partial' : 'needs-attention'),
    status: r.impl.status,
    files_changed: r.impl.files_changed,
    tests_run: r.impl.tests_run,
    db_tests_written: r.impl.db_tests_written || [],
    lead_todo: r.impl.lead_todo || [],
    deviations: r.impl.deviations || [],
    needs_other_files: r.impl.needs_other_files || [],
    verifier: r.verdict ? { verdict: r.verdict.verdict, required_fixes: r.verdict.required_fixes, dod_gaps: r.verdict.dod_gaps || [] } : null,
    review_defects: r.review ? r.review.defects : [],
    notes: r.impl.notes,
  }
})
log('batch ' + (args && args.batchId) + ': ' + out.map((o) => o.id + '=' + o.outcome).join(' '))
return { batchId: args && args.batchId, results: out }
