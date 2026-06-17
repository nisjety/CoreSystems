import {
  ROUTER_POLICY_COMPLEXITIES,
  ROUTER_POLICY_MODES,
  type RouterPolicyComplexity,
  type RouterPolicyComplexityConfig,
  type RouterPolicyMode,
  type RoutingPolicy,
} from '@/shared/api/router-policy-client'
import { VELION_MODES, type VelionMode } from '@/shared/api/chat-client'

/** Map a routing-table mode key to its pinned Velion mode metadata (label/order). */
const MODE_TO_VELION_ID: Record<RouterPolicyMode, string> = {
  budget: 'velion-budget',
  balance: 'velion-balance',
  genius: 'velion-genius',
}

export interface RouterPolicyModeRow {
  mode: RouterPolicyMode
  label: string
  description: string
}

/** Mode rows in display order, labelled from the authoritative VELION_MODES list. */
export function routerPolicyModeRows(): RouterPolicyModeRow[] {
  return ROUTER_POLICY_MODES.map((mode) => {
    const velion: VelionMode | undefined = VELION_MODES.find((m) => m.id === MODE_TO_VELION_ID[mode])
    return {
      mode,
      label: velion?.label ?? mode,
      description: velion?.description ?? '',
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

export { ROUTER_POLICY_COMPLEXITIES, ROUTER_POLICY_MODES }
export type { RouterPolicyComplexity, RouterPolicyMode, RoutingPolicy }
