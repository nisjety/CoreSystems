/**
 * Tool narration (EP-14 S3): what a tool call did, said as one sentence in the active locale.
 *
 * Client-side and pure. A call is matched to a family by its name, or by its intent when its
 * result is a patch (`toolPresentation` already decides content over name). Its state comes
 * from the call and from the turn's retry notices. The sentence is the family's template for
 * that state, with the slots filled from the call's own arguments and result. No model writes
 * any of it, so nothing is sent and a ZDR turn needs no exception.
 *
 * The table is closed: its type requires every family in every state. A tool no template
 * names reads as the generic family, with its name as the object, so an MCP tool still gets
 * a sentence, just a plainer one.
 */

import type { ChatToolCall, LifecycleNotice } from '@/features/chat/components/chat-types'
import { toolPresentation, type ToolIntent, type ToolPresentation } from '@/shared/chat-nodes/tool-presentation'

/** The translator the app passes in (`useI18n().tr`), so one sentence is worded in one locale. */
export type Translator = { tr: (noText: string, enText: string) => string }

export type NarrationState = 'started' | 'done' | 'failed' | 'retrying' | 'awaiting_approval'

/** The verb a sentence uses, and where its object comes from. */
export type ToolFamily = 'search' | 'read' | 'command' | 'account' | 'change' | 'generic'

type NarrationKey = `${ToolFamily}.${NarrationState}`

/** Most characters an object may take, so one long query or path cannot stretch the row. */
const MAX_OBJECT_CHARS = 60
/** Most characters an outcome may take. An error is the tool's own words, so it gets more room. */
const MAX_OUTCOME_CHARS = 120

/**
 * The closed template table. Each entry names both locales inline, inside `i18n.tr`, which is
 * where the UI copy gate reads them. `{object}` and `{outcome}` are the slots. A template that
 * ends in ` - {outcome}` drops that clause when the call has no outcome to report.
 */
export const TEMPLATES: Record<NarrationKey, (i18n: Translator) => string> = {
  'search.started': (i18n) => i18n.tr("Søker etter '{object}'", "Searching for '{object}'"),
  'search.done': (i18n) => i18n.tr("Søkte etter '{object}' - {outcome}", "Searched for '{object}' - {outcome}"),
  'search.failed': (i18n) => i18n.tr("Søk etter '{object}' - feilet: {outcome}", "Search for '{object}' - failed: {outcome}"),
  'search.retrying': (i18n) => i18n.tr("Prøver søket etter '{object}' på nytt ({outcome})", "Retrying search for '{object}' ({outcome})"),
  'search.awaiting_approval': (i18n) => i18n.tr("Venter på godkjenning før søk etter '{object}'", "Waiting for approval to search for '{object}'"),

  'read.started': (i18n) => i18n.tr('Leser {object}', 'Reading {object}'),
  'read.done': (i18n) => i18n.tr('Leste {object} - {outcome}', 'Read {object} - {outcome}'),
  'read.failed': (i18n) => i18n.tr('Leste {object} - feilet: {outcome}', 'Read {object} - failed: {outcome}'),
  'read.retrying': (i18n) => i18n.tr('Prøver å lese {object} på nytt ({outcome})', 'Retrying reading {object} ({outcome})'),
  'read.awaiting_approval': (i18n) => i18n.tr('Venter på godkjenning før lesing av {object}', 'Waiting for approval to read {object}'),

  'command.started': (i18n) => i18n.tr('Kjører en kommando', 'Running a command'),
  'command.done': (i18n) => i18n.tr('Kjørte en kommando - {outcome}', 'Ran a command - {outcome}'),
  'command.failed': (i18n) => i18n.tr('Kjørte en kommando - feilet: {outcome}', 'Ran a command - failed: {outcome}'),
  'command.retrying': (i18n) => i18n.tr('Prøver kommandoen på nytt ({outcome})', 'Retrying the command ({outcome})'),
  'command.awaiting_approval': (i18n) => i18n.tr('Venter på godkjenning før en kommando kjøres', 'Waiting for approval to run a command'),

  'account.started': (i18n) => i18n.tr('Bruker {object}', 'Using {object}'),
  'account.done': (i18n) => i18n.tr('Brukte {object} - {outcome}', 'Used {object} - {outcome}'),
  'account.failed': (i18n) => i18n.tr('Brukte {object} - feilet: {outcome}', 'Used {object} - failed: {outcome}'),
  'account.retrying': (i18n) => i18n.tr('Prøver {object} på nytt ({outcome})', 'Retrying {object} ({outcome})'),
  'account.awaiting_approval': (i18n) => i18n.tr('Venter på godkjenning før bruk av {object}', 'Waiting for approval to use {object}'),

  'change.started': (i18n) => i18n.tr('Endrer {object}', 'Changing {object}'),
  'change.done': (i18n) => i18n.tr('Endret {object} - {outcome}', 'Changed {object} - {outcome}'),
  'change.failed': (i18n) => i18n.tr('Endret {object} - feilet: {outcome}', 'Changed {object} - failed: {outcome}'),
  'change.retrying': (i18n) => i18n.tr('Prøver å endre {object} på nytt ({outcome})', 'Retrying the change to {object} ({outcome})'),
  'change.awaiting_approval': (i18n) => i18n.tr('Venter på godkjenning før endring av {object}', 'Waiting for approval to change {object}'),

  'generic.started': (i18n) => i18n.tr('Bruker {object}', 'Using {object}'),
  'generic.done': (i18n) => i18n.tr('Brukte {object} - {outcome}', 'Used {object} - {outcome}'),
  'generic.failed': (i18n) => i18n.tr('Brukte {object} - feilet: {outcome}', 'Used {object} - failed: {outcome}'),
  'generic.retrying': (i18n) => i18n.tr('Prøver {object} på nytt ({outcome})', 'Retrying {object} ({outcome})'),
  'generic.awaiting_approval': (i18n) => i18n.tr('Venter på godkjenning før bruk av {object}', 'Waiting for approval to use {object}'),
}

