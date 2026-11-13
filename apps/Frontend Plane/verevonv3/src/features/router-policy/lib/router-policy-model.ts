import {
  ROUTER_POLICY_COMPLEXITIES,
  ROUTER_POLICY_MODES,
  type RouterPolicyComplexity,
  type RouterPolicyComplexityConfig,
  type RouterPolicyMode,
  type RoutingPolicy,
} from '@/shared/api/router-policy-client'
import { VEREVON_MODES, type VerevonMode } from '@/shared/api/chat-client'

/** Map a routing-table mode key to its pinned Verevon mode metadata (label/order). */
const MODE_TO_VEREVON_ID: Record<RouterPolicyMode, string> = {
  budget: 'verevon-budget',
  balance: 'verevon-balance',
  genius: 'verevon-genius',
}

export interface RouterPolicyModeRow {
  mode: RouterPolicyMode
  label: string
  description: string
}

/** Mode rows in display order, labelled from the authoritative VEREVON_MODES list. */
export function routerPolicyModeRows(): RouterPolicyModeRow[] {
  return ROUTER_POLICY_MODES.map((mode) => {
    const verevon: VerevonMode | undefined = VEREVON_MODES.find((m) => m.id === MODE_TO_VEREVON_ID[mode])
    return {
      mode,
      label: verevon?.label ?? mode,
      description: verevon?.description ?? '',
    }
  })
}

export const ROUTER_POLICY_COMPLEXITY_LABELS: Record<RouterPolicyComplexity, string> = {
  simple: 'Simple',
  moderate: 'Moderate',
  complex: 'Complex',
}

/** A numeric complexity threshold/score field, paired with display metadata. */
export interface ComplexityFieldSpec {
  key: keyof Omit<RouterPolicyComplexityConfig, 'keywords'>
  label: string
  helpText: string
}

export const COMPLEXITY_FIELD_SPECS: readonly ComplexityFieldSpec[] = [
  { key: 'moderate_threshold', label: 'Moderate threshold', helpText: 'Score at or above which a request is at least moderate.' },
  { key: 'complex_threshold', label: 'Complex threshold', helpText: 'Score at or above which a request is complex.' },
  { key: 'large_total_chars', label: 'Total chars · high threshold', helpText: 'Character count that earns the high-length score.' },
  { key: 'large_total_chars_score', label: 'Total chars · high score', helpText: 'Score added for very long conversations.' },
  { key: 'medium_total_chars', label: 'Total chars · mid threshold', helpText: 'Character count that earns the mid-length score.' },
  { key: 'medium_total_chars_score', label: 'Total chars · mid score', helpText: 'Score added for moderately long conversations.' },
  { key: 'long_user_turn_chars', label: 'Last user chars · threshold', helpText: 'Length of the last user message that earns its score.' },
  { key: 'long_user_turn_score', label: 'Last user chars · score', helpText: 'Score added for a long final user message.' },
  { key: 'code_fence_score', label: 'Code fence score', helpText: 'Score added when the request contains code fences.' },
  { key: 'keyword_score', label: 'Keyword score', helpText: 'Score added when a complexity keyword is present.' },
  { key: 'tool_use_score', label: 'Tool-required score', helpText: 'Score added when tools are required.' },
  { key: 'deep_conversation_turns', label: 'Long conversation · turns threshold', helpText: 'Turn count that earns the long-conversation score.' },
  { key: 'deep_conversation_score', label: 'Long conversation · score', helpText: 'Score added for long multi-turn conversations.' },
]

/** A defensive, fully-populated policy used while the real one loads (or fails). */
export function emptyRoutingPolicy(): RoutingPolicy {
  const emptyRow = { simple: '', moderate: '', complex: '' }
  return {
    enabled: false,
    budget_cap_usd: 0,
    constrained_fraction: 0,
    cheap_fallback: '',
    complexity: {
      large_total_chars: 0,
      large_total_chars_score: 0,
      medium_total_chars: 0,
      medium_total_chars_score: 0,
      long_user_turn_chars: 0,
      long_user_turn_score: 0,
      code_fence_score: 0,
      keyword_score: 0,
      tool_use_score: 0,
      deep_conversation_turns: 0,
      deep_conversation_score: 0,
      moderate_threshold: 0,
      complex_threshold: 0,
      keywords: [],
    },
    table: {
      budget: { ...emptyRow },
      balance: { ...emptyRow },
      genius: { ...emptyRow },
    },
  }
}

