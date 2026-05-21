/**
 * Phase 1 onboarding · state shapes.
 *
 * The wizard runs as a state machine. The current step + every
 * collected field is persisted to `localStorage` so a refresh / tab
 * close / accidental URL change resumes at the same place. Server-side
 * persistence (the user-core onboarding-state endpoint that the old
 * `/onboarding/*` pages already use) lands in the next iteration —
 * localStorage is the bridge until then.
 */

export const ONBOARDING_STEPS = [
  'post-signin',
  'organization',
  'website',
  'connect',
  'social-proof',
  'paywall',
  'assembly',
] as const

export type OnboardingStep = (typeof ONBOARDING_STEPS)[number]

export interface OrganizationPayload {
  name: string
  size?: 'solo' | 'small' | 'medium' | 'large' | 'enterprise'
  brregOrgNumber?: string
}

export interface WebsitePayload {
  url: string
  agentBrief: string
  /** Set once the crawl preview returns its first batch of snippets. */
  crawlJobId?: string
}

export interface ConnectorPick {
  id: string
  label: string
  authedAt?: string
}

export interface PlanRecommendation {
  /** ID of one of the 5 cards (hobby | standard | pro | enterprise | trial). */
  planId: 'hobby' | 'standard' | 'pro' | 'enterprise' | 'trial'
  /** Short human-readable reason; the LLM fills this in. */
  reason: string
  /** When the recommendation was last computed (ISO). */
  generatedAt: string
}

export interface OnboardingState {
  step: OnboardingStep
  organization?: OrganizationPayload
  website?: WebsitePayload
  connectors: ConnectorPick[]
  recommendation?: PlanRecommendation
  /** Set true once `post-signin` slot 1 video has played through once. */
  introPlayed: boolean
  /** Started timestamp (ms since epoch). Used for analytics. */
  startedAt: number
}

export const INITIAL_STATE: OnboardingState = {
  step: 'post-signin',
  connectors: [],
  introPlayed: false,
  startedAt: Date.now(),
}

/**
 * localStorage key. Bumped when the wire shape changes so stale state
 * from older clients gets thrown away cleanly.
 */
export const STORAGE_KEY = 'velion.onboarding.v1'

/**
 * Map legacy `/onboarding/<slug>/page.tsx` slugs to the new step ids.
 * Used by the legacy-redirect bridge so users mid-flow on the old
 * pages re-enter the wizard at the same logical step.
 */
export const LEGACY_SLUG_TO_STEP: Record<string, OnboardingStep> = {
  organization: 'organization',
  profile: 'organization',
  website: 'website',
  team: 'connect',
  connect: 'connect',
  complete: 'assembly',
}