/**
 * The family of each tool the platform names. Every name in the Do tool catalogue appears here,
 * so a new tool is a deliberate choice: a family, or `generic` written out. Names are lower case.
 */
export const FAMILY_BY_TOOL: Record<string, ToolFamily> = {
  // Result sets: the object is what was searched for.
  web_search: 'search',
  'web.search': 'search',
  knowledge_search: 'search',
  recall_memory: 'search',
  company_lookup: 'search',
  // One page or file: the object is its host, or its file name.
  web_fetch: 'read',
  'web.read': 'read',
  'browser.open': 'read',
  // Commands and processes: their text is not put in the sentence.
  code_interpreter: 'command',
  process_start: 'command',
  process_read: 'command',
  process_stdin: 'command',
  process_signal: 'command',
  process_list: 'command',
  // Connected accounts and services: the object is the account's label.
  list_provider_actions: 'account',
  execute_provider_action: 'account',
  list_social_accounts: 'account',
  publish_social_post: 'account',
  shipping_carriers: 'account',
  get_shipping_quotes: 'account',
  track_shipment: 'account',
  book_shipment: 'account',
  // Written out as generic: the tool's name is the object.
  yr_weather: 'generic',
  traffic: 'generic',
  news: 'generic',
  save_memory: 'generic',
  reattach_skill: 'generic',
  list_subagent_results: 'generic',
  read_subagent_result: 'generic',
  'subagent.task': 'generic',
  'subagent.research_fan_out': 'generic',
  'plan.propose': 'generic',
  'browser.observe': 'generic',
  'browser.close': 'generic',
}

/** The family a tool takes when no name above matches, read from the intent its result gives. */
const FAMILY_BY_INTENT: Record<ToolIntent, ToolFamily> = {
  terminal: 'command',
  search: 'search',
  read: 'read',
  diff: 'change',
  generic: 'generic',
}

export function toolFamily(name: string, intent: ToolIntent): ToolFamily {
  // Content wins over name, as it does in `toolPresentation`: a patch is a change whatever it is called.
  if (intent === 'diff') return 'change'
  const key = name.trim().toLowerCase()
  const explicit = Object.hasOwn(FAMILY_BY_TOOL, key) ? FAMILY_BY_TOOL[key] : undefined
  return explicit ?? FAMILY_BY_INTENT[intent]
}

const AWAITING_APPROVAL: ReadonlySet<string> = new Set(['awaiting_approval', 'waiting_approval'])

/** Still out: no status yet, or `running`. The same test `StepsPill` applies, so the two agree. */
const isOut = (call: ChatToolCall) => !call.status || call.status === 'running'

/** The failure test the activity line uses, so the strip and the rows agree on what failed. */
const isFailed = (call: ChatToolCall) => Boolean(call.error) || call.status === 'error' || call.status === 'failed'

/** The retry notice matched to this call, if the gateway retried it. */
function retryFor(call: ChatToolCall, notices: readonly LifecycleNotice[] | undefined) {
  return notices?.findLast((notice) => notice.kind === 'retry' && notice.callId === call.id)
}

export function narrationState(call: ChatToolCall, notices?: readonly LifecycleNotice[]): NarrationState {
  if (call.status !== undefined && AWAITING_APPROVAL.has(call.status)) return 'awaiting_approval'
  if (isOut(call)) return retryFor(call, notices) ? 'retrying' : 'started'
  return isFailed(call) ? 'failed' : 'done'
}

/** The argument keys each family reads its object from, first match wins. */
const SEARCH_KEYS = ['query', 'q', 'search', 'name', 'company'] as const
const ACCOUNT_KEYS = ['account_label', 'connection_label', 'account', 'provider'] as const
const FILE_KEYS = ['file_name', 'filename', 'file', 'path'] as const

