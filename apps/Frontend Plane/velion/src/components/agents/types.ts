export type AgentUseCase =
  | 'customer_support'
  | 'sales'
  | 'marketing'
  | 'hr'
  | 'faq'
  | 'onboarding'
  | 'other'

export type AgentStatus = 'active' | 'inactive' | 'draft'

/**
 * Harness profile (docs/HARNESS_PHASE1.md §1).
 *   - `chat`           — clean ChatGPT/Claude/Manus-style surface; the harness
 *                        stays invisible (no operator/task-graph UI).
 *   - `deployed_agent` — Intercom/Chatbase/Zendesk-style; the operator gets the
 *                        run-event feed, HITL handoff, and inbox surfaces.
 */
export type AgentProfile = 'chat' | 'deployed_agent'

// U2-2 / U3-2 (ui-ux-velion-gap.md §10): aligned with the real Azure /
// Anthropic deployments and capability-core's seeded `models` registry.
// The previous values (`gpt-5.4-mini`, `gpt-5.4`, `claude-sonnet-4-6`,
// `claude-opus-4-6`) were fictional names that Azure returned 404 for —
// agents could be *created* with them in Convex but never actually run.
// `app/api/chat/_lib/models.ts::normalizeLegacyAlias` still remaps the
// old names so existing Convex rows keep working until they are migrated.
//
// The runtime catalog is now fetched from `/api/models` (proxy to
// capability-core `/api/v1/capabilities?kind=model`); this static union
// is only the type-level fallback for components that haven't migrated.
export type SupportedModel =
  | 'claude-sonnet-4-5'
  | 'claude-opus-4-1'
  | 'gpt-4o-mini'
  | 'gpt-5-mini'

export const DEFAULT_MODEL: SupportedModel = 'gpt-4o-mini'

export interface KnowledgeSource {
  type: string
  name: string
  id?: string
}

/** Persisted agent configuration stored in Convex. */
export interface PersistedAgent {
  id: string
  orgId: string
  name: string
  description?: string
  useCase: AgentUseCase
  status: AgentStatus
  model: string
  temperature?: number
  systemPrompt?: string
  tone?: string
  greeting?: string
  tools?: string[]
  knowledgeSources?: KnowledgeSource[]
  /** Harness profile. Absent on legacy rows — resolve via {@link resolveAgentProfile}. */
  profile?: AgentProfile
  // Wave 9 (ui-ux-velion-gap.md §19): public embed widget config.
  publicEnabled?: boolean
  publicSecret?: string
  embedTheme?: {
    accentColor?: string
    buttonLabel?: string
    welcomeMessage?: string
  }
  createdBy?: string
  createdAt: number
  updatedAt: number
}

export type AgentCreateInput = Omit<PersistedAgent, 'id' | 'createdAt' | 'updatedAt'>

export type AgentUpdateInput = Partial<
  Omit<PersistedAgent, 'id' | 'orgId' | 'createdAt' | 'updatedAt'>
>

/** @deprecated Use PersistedAgent instead */
export interface Agent {
  id: string
  name: string
  description?: string
  useCase: AgentUseCase
  status: AgentStatus
  model: string
  createdAt: string
  tools: string[]
}

export const USE_CASE_LABELS: Record<AgentUseCase, string> = {
  customer_support: 'Customer Support',
  sales: 'Sales',
  marketing: 'Marketing',
  hr: 'HR & Recruiting',
  faq: 'FAQ',
  onboarding: 'Onboarding',
  other: 'Other',
}

export const MODEL_LABELS: Record<SupportedModel, string> = {
  'claude-sonnet-4-5': 'Claude Sonnet 4.5',
  'claude-opus-4-1': 'Claude Opus 4.1',
  'gpt-4o-mini': 'GPT-4o Mini',
  'gpt-5-mini': 'GPT-5 Mini (reasoning)',
}

export const MODEL_TIERS: Record<SupportedModel, 'low' | 'high'> = {
  'gpt-4o-mini': 'low',
  'gpt-5-mini': 'high',
  'claude-sonnet-4-5': 'high',
  'claude-opus-4-1': 'high',
}

/**
 * Resolve an agent's harness profile, applying the migration default for rows
 * created before the field existed: a publicly-embedded agent is a
 * `deployed_agent` (operator surfaces), otherwise `chat` (clean surface).
 */
export function resolveAgentProfile(
  agent: Pick<PersistedAgent, 'profile' | 'publicEnabled'> | null | undefined,
): AgentProfile {
  if (!agent) return 'chat'
  if (agent.profile) return agent.profile
  return agent.publicEnabled ? 'deployed_agent' : 'chat'
}

export const AGENT_PROFILE_LABELS: Record<AgentProfile, string> = {
  chat: 'Chat',
  deployed_agent: 'Deployed agent',
}
