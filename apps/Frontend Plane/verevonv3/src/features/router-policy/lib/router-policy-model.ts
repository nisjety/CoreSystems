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
 * back every field it was given. The ladder, the role table and the complexity flag are not shown on the page, and a
 * field left out of a save is reset on the server.
 */
export type RouterPolicyDocument = RoutingPolicy & {
  tool_fallback_ladder?: string[]
  roles?: RouterPolicyRoleTable
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

/** One refusal of a role table, as inference-core names it under `details` of an `invalid_role_table` error. */
export interface RouterPolicyRoleIssue {
  purpose: RouterPolicyRolePurpose
  reason: string
  model?: string
  effort?: string
}

/** The refusals in an `invalid_role_table` body that name a purpose the page shows. Anything else is dropped. */
export function roleIssuesFrom(details: unknown): RouterPolicyRoleIssue[] {
  if (!Array.isArray(details)) return []
  const issues: RouterPolicyRoleIssue[] = []
  for (const item of details as unknown[]) {
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

export { ROUTER_POLICY_COMPLEXITIES, ROUTER_POLICY_MODES }
export type { RouterPolicyComplexity, RouterPolicyMode, RoutingPolicy }
