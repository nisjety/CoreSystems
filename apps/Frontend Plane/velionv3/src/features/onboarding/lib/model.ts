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
    id: 'linkedin',
    label: 'LinkedIn',
    hint: 'Sider, innlegg og kampanjesignaler',
    provider: 'linkedin',
    sources: ['company_pages', 'posts'],
    category: 'social',
  },
  {
    id: 'instagram',
    label: 'Instagram',
    hint: 'Business-profiler, medier og kommentarer',
    provider: 'instagram',
    sources: ['business_profile', 'media', 'comments'],
    category: 'social',
  },
  {
    id: 'facebook',
    label: 'Facebook',
    hint: 'Pages, innlegg, meldinger og kommentarer',
    provider: 'facebook',
    sources: ['pages', 'posts', 'comments', 'messages'],
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
]

export const onboardingPlanCards: Array<{
  id: PlanId
  name: string
  price: string
  description: string
  features: string[]
}> = [
  {
    id: 'trial',
    name: 'Gratis',
    price: '0',
    description: 'Prøv Velion og agenten i 14 dager før du velger betalt plan.',
    features: ['Ingen kort kreves', '14 dagers prøveperiode', 'Oppgrader når du er klar'],
  },
  {
    id: 'hobby',
    name: 'Essential',
    price: '299',
    description: 'For små team som vil validere en enkel chatbot.',
    features: ['4 kr per henvendelse løst av AI', 'Chatbot + delt innboks', 'Nettside og kunnskapskilder'],
  },
  {
    id: 'standard',
    name: 'Advanced',
    price: '999',
    description: 'For team som trenger automasjon, ruting og flere kilder.',
    features: ['3,50 kr per henvendelse løst av AI', 'Automasjon og ruting', 'Flere team-innbokser', '20 Lite-seter inkludert'],
  },
  {
    id: 'pro',
    name: 'Expert',
    price: '1499',
    description: 'For større supportteam med rapportering og styring.',
    features: ['2,90 kr per henvendelse løst av AI', 'SSO og identitetsstyring', 'SLA, rapportering og multibrand', '50 Lite-seter inkludert'],
  },
  {
    id: 'enterprise',
    name: 'Custom',
    price: 'Custom',
    description: 'Kontakt salg for volum, onboarding og governance.',
    features: ['Volumpris per AI-svar', 'Tilpassede vilkår', 'Utvidet onboarding', 'Dedikert success-team'],
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
