import type {
  BrandingSignals,
  CrawlSnippet,
  PlanRecommendation,
  PreviewResponse,
  ThemeMode,
} from '@/features/onboarding/lib/api'

export type Step =
  | 'post-signin'
  | 'website'
  | 'organization'
  | 'connect'
  | 'social-proof'
  | 'paywall'
  | 'assembly'

export type StepTransitionPhase = 'idle' | 'leaving' | 'entering'
export type OrgSize = 'solo' | 'small' | 'medium' | 'large' | 'enterprise'
export type PlanId = 'trial' | 'hobby' | 'standard' | 'pro' | 'enterprise'

export type ConnectorOption = {
  id: string
  label: string
  hint: string
  provider: string
  sources: string[]
  category: ConnectorCategory
}

export type ConnectorCategory = 'work' | 'social' | 'other'

type ConnectedSource = {
  id: string
  label: string
  status: 'pending' | 'connected' | 'partial'
  connectUrl?: string
  sources?: string[]
  sourceCount?: number
}

export type OnboardingState = {
  step: Step
  introPlayed: boolean
  websiteSkipped: boolean
  website: {
    url: string
    brief: string
    crawlJobId?: string
    branding?: BrandingSignals
    snippets: CrawlSnippet[]
    pages: number
    elements: number
    status: 'idle' | 'starting' | 'running' | 'completed' | 'failed' | 'cancelled'
    warning?: string
  }
  organization: {
    id?: string
    name: string
    orgNumber?: string
    employeeCount?: number
    size?: OrgSize
    /** Brreg industry (naeringskode1.beskrivelse) — a strong personalization
     * signal for the AI plan recommendation. */
    industry?: string
    /** Brreg organisasjonsform.beskrivelse (e.g. "Aksjeselskap"). */
    orgForm?: string
    /** Zero Data Retention posture for interactive AI content. It is OFF by
     * default; organizations may opt in when they need conversation content to
     * remain transient rather than powering history or memory. */
    zeroDataRetention: boolean
  }
  connectors: ConnectedSource[]
  recommendation?: PlanRecommendation
  plan?: PlanId
  themeMode: ThemeMode
}

export type GraphDisplayNode = PreviewResponse['nodes'][number] & {
  position: { left: string; top: string }
}

export const onboardingSteps: readonly Step[] = [
  'post-signin',
  'website',
  'organization',
  'connect',
  'social-proof',
  'paywall',
  'assembly',
]

export const onboardingConnectorOptions: readonly ConnectorOption[] = [
  {
    id: 'slack',
    label: 'Slack',
    hint: 'Kanaler + tråder',
    provider: 'slack',
    sources: ['messages'],
    category: 'work',
  },
  {
    id: 'microsoft365',
    label: 'Microsoft 365',
    hint: 'Teams, Outlook, SharePoint, OneDrive',
    provider: 'microsoft',
    sources: ['teams', 'outlook', 'sharepoint', 'onedrive'],
    category: 'work',
  },
  {
    id: 'notion',
    label: 'Notion',
    hint: 'Sider + databaser',
    provider: 'notion',
    sources: ['pages', 'databases'],
    category: 'work',
  },
  {
    id: 'gdrive',
    label: 'Google Workspace',
    hint: 'Drive, dokumenter, Gmail og kalender',
    provider: 'google',
    sources: ['google_drive', 'documents', 'gmail', 'calendar'],
    category: 'work',
  },
  {
    id: 'github',
    label: 'GitHub',
    hint: 'README + issues',
    provider: 'github',
    sources: ['issues'],
    category: 'work',
  },
  {
    id: 'discord',
    label: 'Discord',
    hint: 'Server-identitet og medlemskap',
    provider: 'discord',
    sources: ['guilds'],
    category: 'work',
  },
  {
    id: 'linkedin',
    label: 'LinkedIn',
    hint: 'Sider, innlegg og kampanjesignaler',
    provider: 'linkedin',
    sources: ['company_pages', 'posts'],
    category: 'social',
  },
  {
    // Unified Meta connection: Facebook Pages + Instagram + WhatsApp Business
    // + Meta Ads authorize through ONE dialog (supersedes the separate
    // facebook/instagram/whatsapp/meta-ads providers).
    id: 'meta',
    label: 'Meta',
    hint: 'Facebook Pages, Instagram, WhatsApp og Meta Ads',
    provider: 'meta',
    sources: ['pages', 'instagram_business', 'whatsapp', 'ads'],
    category: 'social',
  },
  {
    id: 'tiktok',
    label: 'TikTok',
    hint: 'Creator-profil, videoer og publisering',
    provider: 'tiktok',
    sources: ['creator_profile', 'videos'],
    category: 'social',
  },
  {
    id: 'x',
    label: 'X',
    hint: 'Innlegg, omtaler og DM-signaler',
    provider: 'x',
    sources: ['posts', 'mentions', 'direct_messages'],
    category: 'social',
  },
  {
    id: 'snapchat',
    label: 'Snapchat',
    hint: 'Ads-kontoer, kampanjer og analyser',
    provider: 'snapchat',
    sources: ['ad_accounts', 'campaigns', 'analytics'],
    category: 'social',
  },
  {
    id: 'shopify',
    label: 'Shopify',
    hint: 'Produkter, ordre og kundeprofiler',
    provider: 'shopify',
    sources: ['products', 'orders', 'customers'],
    category: 'other',
  },
  {
    id: 'stripe',
    label: 'Stripe',
    hint: 'Kunder, abonnement og fakturaer',
    provider: 'stripe',
    sources: ['customers', 'subscriptions', 'invoices'],
    category: 'other',
  },
  {
    // Verevon's own freight aggregator (shipping-core, Ingestion Plane) — one
    // integration covers hele transportørflåten. No per-user OAuth: connecting
    // verifies the aggregator and lists the carriers it can compare.
    id: 'shipping',
    label: 'Frakt & sporing',
    hint: 'Bring, PostNord, DHL, Helthjem, Porterbuddy m.fl. — priser og sporing',
    provider: 'shipping',
    sources: ['quotes', 'carriers', 'tracking'],
    category: 'other',
  },
]

