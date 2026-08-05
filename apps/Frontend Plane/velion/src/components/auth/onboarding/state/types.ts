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

export type OnboardingPlanId =
  | 'trial'
  | 'hobby'
  | 'standard'
  | 'pro'
  | 'enterprise'

export interface OrganizationPayload {
  /** Control-plane org id returned by org-core once the org is created. */
  id?: string
  name: string
  slug?: string
  /** Selected Verevon plan card from the paywall step. */
  plan?: OnboardingPlanId
  size?: 'solo' | 'small' | 'medium' | 'large' | 'enterprise'
  brregOrgNumber?: string
  /** Exact employee count from Brreg when available. */
  employeeCount?: number
}

/**
 * Brand signals extracted from the seed page by
 * `quarry_transform::branding_rendered::extract` and forwarded as
 * `branding_extracted` events by the runtime. Every field is best-
 * effort; downstream renderers must degrade gracefully when missing.
 */
export interface BrandingSignals {
  /** Resolved page URL the branding was extracted from. */
  url?: string
  /** Detected site name (og:site_name → application-name fallback). */
  siteName?: string
  /** Absolute favicon URL. Falls back to `/favicon.ico` on the host. */
  favicon?: string
  /** `<meta name="theme-color">` value if present. */
  themeColor?: string
  /** Absolute og:image URL. */
  ogImage?: string
  /** apple-touch-icon URL. */
  appleTouchIcon?: string
  /** Hex color codes detected in inline CSS (primary palette candidates). */
  palette?: string[]
  /** Detected primary font family. */
  fontFamily?: string
  /** Largest `<img>` URL by area — likely a logo/hero image. */
  logoCandidate?: string
  /** Body background color when detected from inline style. */
  bodyBackground?: string
}

export interface WebsitePayload {
  url: string
  agentBrief: string
  /** Set once the crawl preview returns its first batch of snippets. */
  crawlJobId?: string
  /**
   * Brand signals captured from the seed page. Populated by the live
   * crawl preview's `branding` SSE event so later steps can theme the
   * UI with the detected colors and surface the real logo.
   */
  branding?: BrandingSignals
}

export interface ConnectorPick {
  id: string
  label: string
  authedAt?: string
}

export interface PlanRecommendation {
  /** ID of one of the 5 cards (hobby | standard | pro | enterprise | trial). */
  planId: OnboardingPlanId
  /** Short human-readable reason; the LLM fills this in. */
  reason: string
  /** Quick sales-engineer summary rendered on the paywall. */
  summary?: string
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
export const STORAGE_KEY = 'verevon.onboarding.v1'

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