/** Immutable update of a top-level numeric field. */
export function withTopLevelNumber(
  policy: RoutingPolicy,
  key: 'budget_cap_usd' | 'constrained_fraction',
  value: number,
): RoutingPolicy {
  return { ...policy, [key]: value }
}

/** Immutable update of the enabled flag. */
export function withEnabled(policy: RoutingPolicy, enabled: boolean): RoutingPolicy {
  return { ...policy, enabled }
}

/** Immutable update of the cheap fallback model id. */
export function withCheapFallback(policy: RoutingPolicy, cheapFallback: string): RoutingPolicy {
  return { ...policy, cheap_fallback: cheapFallback }
}

/** Immutable update of a single complexity numeric field. */
export function withComplexityNumber(
  policy: RoutingPolicy,
  key: ComplexityFieldSpec['key'],
  value: number,
): RoutingPolicy {
  return {
    ...policy,
    complexity: { ...policy.complexity, [key]: value },
  }
}

/** Immutable replacement of the keyword list. */
export function withKeywords(policy: RoutingPolicy, keywords: string[]): RoutingPolicy {
  return {
    ...policy,
    complexity: { ...policy.complexity, keywords: [...keywords] },
  }
}

/** Immutable update of a single routing-table cell (mode × complexity → model id). */
export function withTableCell(
  policy: RoutingPolicy,
  mode: RouterPolicyMode,
  complexity: RouterPolicyComplexity,
  modelId: string,
): RoutingPolicy {
  return {
    ...policy,
    table: {
      ...policy.table,
      [mode]: { ...policy.table[mode], [complexity]: modelId },
    },
  }
}

/** Parse a comma/newline-separated keyword input into a clean list. */
export function parseKeywords(raw: string): string[] {
  return raw
    .split(/[\n,]/)
    .map((keyword) => keyword.trim())
    .filter((keyword) => keyword.length > 0)
}

/** The side-task purposes the role table names, in the order the page lists them (the keys of `roles`). */
export const ROUTER_POLICY_ROLE_PURPOSES = ['title', 'follow_ups', 'verification_judge', 'research_plan'] as const
export type RouterPolicyRolePurpose = (typeof ROUTER_POLICY_ROLE_PURPOSES)[number]

/** A role names one to this many models. The platform refuses more, and a role with none is removed. */
export const ROUTER_POLICY_ROLE_MAX_CANDIDATES = 4

/** One purpose's entry: the models tried in order, and the effort the entry sets when it chooses the model. */
export interface RouterPolicyRoleEntry {
  candidates: string[]
  effort?: string
}

/** The side-task role table, keyed by purpose. The gateway leaves it out of the document while it is empty. */
export type RouterPolicyRoleTable = Partial<Record<RouterPolicyRolePurpose, RouterPolicyRoleEntry>>

/**
 * The document as the gateway returns it. A save is a full-document PUT, so the page edits a copy of it and sends
 * back every field it was given. The ladder, the role table, the catalogue overlay and the complexity flag are kept as
 * the server sent them unless the page edits them, and a field left out of a save is reset on the server.
 */
export type RouterPolicyDocument = RoutingPolicy & {
  tool_fallback_ladder?: string[]
  roles?: RouterPolicyRoleTable
  catalogue_overlay?: RouterPolicyCatalogueOverlay
  complexity: RouterPolicyComplexityConfig & { tool_use_floors_complex?: boolean }
}

/** The entry a purpose has, or `undefined` while the purpose names no models. */
export function roleEntryOf(
  document: RouterPolicyDocument,
  purpose: RouterPolicyRolePurpose,
): RouterPolicyRoleEntry | undefined {
  return document.roles?.[purpose]
}

