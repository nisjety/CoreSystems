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

/**
 * How a tool call is drawn. `terminal` is a one-shot command. `browser`, `process` and `file` are the computer
 * panel's kinds (P1): a browser step, a process the agent started, and a workspace file.
 */
export type ToolIntent = 'terminal' | 'search' | 'read' | 'diff' | 'generic' | 'browser' | 'process' | 'file'

/**
 * How a `code_interpreter` run was confined, from its enforcement report (EP-9
 * E1). `isolated_no_network` is the only positive claim: the report must say the
 * process was wrapped, its network is `off`, and the sandbox enforced that in
 * full. Any other report, including a value this client does not know, is
 * `unknown` and is never shown as isolated.
 */
export type SandboxBadge = 'isolated_no_network' | 'unknown'

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
  /**
   * How the run was confined. Absent when the result carries no report, which is
   * every result while the report switch is off.
   */
  sandbox?: SandboxBadge
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
  // Reads matches out of the person's other conversations. Its JSON envelope names them `hits` and reports
  // no count, so the card shows the read without a number.
  'recall_conversations',
])

/**
 * The browser tools (`browser.*`). Their output is a page observation: the live browser view draws it, and a tool
 * card keeps the raw text.
 */
const BROWSER_TOOLS = new Set(['browser.open', 'browser.observe', 'browser.act', 'browser.close', 'browser.console'])

/**
 * The process host's tools: a long-running process the agent starts, reads from, writes to and signals. Their output
 * is a transcript, which the computer panel's terminal viewer draws.
 */
const PROCESS_TOOLS = new Set(['process_start', 'process_read', 'process_stdin', 'process_signal', 'process_list'])

/**
 * The workspace file tools (WS24): reading, searching, listing, editing and writing files in the thread's workspace.
 * Their output is file text or a listing, which the computer panel's file viewer draws.
 */
const FILE_TOOLS = new Set([
  'workspace_read',
  'workspace_grep',
  'workspace_glob',
  'workspace_list',
  'workspace_edit',
  'workspace_put',
])

function intentFor(name: string): ToolIntent {
  const normalized = name.trim().toLowerCase()
  if (TERMINAL_TOOLS.has(normalized)) return 'terminal'
  if (PROCESS_TOOLS.has(normalized)) return 'process'
  if (BROWSER_TOOLS.has(normalized)) return 'browser'
  if (FILE_TOOLS.has(normalized)) return 'file'
  if (SEARCH_TOOLS.has(normalized)) return 'search'
  if (READ_TOOLS.has(normalized)) return 'read'
  // Anything else — action tools, MCP tools, subagents — renders generically.
  // Guessing an intent from an unknown name would mis-shape third-party MCP
  // output, which is exactly where a wrong card is most confusing.
  return 'generic'
}

/** Parse the tool's own JSON envelope, if it emitted one. */
export function envelope(output: string | undefined): Record<string, unknown> | null {
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

/**
 * Only the tool that confines the process may claim that it did. A third-party
 * tool that echoes a `sandbox` object must not earn the badge, so the report is
 * read from `code_interpreter` alone.
 */
function sandboxBadge(name: string, body: Record<string, unknown>): SandboxBadge | undefined {
  if (name.trim().toLowerCase() !== 'code_interpreter' || !('sandbox' in body)) return undefined
  const report = body.sandbox
  if (report && typeof report === 'object') {
    const { sandboxed, network, enforcement } = report as Record<string, unknown>
    if (sandboxed === true && network === 'off' && enforcement === 'full') {
      return 'isolated_no_network'
    }
  }
  return 'unknown'
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
    sandbox: sandboxBadge(call.name, body),
  }
}