export const onboardingPlanCards: Array<{
  id: PlanId
  name: string
  price: string
  description: string
  features: string[]
  /** Mirrors billingPlans' checkoutEnabled (see @/features/billing/lib/plans):
   * false blocks the commit path from starting a real checkout session — the
   * enterprise/"Custom" tier's real price is never shown in this UI, so
   * selecting it must route to sales instead of a Nexi/Hyperswitch session. */
  checkoutEnabled: boolean
}> = [
  {
    id: 'trial',
    name: 'Gratis',
    price: '0',
    description: 'Prøv Verevon og agenten i 14 dager før du velger betalt plan.',
    features: ['Ingen kort kreves', '14 dagers prøveperiode', 'Oppgrader når du er klar'],
    checkoutEnabled: true,
  },
  {
    id: 'hobby',
    name: 'Essential',
    price: '299',
    description: 'For små team som vil validere en enkel chatbot.',
    features: ['Bruksbasert prising for AI-løste henvendelser', 'Chatbot + delt innboks', 'Nettside og kunnskapskilder'],
    checkoutEnabled: true,
  },
  {
    id: 'standard',
    name: 'Advanced',
    price: '999',
    description: 'For team som trenger automasjon, ruting og flere kilder.',
    features: ['Bruksbasert prising for AI-løste henvendelser', 'Automasjon og ruting', 'Flere team-innbokser', '20 Lite-seter inkludert'],
    checkoutEnabled: true,
  },
  {
    id: 'pro',
    name: 'Expert',
    price: '1499',
    description: 'For større supportteam med rapportering og styring.',
    features: ['Bruksbasert prising for AI-løste henvendelser', 'SSO og identitetsstyring', 'SLA, rapportering og multibrand', '50 Lite-seter inkludert'],
    checkoutEnabled: true,
  },
  {
    id: 'enterprise',
    name: 'Custom',
    price: 'Custom',
    description: 'Kontakt salg for volum, onboarding og governance.',
    features: ['Volumpris per AI-svar', 'Tilpassede vilkår', 'Utvidet onboarding', 'Dedikert success-team'],
    checkoutEnabled: false,
  },
]

export const onboardingFooterLinks = ['Om oss', 'Personvern', 'Opphavsrett', 'Cookie-innstillinger'] as const

export const onboardingCrawlPhases = [
  'Oppdager sider',
  'Kartlegger struktur',
  'Trekker ut innhold',
  'Bygger kunnskapsbase',
  'Ferdig!',
] as const

export const onboardingSizeOptions = ['solo', 'small', 'medium', 'large', 'enterprise'] as const