/**
 * The document with one purpose's candidates replaced and its effort kept. An empty list removes the purpose, and a
 * table left with no purpose is removed too, so the body matches what the server writes for an empty table.
 */
export function withRoleCandidates(
  document: RouterPolicyDocument,
  purpose: RouterPolicyRolePurpose,
  candidates: readonly string[],
): RouterPolicyDocument {
  const table: RouterPolicyRoleTable = { ...document.roles }
  if (candidates.length === 0) delete table[purpose]
  else table[purpose] = { ...table[purpose], candidates: [...candidates] }
  const next: RouterPolicyDocument = { ...document, roles: table }
  if (Object.keys(table).length === 0) delete next.roles
  return next
}

/** The candidates with the one at `index` moved one place earlier (-1) or later (+1). A move off either end changes nothing. */
export function moveCandidate(candidates: readonly string[], index: number, step: -1 | 1): string[] {
  const next = [...candidates]
  const moved = next[index]
  const target = next[index + step]
  if (moved === undefined || target === undefined) return next
  next[index] = target
  next[index + step] = moved
  return next
}

/** One refusal of a role table, as inference-core names it under `details.issues` of an `invalid_role_table` error. */
export interface RouterPolicyRoleIssue {
  purpose: RouterPolicyRolePurpose
  reason: string
  model?: string
  effort?: string
}

/**
 * The refusals in an `invalid_role_table` body that name a purpose the page shows. Anything else is dropped. The
 * issues sit under `details.issues`; a bare array of them, as some upstreams send, is read the same way.
 */
export function roleIssuesFrom(details: unknown): RouterPolicyRoleIssue[] {
  const keyed =
    details && typeof details === 'object' && !Array.isArray(details) ? (details as Record<string, unknown>).issues : undefined
  const listed = Array.isArray(details) ? details : keyed
  if (!Array.isArray(listed)) return []
  const issues: RouterPolicyRoleIssue[] = []
  for (const item of listed as unknown[]) {
    if (!item || typeof item !== 'object') continue
    const record = item as Record<string, unknown>
    const purpose = ROUTER_POLICY_ROLE_PURPOSES.find((known) => known === record.purpose)
    if (purpose === undefined || typeof record.reason !== 'string') continue
    issues.push({
      purpose,
      reason: record.reason,
      ...(typeof record.model === 'string' ? { model: record.model } : {}),
      ...(typeof record.effort === 'string' ? { effort: record.effort } : {}),
    })
  }
  return issues
}

/**
 * The platform catalogue overlay (F-t3-065 phase 1), as the routing policy stores it: the models the listing hides,
 * the order the listing puts first, and the alias ids that stand for a listed model. The server reads an absent
 * overlay as an empty one, and so does the page.
 */
export interface RouterPolicyCatalogueOverlay {
  hidden: string[]
  order: string[]
  aliases: Record<string, string>
}

/** The most hidden ids, ordered ids and aliases the overlay may hold. inference-core refuses more. */
export const CATALOGUE_OVERLAY_MAX_HIDDEN = 200
export const CATALOGUE_OVERLAY_MAX_ORDER = 200
export const CATALOGUE_OVERLAY_MAX_ALIASES = 50

/** An alias id: one to 64 lower-case ASCII letters, digits, `.`, `_` or `-`. */
export const CATALOGUE_OVERLAY_ALIAS_PATTERN = /^[a-z0-9._-]{1,64}$/

/**
 * The ids the routing layer answers for itself: the empty and default ids, the automatic and Verevon modes, and the
 * side-task role. The overlay never hides, lists or aliases one. Matched after trimming and without regard to case,
 * as inference-core's `is_reserved_id` matches them.
 */
const ROUTING_LAYER_IDS: ReadonlySet<string> = new Set([
  '',
  'default',
  'auto',
  'verevon',
  'verevon-auto',
  'verevon-balance',
  'verevon-budget',
  'verevon-genius',
  'verevon-role',
])

