import { execFileSync } from 'node:child_process'
import { readFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import * as ts from 'typescript'
import { describe, expect, it } from 'vitest'

// The SPA's reads of the chat event payloads, checked against the Model Plane golden (WS12 S8).
//
// The golden (WS12 S7) lists the keys each ChatEvent carries and whether each one is always present.
// This test reads the native `switch (event.event)` in `dispatchEvent`, and the helpers the payload is
// handed to, and fails when a case reads a key the golden does not list, when a key the golden marks
// always present goes unread, or when an exception in the tables below stops being true.

const here = dirname(fileURLToPath(import.meta.url))
const GOLDEN_PATH = resolve(here, 'contracts/sse-event-payloads.json')
const CHAT_CLIENT_PATH = resolve(here, 'chat-client.ts')
const GENERATOR_PATH = resolve(here, '../../../scripts/gen-chat-event-types.mjs')

type GoldenEvent = {
  variants: string[]
  keys: Record<string, { types: string[]; presence: 'always' | 'optional' }>
}
type Golden = { events: Record<string, GoldenEvent> }

/** Case labels of the native switch that the golden does not describe, each with its reason. */
const TRANSPORT_EVENTS: Record<string, string> = {
  connected: 'the stream handshake, which carries no ChatEvent payload',
  chunk: 'a text delta on the transport',
  done: 'the stream terminal',
  awaiting_approval: 'an orchestration event with no golden payload yet',
  citations: 'a batched citation list; each item is read as a citation',
  search_results: 'the batched citation list under its older name, read through `results`',
}

/** Keys a case reads that the golden does not list: camelCase spellings, older names, the BFF error envelope. */
const LEGACY_READ_ALIASES: Record<string, readonly string[]> = {
  attachment: ['mime'],
  citation: [
    'href', 'description', 'claimId', 'sourceGroupId', 'sourceGroupLabel',
    'start_offset', 'startOffset', 'end_offset', 'endOffset',
  ],
  error: ['error', 'run_id', 'runId', 'requestId', 'retryAfterMs', 'resetsAt'],
  grounding: ['grounding'],
  step_update: ['step_id', 'name', 'message'],
  tool_call: ['tool_call_id', 'call_id', 'tool', 'function_name', 'arguments'],
  tool_result: ['tool_call_id', 'call_id', 'result', 'data'],
  usage: ['cacheReadTokens', 'cacheWriteTokens'],
}

/** The entry count of LEGACY_READ_ALIASES. Aliases only shrink: removing one lowers this pin, adding one is a reviewed change. */
const LEGACY_READ_ALIAS_COUNT = 31

/** Always-present keys a case does not read itself, each with the reason, or the module that reads it instead. */
const UNUSED_KEYS: Record<string, readonly string[]> = {
  // The request id is not shown on these frames.
  reasoning_delta: ['request_id'],
  queued_input: ['request_id'],
  memory_recall: ['request_id'],
  verification: ['request_id'],
  // The whole frame is handed to the grounding normaliser, which reads these (chat-normalizers.ts).
  grounding: ['factCount', 'facts', 'lowConfidence', 'mode', 'query', 'sourceCount', 'sources'],
  // The whole frame is handed to run-park.ts's runParkedFrameOf, which reads and validates these (run-park.test.ts).
  run_parked: ['reason', 'request_id', 'resume_after', 'run_id', 'status'],
}

/** The tables `violationsOf` checks against; a test may pass its own to exercise a finding. */
type Tables = {
  transport: Record<string, string>
  aliases: Record<string, readonly string[]>
  aliasCount: number
  unused: Record<string, readonly string[]>
}

const TABLES: Tables = {
  transport: TRANSPORT_EVENTS,
  aliases: LEGACY_READ_ALIASES,
  aliasCount: LEGACY_READ_ALIAS_COUNT,
  unused: UNUSED_KEYS,
}

/** Helpers in chat-client.ts that read the payload they are given; their reads belong to the case that calls them. */
const READING_HELPERS: readonly string[] = ['emitCitation', 'failureTiming', 'failureFeature', 'parseTurnTiming', 'usageProgressFrame', 'sourceScopeAppliedEvent']
/** Helpers that read the payload in another module, which that module's own tests cover. */
const READ_ELSEWHERE: readonly string[] = ['servedByField', 'runParkedFrameOf']

type CaseReads = {
  /** Keys read from the payload, directly or through the helpers it is handed to. */
  keys: Set<string>
  /** Helpers handed the payload that this checker does not follow. */
  unchecked: string[]
}

function visit(node: ts.Node, visitor: (node: ts.Node) => void): void {
  visitor(node)
  ts.forEachChild(node, (child) => {
    visit(child, visitor)
  })
}

function unwrap(expression: ts.Expression): ts.Expression {
  let current = expression
  while (ts.isAsExpression(current) || ts.isParenthesizedExpression(current) || ts.isNonNullExpression(current)) {
    current = current.expression
  }
  return current
}

function isNamed(expression: ts.Expression, names: ReadonlySet<string>): boolean {
  return ts.isIdentifier(expression) && names.has(expression.text)
}

/** `payload`, and any local bound to it, as in `const usage = payload as ...`. */
function payloadNames(nodes: readonly ts.Node[]): Set<string> {
  const names = new Set(['payload'])
  let grew = true
  while (grew) {
    grew = false
    for (const node of nodes) {
      visit(node, (child) => {
        if (
          ts.isVariableDeclaration(child) &&
          ts.isIdentifier(child.name) &&
          child.initializer &&
          isNamed(unwrap(child.initializer), names) &&
          !names.has(child.name.text)
        ) {
          names.add(child.name.text)
          grew = true
        }
      })
    }
  }
  return names
}

function bodyOf(file: ts.SourceFile, name: string): ts.Block {
  for (const statement of file.statements) {
    if (ts.isFunctionDeclaration(statement) && statement.name?.text === name && statement.body) return statement.body
  }
  throw new Error(`chat-client.ts has no function ${name} with a body`)
}

type CaseGroup = { labels: string[]; statements: ts.Statement[] }

/** The cases of `dispatchEvent`'s switch on `event.event`, consecutive labels sharing the body that follows them. */
function caseGroups(file: ts.SourceFile): CaseGroup[] {
  const switches: ts.SwitchStatement[] = []
  visit(bodyOf(file, 'dispatchEvent'), (node) => {
    if (ts.isSwitchStatement(node) && node.expression.getText(file) === 'event.event') switches.push(node)
  })
  const sw = switches.length === 1 ? switches[0] : undefined
  if (!sw) throw new Error('dispatchEvent must hold exactly one switch on event.event')

  const groups: CaseGroup[] = []
  let labels: string[] = []
  for (const clause of sw.caseBlock.clauses) {
    if (!ts.isCaseClause(clause)) continue
    if (!ts.isStringLiteral(clause.expression)) {
      throw new Error(`a case of dispatchEvent is not a string literal: ${clause.expression.getText(file)}`)
    }
    labels.push(clause.expression.text)
    if (clause.statements.length > 0) {
      groups.push({ labels, statements: [...clause.statements] })
      labels = []
    }
  }
  return groups
}

/** The keys `nodes` read from the payload, following the reading helpers they hand it to. */
function readsThrough(file: ts.SourceFile, nodes: readonly ts.Node[], followed = new Set<string>()): CaseReads {
  const names = payloadNames(nodes)
  const keys = new Set<string>()
  const unchecked: string[] = []
  const calls: string[] = []
  for (const node of nodes) {
    visit(node, (child) => {
      if (ts.isPropertyAccessExpression(child) && isNamed(child.expression, names)) {
        keys.add(child.name.text)
      } else if (
        ts.isElementAccessExpression(child) &&
        isNamed(child.expression, names) &&
        ts.isStringLiteral(child.argumentExpression)
      ) {
        keys.add(child.argumentExpression.text)
      } else if (ts.isCallExpression(child) && child.arguments.some((arg) => isNamed(arg, names))) {
        calls.push(ts.isIdentifier(child.expression) ? child.expression.text : child.expression.getText(file))
      }
    })
  }
  for (const callee of calls) {
    if (READ_ELSEWHERE.includes(callee)) continue
    if (!READING_HELPERS.includes(callee)) {
      unchecked.push(callee)
      continue
    }
    // Followed once per case: a second call adds nothing, and a helper that calls itself stops here.
    if (followed.has(callee)) continue
    followed.add(callee)
    const helper = readsThrough(file, [bodyOf(file, callee)], followed)
    for (const key of helper.keys) keys.add(key)
    unchecked.push(...helper.unchecked)
  }
  return { keys, unchecked }
}

/** The reads of every case label `dispatchEvent` handles. */
function readModel(source: string): Map<string, CaseReads> {
  const file = ts.createSourceFile(CHAT_CLIENT_PATH, source, ts.ScriptTarget.Latest, true)
  const model = new Map<string, CaseReads>()
  for (const group of caseGroups(file)) {
    const reads = readsThrough(file, group.statements)
    for (const label of group.labels) model.set(label, reads)
  }
  return model
}

/** Every way the SPA's reads disagree with the golden and the tables, as readable findings. */
function violationsOf(source: string, golden: Golden, tables: Tables = TABLES): string[] {
  const model = readModel(source)
  const findings: string[] = []
  const isGolden = (name: string) => Object.hasOwn(golden.events, name)
  const isTransport = (name: string) => Object.hasOwn(tables.transport, name)

  for (const label of model.keys()) {
    if (!isGolden(label) && !isTransport(label)) {
      findings.push(`case '${label}' is neither a golden event nor a transport event`)
    }
  }
  for (const label of Object.keys(tables.transport)) {
    if (!model.has(label)) findings.push(`transport event '${label}' is no longer a case of dispatchEvent`)
    if (isGolden(label)) findings.push(`transport event '${label}' is now a golden event`)
  }

  for (const [event, spec] of Object.entries(golden.events)) {
    const reads = model.get(event)
    if (!reads) {
      findings.push(`the golden describes '${event}', which dispatchEvent does not handle`)
      continue
    }
    const aliases: readonly string[] = tables.aliases[event] ?? []
    const unused: readonly string[] = tables.unused[event] ?? []

    for (const key of reads.keys) {
      if (!Object.hasOwn(spec.keys, key) && !aliases.includes(key)) {
        findings.push(`${event} reads '${key}', which the golden does not list`)
      }
    }
    for (const callee of reads.unchecked) {
      findings.push(`${event} passes the payload to '${callee}', which the checker does not follow`)
    }
    for (const alias of aliases) {
      if (!reads.keys.has(alias)) findings.push(`${event} lists '${alias}' as an alias but does not read it`)
      if (Object.hasOwn(spec.keys, alias)) findings.push(`${event} lists '${alias}' as an alias but the golden lists it`)
    }
    for (const [key, entry] of Object.entries(spec.keys)) {
      if (entry.presence !== 'always') continue
      const isUnused = unused.includes(key)
      if (!reads.keys.has(key) && !isUnused) findings.push(`${event} does not read the always-present key '${key}'`)
      if (reads.keys.has(key) && isUnused) findings.push(`${event} reads '${key}', so it is not unused`)
    }
    for (const key of unused) {
      if (spec.keys[key]?.presence !== 'always') {
        findings.push(`${event} lists '${key}' as unused, but the golden does not always send it`)
      }
    }
  }

  for (const event of new Set([...Object.keys(tables.aliases), ...Object.keys(tables.unused)])) {
    if (!isGolden(event) || !model.has(event)) {
      findings.push(`'${event}' is named in the tables but dispatchEvent does not handle it as a golden event`)
    }
  }
  const aliasTotal = Object.values(tables.aliases).reduce((sum, list) => sum + list.length, 0)
  if (aliasTotal !== tables.aliasCount) {
    findings.push(`the legacy read aliases number ${aliasTotal}, pinned at ${tables.aliasCount}`)
  }
  return findings
}

function goldenEvent(golden: Golden, name: string): GoldenEvent {
  const event = golden.events[name]
  if (!event) throw new Error(`the golden has no event ${name}`)
  return event
}

describe('the SPA reads of the chat event payloads', () => {
  const golden = JSON.parse(readFileSync(GOLDEN_PATH, 'utf8')) as Golden
  const source = readFileSync(CHAT_CLIENT_PATH, 'utf8')

  it('reads exactly the keys the golden lists, with the reviewed aliases and exceptions', () => {
    expect(violationsOf(source, golden)).toEqual([])
  })

  it('reports a read renamed in a case block', () => {
    const renamed = source.replace(
      "handlers.onReasoning?.({ delta: str(payload.delta) ?? '' })",
      "handlers.onReasoning?.({ delta: str(payload.text) ?? '' })",
    )
    expect(renamed).not.toBe(source)
    expect(violationsOf(renamed, golden)).toEqual(
      expect.arrayContaining([
        "reasoning_delta reads 'text', which the golden does not list",
        "reasoning_delta does not read the always-present key 'delta'",
      ]),
    )
  })

  it('reports a citation number read under a key the golden does not list', () => {
    const renamed = source.replace('num(payload.number)', 'num(payload.citation_number)')
    expect(renamed).not.toBe(source)
    expect(violationsOf(renamed, golden)).toEqual([
      "citation reads 'citation_number', which the golden does not list",
    ])
  })

  it('reports a golden key renamed while the SPA still reads the old name', () => {
    const renamed = JSON.parse(JSON.stringify(golden)) as Golden
    const reasoning = goldenEvent(renamed, 'reasoning_delta')
    reasoning.keys = Object.fromEntries(
      Object.entries(reasoning.keys).map(([key, entry]) => [key === 'delta' ? 'text' : key, entry]),
    )
    expect(violationsOf(source, renamed)).toEqual(
      expect.arrayContaining([
        "reasoning_delta reads 'delta', which the golden does not list",
        "reasoning_delta does not read the always-present key 'text'",
      ]),
    )
  })

  it('reports a payload handed to a helper the checker does not follow', () => {
    const changed = source.replace(
      "handlers.onReasoning?.({ delta: str(payload.delta) ?? '' })",
      "handlers.onReasoning?.({ delta: readDelta(payload) ?? '' })",
    )
    expect(violationsOf(changed, golden)).toEqual(
      expect.arrayContaining(["reasoning_delta passes the payload to 'readDelta', which the checker does not follow"]),
    )
  })

  it('follows a helper that a case calls twice without a finding', () => {
    const twice = source.replace('...failureTiming(payload),', '...failureTiming(payload),\n        ...failureTiming(payload),')
    expect(twice).not.toBe(source)
    expect(violationsOf(twice, golden)).toEqual([])
  })

  it('reports an alias the case no longer reads, once the pin is kept in step', () => {
    const tables: Tables = {
      ...TABLES,
      aliases: { ...TABLES.aliases, usage: [...(TABLES.aliases.usage ?? []), 'ghost'] },
      aliasCount: TABLES.aliasCount + 1,
    }
    expect(violationsOf(source, golden, tables)).toEqual([
      "usage lists 'ghost' as an alias but does not read it",
    ])
  })

  it('the generated payload types are the generator output for the vendored golden', () => {
    expect(() =>
      execFileSync(process.execPath, [GENERATOR_PATH, '--check'], { stdio: 'pipe' }),
    ).not.toThrow()
  })
})