/** The arguments as an object. A provider may send them as their JSON text. */
function argsObject(args: unknown): Record<string, unknown> | undefined {
  let value: unknown = args
  if (typeof value === 'string') {
    try {
      value = JSON.parse(value)
    } catch {
      return undefined
    }
  }
  return value !== null && typeof value === 'object' && !Array.isArray(value) ? (value as Record<string, unknown>) : undefined
}

function firstText(source: Record<string, unknown> | undefined, keys: readonly string[]): string | undefined {
  for (const key of keys) {
    const value = source?.[key]
    if (typeof value === 'string' && value.trim()) return value
  }
  return undefined
}

/** The host alone. A path or a query can carry a token, and a sentence does not need either. */
function hostOf(url: string): string | undefined {
  try {
    return new URL(url).hostname || undefined
  } catch {
    return undefined
  }
}

function fileOf(args: Record<string, unknown> | undefined): string | undefined {
  const path = firstText(args, FILE_KEYS)
  return path ? path.split(/[\\/]/).pop() : undefined
}

/**
 * The object a family's sentence names, read from the arguments the tool takes. A missing or
 * empty argument reads as no object, never as a guess.
 */
function objectOf(family: ToolFamily, call: ChatToolCall): string | undefined {
  const args = argsObject(call.args)
  if (family === 'search') return firstText(args, SEARCH_KEYS)
  if (family === 'account') return firstText(args, ACCOUNT_KEYS)
  if (family === 'change') return fileOf(args)
  if (family === 'read') {
    const url = firstText(args, ['url'])
    return (url ? hostOf(url) : undefined) ?? fileOf(args)
  }
  return undefined
}

/** Whitespace collapsed, and cut to `max` characters with an ellipsis. Counted by code point, so no pair is split. */
function capped(text: string, max: number): string {
  const flat = text.replace(/\s+/g, ' ').trim()
  const chars = Array.from(flat)
  return chars.length > max ? `${chars.slice(0, max - 1).join('')}…` : flat
}

/** A tool name read as words, for a sentence with no better object. */
function toolLabel(name: string): string {
  return (
    name
      .replace(/^mcp__/, '')
      .replace(/__/g, ' ')
      .replace(/[_.:-]+/g, ' ')
      .replace(/\s+/g, ' ')
      .trim()
      .toLowerCase() || name
  )
}

function countPhrase(count: number, i18n: Translator): string {
  if (count === 0) return i18n.tr('ingen resultater', 'no results')
  if (count === 1) return i18n.tr('1 resultat', '1 result')
  return i18n.tr(`${count} resultater`, `${count} results`)
}

function outcomeOf(
  state: NarrationState,
  family: ToolFamily,
  call: ChatToolCall,
  presentation: ToolPresentation,
  notices: readonly LifecycleNotice[] | undefined,
  i18n: Translator,
): string | undefined {
  if (state === 'done') {
    const parts: string[] = []
    if (presentation.count !== undefined) parts.push(countPhrase(presentation.count, i18n))
    if (family === 'change' && (presentation.added !== undefined || presentation.removed !== undefined)) {
      parts.push(`+${presentation.added ?? 0} −${presentation.removed ?? 0}`)
    }
    if (presentation.truncated) parts.push(i18n.tr('avkortet', 'truncated'))
    return parts.length > 0 ? parts.join(', ') : undefined
  }
  if (state === 'failed') {
    return capped(call.error?.trim() || i18n.tr('ingen detaljer', 'no details'), MAX_OUTCOME_CHARS)
  }
  if (state === 'retrying') {
    const retry = retryFor(call, notices)
    if (retry?.kind === 'retry') {
      return i18n.tr(`forsøk ${retry.attempt} av ${retry.maxAttempts}`, `attempt ${retry.attempt} of ${retry.maxAttempts}`)
    }
  }
  return undefined
}

/**
 * Fills the slots in one pass, so a value that spells a slot name is kept as typed. The replacer is a
 * function too, so a `$` in a query is not read as a replacement pattern.
 */
function fill(template: string, slots: { object: string; outcome?: string }): string {
  const text = slots.outcome === undefined ? template.replace(' - {outcome}', '') : template
  const values: Record<'object' | 'outcome', string> = { object: slots.object, outcome: slots.outcome ?? '' }
  return text.replace(/\{(object|outcome)\}/g, (_slot, name: keyof typeof values) => values[name])
}

/** One tool call as one sentence, in the locale `i18n` speaks. */
export function narrate(call: ChatToolCall, i18n: Translator, notices?: readonly LifecycleNotice[]): string {
  const presentation = toolPresentation(call)
  const family = toolFamily(call.name, presentation.intent)
  const state = narrationState(call, notices)
  const key: NarrationKey = `${family}.${state}`
  return fill(TEMPLATES[key](i18n), {
    object: capped(objectOf(family, call) ?? '', MAX_OBJECT_CHARS) || toolLabel(call.name),
    outcome: outcomeOf(state, family, call, presentation, notices, i18n),
  })
}