/** Whether `id` is one the routing layer answers for itself (see `ROUTING_LAYER_IDS`). */
export function isRoutingLayerId(id: string): boolean {
  return ROUTING_LAYER_IDS.has(id.trim().toLowerCase())
}

/** The model-id form the server compares: without regard to case. */
function foldId(id: string): string {
  return id.toLowerCase()
}

function stringList(value: unknown): string[] {
  return Array.isArray(value) ? value.filter((item): item is string => typeof item === 'string') : []
}

function stringMap(value: unknown): Record<string, string> {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return {}
  return Object.fromEntries(
    Object.entries(value).filter((entry): entry is [string, string] => typeof entry[1] === 'string'),
  )
}

/**
 * The overlay the document holds. A value that is not the shape the server writes reads as empty, so a list or map
 * the page cannot read is never written back as something else.
 */
export function catalogueOverlayOf(document: object): RouterPolicyCatalogueOverlay {
  const raw: unknown = (document as { readonly catalogue_overlay?: unknown }).catalogue_overlay
  const record: Record<string, unknown> =
    raw && typeof raw === 'object' && !Array.isArray(raw) ? (raw as Record<string, unknown>) : {}
  return { hidden: stringList(record.hidden), order: stringList(record.order), aliases: stringMap(record.aliases) }
}

/** Whether the overlay changes nothing. A document leaves an empty overlay out, as the server writes it. */
export function isEmptyCatalogueOverlay(overlay: RouterPolicyCatalogueOverlay): boolean {
  return overlay.hidden.length === 0 && overlay.order.length === 0 && Object.keys(overlay.aliases).length === 0
}

/**
 * The document with its overlay replaced. An empty overlay is left out of the document. Every other field is kept as
 * it was, so a save writes back what the page does not edit.
 */
export function withCatalogueOverlay(
  document: RouterPolicyDocument,
  overlay: RouterPolicyCatalogueOverlay,
): RouterPolicyDocument {
  const next: RouterPolicyDocument = { ...document, catalogue_overlay: overlay }
  if (isEmptyCatalogueOverlay(overlay)) delete next.catalogue_overlay
  return next
}

/** The overlay with one model hidden, or shown again. Ids match without regard to case, as the server matches them. */
export function withOverlayHidden(
  overlay: RouterPolicyCatalogueOverlay,
  id: string,
  hidden: boolean,
): RouterPolicyCatalogueOverlay {
  const isThis = (item: string) => foldId(item) === foldId(id)
  if (!hidden) return { ...overlay, hidden: overlay.hidden.filter((item) => !isThis(item)) }
  if (overlay.hidden.some(isThis)) return overlay
  return { ...overlay, hidden: [...overlay.hidden, id] }
}

/** The overlay with its order replaced. The ids it names come first, in this order. */
export function withOverlayOrder(
  overlay: RouterPolicyCatalogueOverlay,
  order: readonly string[],
): RouterPolicyCatalogueOverlay {
  return { ...overlay, order: [...order] }
}

/** Whether the overlay already holds an alias with exactly this id. */
export function hasOverlayAlias(overlay: RouterPolicyCatalogueOverlay, alias: string): boolean {
  return Object.keys(overlay.aliases).includes(alias)
}

/** The overlay with an alias pointing at `target`. An alias that exists keeps its place; a new one goes last. */
export function withOverlayAliasTarget(
  overlay: RouterPolicyCatalogueOverlay,
  alias: string,
  target: string,
): RouterPolicyCatalogueOverlay {
  return { ...overlay, aliases: { ...overlay.aliases, [alias]: target } }
}

/** The overlay with an alias removed. */
export function withoutOverlayAlias(
  overlay: RouterPolicyCatalogueOverlay,
  alias: string,
): RouterPolicyCatalogueOverlay {
  return { ...overlay, aliases: Object.fromEntries(Object.entries(overlay.aliases).filter(([key]) => key !== alias)) }
}

/**
 * The overlay with an alias renamed in place, so its row keeps its place. A name another alias already holds is
 * refused: the overlay comes back unchanged, and the page says why.
 */
