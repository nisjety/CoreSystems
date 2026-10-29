/**
 * Tool presentation as **recomputed intent**.
 *
 * A tool call is stored as `{ id, name, args, status, output, error }` — never
 * with a chosen presentation. How to show it is derived here, at render time,
 * from the call itself.
 *
 * # Why never persisted
 *
 * If a card style were stored on the turn, changing this mapping would only
 * affect new conversations: every existing transcript would keep rendering the
 * old style forever, and a tool whose output shape changed would keep a card
 * that no longer fits it. Recomputing means one function decides for all
 * history, and it is a pure function of data the transcript already holds — so
 * `toolPresentation` is safe to call on the render path and needs no migration.
 *
 * # `diff` is detected from the OUTPUT, not the tool name
 *
 * No tool this platform ships produces a patch — there is no file-editing tool,
 * and no tool result carries before/after state. A name-keyed `diff` branch
 * would therefore be dead code, which is the pattern the adoption plan rejects
 * (§13.3).
 *
 * But third-party MCP tools DO return unified diffs (a git server, a codemod
 * runner), and those arrive under names we have never seen. So the `diff` intent
 * is decided by looking at whether the output *is* a patch. That is the intent
 * being genuinely recomputed rather than looked up, it is not dead on arrival,
 * and it works for tools nobody here registered.
 *
 * Detection is deliberately strict (a real `@@ -a,b +c,d @@` hunk header): `+`
 * and `-` line prefixes alone match prose, markdown lists and log output, and a
 * wrong diff card is more confusing than a generic one.
 */

import type { ChatToolCall } from '@/features/chat/components/chat-types'
import { looksLikeUnifiedDiff, parseUnifiedDiff } from './diff'

export type ToolIntent = 'terminal' | 'search' | 'read' | 'diff' | 'generic'

export type ToolPresentation = {
  intent: ToolIntent
  /** Result count, when the output reports one. */
  count?: number
  /**
   * The output was cut short by the tool itself.
   *
   * Distinct from "no results": a truncated read means there IS more and the
   * model did not see it, which is worth showing next to an answer built on it.
   */
  truncated?: boolean
  /** A tool-reported status string (`ok`, `no_results`, `low_confidence`, …). */
  outcome?: string
  /** Lines added, for a `diff` intent. */
  added?: number
  /** Lines removed, for a `diff` intent. */
  removed?: number
}

/**
 * Tools that run code or commands. Their output is a terminal transcript and
 * belongs in a monospace block, not prose.
 */
const TERMINAL_TOOLS = new Set(['shell', 'code_interpreter'])

/**
 * Tools that answer with a result SET. What matters is how many came back and
 * whether the list was cut — not the raw payload.
 */
const SEARCH_TOOLS = new Set([
  'web_search',
  'web.search',
  'knowledge_search',
  'company_lookup',
  'list_provider_actions',
  'list_social_accounts',
  'shipping_carriers',
  'get_shipping_quotes',
])

/**
 * Tools that fetch ONE thing. What matters is what was read and whether it was
 * complete.
 */
const READ_TOOLS = new Set([
  'web_fetch',
  'web.read',
  'yr_weather',
  'traffic',
  'news',
  'track_shipment',
  'recall_memory',
  // Reads earlier messages of THIS conversation back out of the durable thread
  // after compaction dropped them from the prompt. Its output is rendered text,
  // not a JSON envelope, so no count or outcome is surfaced — but a read is
  // still what it is, and 'generic' would say less than we know.
  'reattach_context',
])

function intentFor(name: string): ToolIntent {
  const normalized = name.trim().toLowerCase()
  if (TERMINAL_TOOLS.has(normalized)) return 'terminal'
  if (SEARCH_TOOLS.has(normalized)) return 'search'
  if (READ_TOOLS.has(normalized)) return 'read'
  // Anything else — action tools, MCP tools, subagents — renders generically.
  // Guessing an intent from an unknown name would mis-shape third-party MCP
  // output, which is exactly where a wrong card is most confusing.
  return 'generic'
}

/** Parse the tool's own JSON envelope, if it emitted one. */
function envelope(output: string | undefined): Record<string, unknown> | null {
  if (!output) return null
  const trimmed = output.trim()
  if (!trimmed.startsWith('{')) return null
  try {
    const parsed: unknown = JSON.parse(trimmed)
    return parsed && typeof parsed === 'object' ? (parsed as Record<string, unknown>) : null
  } catch {
    // Not JSON, or truncated mid-object. Either way there is nothing to read
    // and the generic body still shows the raw text.
    return null
  }
}

function numberField(source: Record<string, unknown>, ...keys: string[]): number | undefined {
  for (const key of keys) {
    const value = source[key]
    if (typeof value === 'number' && Number.isFinite(value)) return value
  }
  return undefined
}

export function toolPresentation(call: ChatToolCall): ToolPresentation {
  // Content wins over name: a tool that returned a patch gets the diff card
  // whatever it is called, including tools registered by an MCP server this
  // client has never heard of.
  if (call.output && looksLikeUnifiedDiff(call.output)) {
    const { stat } = parseUnifiedDiff(call.output)
    return { intent: 'diff', added: stat.added, removed: stat.removed }
  }
  const intent = intentFor(call.name)
  const body = envelope(call.output)
  if (!body) return { intent }

  const outcome = typeof body.status === 'string' ? body.status : undefined
  // `count` and `result_count` are the two spellings the backends emit
  // (model-gateway's builtin tools and execution-core's knowledge tools
  // respectively). A `results` array is the fallback when neither is reported.
  const reported = numberField(body, 'count', 'result_count', 'total')
  const results = Array.isArray(body.results) ? body.results.length : undefined
  const truncated = typeof body.truncated === 'boolean' ? body.truncated : undefined

  return {
    intent,
    count: reported ?? results,
    truncated,
    outcome,
  }
}
