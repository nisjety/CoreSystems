import { requestJson } from '@/shared/api/http'

// The router policy is the runtime tuning surface for the Velion intent layer
// (complexity scoring → mode/complexity → concrete model). The wire shape is
// snake_case and 1:1 with the backend RoutingPolicy contract, so we keep
// snake_case here rather than mapping to camelCase — a lossless representation
// makes the full-document PUT trivially correct.

/** Velion intent modes — the three rows of the routing table. */
export type RouterPolicyMode = 'budget' | 'balance' | 'genius'

/** Complexity buckets — the three columns of the routing table. */
export type RouterPolicyComplexity = 'simple' | 'moderate' | 'complex'

/** A single mode row: one model id per complexity bucket. */
export interface RouterPolicyModeTable {
  simple: string
  moderate: string
  complex: string
}

/** The 3×3 routing table keyed by mode → complexity → model id. */
export interface RouterPolicyTable {
  budget: RouterPolicyModeTable
  balance: RouterPolicyModeTable
  genius: RouterPolicyModeTable
}

/**
 * Complexity-scoring knobs that drive which bucket a request lands in. Field
 * names are 1:1 with inference-core's `ComplexityWeights` serde output
 * (`apps/Model Plane/rust/services/inference-core/src/provider/routing_policy.rs`),
 * so the full-document PUT round-trips losslessly.
 */
export interface RouterPolicyComplexityConfig {
  large_total_chars: number
  large_total_chars_score: number
  medium_total_chars: number
  medium_total_chars_score: number
  long_user_turn_chars: number
  long_user_turn_score: number
  code_fence_score: number
  keyword_score: number
  tool_use_score: number
  deep_conversation_turns: number
  deep_conversation_score: number
  moderate_threshold: number
  complex_threshold: number
  keywords: string[]
}

/** The full routing policy document. */
export interface RoutingPolicy {
  enabled: boolean
  budget_cap_usd: number
  constrained_fraction: number
  cheap_fallback: string
  complexity: RouterPolicyComplexityConfig
  table: RouterPolicyTable
  version?: number
  updated_by?: string
  updated_at?: string
}

/** Ordered mode keys, matching the routing-table rows. */
export const ROUTER_POLICY_MODES: readonly RouterPolicyMode[] = ['budget', 'balance', 'genius']

/** Ordered complexity keys, matching the routing-table columns. */
export const ROUTER_POLICY_COMPLEXITIES: readonly RouterPolicyComplexity[] = [
  'simple',
  'moderate',
  'complex',
]

function orgHeaders(orgId: string): HeadersInit {
  return { 'x-velion-org-id': orgId }
}

/** Fetch the active router policy for the org. */
export function getRouterPolicy(orgId: string, signal?: AbortSignal): Promise<RoutingPolicy> {
  return requestJson<RoutingPolicy>('/api/v1/router-policy', {
    headers: orgHeaders(orgId),
    signal,
  })
}

/**
 * Persist a router policy. The full document is sent; `version`/`updated_*` are
 * authored server-side, so we strip them from the body (immutably — the caller's
 * object is never mutated). The server returns the stored policy with fresh
 * version + audit metadata.
 */
export function updateRouterPolicy(
  orgId: string,
  policy: RoutingPolicy,
  signal?: AbortSignal,
): Promise<RoutingPolicy> {
  const { version: _version, updated_by: _updatedBy, updated_at: _updatedAt, ...rest } = policy
  void _version
  void _updatedBy
  void _updatedAt
  const body: RoutingPolicy = { ...rest, version: 0, updated_by: '', updated_at: '' }
  return requestJson<RoutingPolicy>('/api/v1/router-policy', {
    method: 'PUT',
    body: JSON.stringify(body),
    headers: orgHeaders(orgId),
    signal,
  })
}