export function withRenamedOverlayAlias(
  overlay: RouterPolicyCatalogueOverlay,
  from: string,
  to: string,
): RouterPolicyCatalogueOverlay {
  if (from === to || hasOverlayAlias(overlay, to)) return overlay
  const aliases = Object.fromEntries(
    Object.entries(overlay.aliases).map(([key, target]) => (key === from ? [to, target] : [key, target])),
  )
  return { ...overlay, aliases }
}

/** The overlay with a new alias row, named blank until the person names it. Only one blank row can exist. */
export function withNewOverlayAlias(overlay: RouterPolicyCatalogueOverlay, target: string): RouterPolicyCatalogueOverlay {
  return hasOverlayAlias(overlay, '') ? overlay : withOverlayAliasTarget(overlay, '', target)
}

/** The lists of the overlay, as inference-core names them in a refusal. */
export type CatalogueOverlayList = 'hidden' | 'order' | 'aliases'

/**
 * One refusal of the overlay, as inference-core names it under `details.issues` of an `invalid_catalogue_overlay`
 * error. The page runs the same checks before it saves, so most refusals show on the row before a save is sent.
 */
export type CatalogueOverlayIssue =
  | { reason: 'too_many'; list: CatalogueOverlayList; count: number; max: number }
  | { reason: 'blank_id'; list: 'hidden' | 'order' }
  | { reason: 'reserved_id'; list: 'hidden' | 'aliases'; id: string }
  | { reason: 'malformed_alias'; alias: string }
  | { reason: 'shadows_model'; alias: string }
  | { reason: 'unknown_target'; alias: string; target: string }
  | { reason: 'hidden_target'; alias: string; target: string }

/**
 * Every way the overlay fails, in the order inference-core checks them. `listedIds` are the chat models an alias may
 * name and may not shadow, without the overlay's own alias entries. `undefined` means the listing is unavailable, and
 * the checks that need it are skipped rather than refusing every alias.
 */
export function catalogueOverlayIssues(
  overlay: RouterPolicyCatalogueOverlay,
  listedIds: readonly string[] | undefined,
): CatalogueOverlayIssue[] {
  const listed = listedIds === undefined ? undefined : new Set(listedIds.map(foldId))
  const hidden = new Set(overlay.hidden.map(foldId))
  const isHidden = (id: string) => hidden.has(foldId(id))
  const issues: CatalogueOverlayIssue[] = []
  const counts: Array<[CatalogueOverlayList, number, number]> = [
    ['hidden', overlay.hidden.length, CATALOGUE_OVERLAY_MAX_HIDDEN],
    ['order', overlay.order.length, CATALOGUE_OVERLAY_MAX_ORDER],
    ['aliases', Object.keys(overlay.aliases).length, CATALOGUE_OVERLAY_MAX_ALIASES],
  ]
  for (const [list, count, max] of counts) {
    if (count > max) issues.push({ reason: 'too_many', list, count, max })
  }
  for (const id of overlay.hidden) {
    if (id.trim() === '') issues.push({ reason: 'blank_id', list: 'hidden' })
    else if (isRoutingLayerId(id)) issues.push({ reason: 'reserved_id', list: 'hidden', id })
  }
  for (const id of overlay.order) {
    if (id.trim() === '') issues.push({ reason: 'blank_id', list: 'order' })
  }
  for (const [alias, target] of Object.entries(overlay.aliases)) {
    if (!CATALOGUE_OVERLAY_ALIAS_PATTERN.test(alias)) issues.push({ reason: 'malformed_alias', alias })
    else if (isRoutingLayerId(alias)) issues.push({ reason: 'reserved_id', list: 'aliases', id: alias })
    else if (isHidden(alias) || (listed?.has(foldId(alias)) ?? false)) issues.push({ reason: 'shadows_model', alias })
    else if (isHidden(target)) issues.push({ reason: 'hidden_target', alias, target })
    else if (listed !== undefined && !listed.has(foldId(target))) issues.push({ reason: 'unknown_target', alias, target })
  }
  return issues
}

const OVERLAY_LISTS: readonly CatalogueOverlayList[] = ['hidden', 'order', 'aliases']

function parseCatalogueOverlayIssue(item: unknown): CatalogueOverlayIssue | undefined {
  if (!item || typeof item !== 'object') return undefined
  const record = item as Record<string, unknown>
  const text = (key: string): string | undefined => (typeof record[key] === 'string' ? (record[key] as string) : undefined)
  const list = OVERLAY_LISTS.find((known) => known === record.list)
  const alias = text('alias')
  const target = text('target')
  const count = record.count
  const max = record.max
  switch (record.reason) {
    case 'too_many':
      return list !== undefined && typeof count === 'number' && typeof max === 'number'
        ? { reason: 'too_many', list, count, max }
        : undefined
    case 'blank_id':
      return list === 'hidden' || list === 'order' ? { reason: 'blank_id', list } : undefined
    case 'reserved_id': {
      const id = text('id')
      return (list === 'hidden' || list === 'aliases') && id !== undefined ? { reason: 'reserved_id', list, id } : undefined
    }
    case 'malformed_alias':
      return alias !== undefined ? { reason: 'malformed_alias', alias } : undefined
    case 'shadows_model':
      return alias !== undefined ? { reason: 'shadows_model', alias } : undefined
    case 'unknown_target':
      return alias !== undefined && target !== undefined ? { reason: 'unknown_target', alias, target } : undefined
    case 'hidden_target':
      return alias !== undefined && target !== undefined ? { reason: 'hidden_target', alias, target } : undefined
    default:
      return undefined
  }
}

/**
 * The refusals in an `invalid_catalogue_overlay` body. They sit under `details.issues`; a bare array of them is read the
 * same way. A refusal this page does not know is dropped.
 */
export function catalogueOverlayIssuesFrom(details: unknown): CatalogueOverlayIssue[] {
  const keyed =
    details && typeof details === 'object' && !Array.isArray(details) ? (details as Record<string, unknown>).issues : undefined
  const listed: unknown = Array.isArray(details) ? details : keyed
  if (!Array.isArray(listed)) return []
  return listed.flatMap((item: unknown) => {
    const issue = parseCatalogueOverlayIssue(item)
    return issue ? [issue] : []
  })
}

/**
 * The rows in the order the overlay sets: the ids it names come first, in its order, and every other row follows in
 * the order it had. The server sorts the listing the same way.
 */
export function inCatalogueOrder<T extends { readonly id: string }>(rows: readonly T[], order: readonly string[]): T[] {
  const rank = new Map<string, number>()
  order.forEach((id, position) => {
    const key = foldId(id)
    if (!rank.has(key)) rank.set(key, position)
  })
  const rankOf = (row: T) => rank.get(foldId(row.id)) ?? Number.MAX_SAFE_INTEGER
  return rows
    .map((row, position) => ({ row, position, rank: rankOf(row) }))
    .sort((left, right) => left.rank - right.rank || left.position - right.position)
    .map(({ row }) => row)
}

/**
 * The listed models without the entries the listing adds for the overlay's aliases. An alias lists as a copy of its
 * target under its own id, so such an entry carries the target's name and provider. An entry that only shares an
 * alias's id is a model of its own and stays. `aliases` are the alias-to-target pairs of the loaded overlay and of the
 * draft.
 */
export function withoutAliasCopies<T extends { readonly id: string; readonly name: string; readonly provider?: string }>(
  models: readonly T[],
  aliases: ReadonlyArray<readonly [string, string]>,
): T[] {
  const byId = new Map<string, T>(models.map((model): [string, T] => [foldId(model.id), model]))
  const copies = new Set<T>()
  for (const [alias, target] of aliases) {
    const copy = byId.get(foldId(alias))
    const original = byId.get(foldId(target))
    if (copy && original && copy !== original && copy.name === original.name && copy.provider === original.provider) {
      copies.add(copy)
    }
  }
  return models.filter((model) => !copies.has(model))
}

export { ROUTER_POLICY_COMPLEXITIES, ROUTER_POLICY_MODES }
export type { RouterPolicyComplexity, RouterPolicyMode, RoutingPolicy }
