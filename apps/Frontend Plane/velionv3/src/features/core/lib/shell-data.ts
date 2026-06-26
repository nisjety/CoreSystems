import { pickLocaleText, type Locale } from '@/shared/i18n/locales'

export type VelionRoute =
  | '/dashboard'
  | '/search'
  | '/chat'
  | '/studio'
  | '/studio/canvas'
  | '/studio/campaigns'
  | '/studio/templates'
  | '/inbox'
  | '/tickets'
  | '/social'
  | '/social/accounts'
  | '/social/calendar'
  | '/social/drafts'
  | '/social/approvals'
  | '/social/campaigns'
  | '/social/competitors'
  | '/social/trends'
  | '/social/evergreen'
  | '/insights'
  | '/insights/overview'
  | '/insights/social'
  | '/insights/inbox'
  | '/insights/agents'
  | '/insights/campaigns'
  | '/insights/experiments'
  | '/ingestions'
  | '/knowledge'
  | '/leads'
  | '/agents'
  | '/agents/runs'
  | '/agents/cost'
  | '/account'
  | '/settings'
  | '/settings/workspace'
  | '/settings/members'
  | '/settings/billing'
  | '/settings/sso'
  | '/settings/org-security'
  | '/settings/integrations'
  | '/settings/trust'
  | '/settings/router-policy'
  | '/settings/finetune'
  | '/settings/mcp'

export type WorkspaceIdentity = {
  accentColor?: string | null
  domain?: string | null
  initial: string
  logoUrl?: string | null
  name: string
  plan: string
  role?: string | null
  userEmail?: string | null
  userName?: string | null
  userAvatar?: string | null
}

export const fallbackWorkspaceIdentity: WorkspaceIdentity = {
  name: 'Workspace',
  plan: 'Free',
  initial: 'V',
}

export const demoWorkspaceIdentity: WorkspaceIdentity = {
  name: 'Velion',
  plan: 'Trial',
  initial: 'V',
  accentColor: '#111111',
  domain: 'velion.dev',
  userEmail: 'local@velion.dev',
  userName: 'Velion Local',
}

export function formatPlanLabel(plan?: string | null): string {
  const normalized = plan?.trim().toLowerCase()
  if (!normalized) return 'Free'

  const labels: Record<string, string> = {
    advanced: 'Advanced',
    custom: 'Custom',
    enterprise: 'Enterprise',
    essential: 'Essential',
    expert: 'Expert',
    free: 'Free',
    hobby: 'Hobby',
    // Velion brand name for the `pro` billing tier (billing-core: "Velion Expert").
    pro: 'Expert',
    standard: 'Standard',
    trial: 'Trial',
  }

  return labels[normalized] ?? normalized.replace(/(^|[-_\s])(\w)/g, (_match, prefix: string, char: string) => `${prefix === '_' ? ' ' : prefix}${char.toUpperCase()}`)
}

export function getNavbarLabels(activeRoute: VelionRoute, locale: Locale = 'no') {
  const labels = (moduleNo: string, moduleEn: string, tabNo: string, tabEn: string) => ({
    moduleLabel: pickLocaleText(locale, moduleNo, moduleEn),
    tabLabel: pickLocaleText(locale, tabNo, tabEn),
  })

  switch (activeRoute) {
    case '/chat':
      return labels('Chat', 'Chat', 'Oppgaver', 'Tasks')
    case '/studio':
    case '/studio/canvas':
      return { moduleLabel: 'Studio', tabLabel: 'Canvas' }
    case '/studio/campaigns':
      return labels('Studio', 'Studio', 'Kampanjer', 'Campaigns')
    case '/studio/templates':
      return labels('Studio', 'Studio', 'Maler', 'Templates')
    case '/inbox':
      return labels('Innboks', 'Inbox', 'Din innboks', 'Your inbox')
    case '/tickets':
      return labels('Saker', 'Ticketing', 'Mine saker', 'My tickets')
    case '/social':
    case '/social/calendar':
      return labels('Sosialt', 'Social', 'Kalender', 'Calendar')
    case '/social/accounts':
      return labels('Sosialt', 'Social', 'Kontoer', 'Accounts')
    case '/social/drafts':
      return labels('Sosialt', 'Social', 'Utkast', 'Drafts')
    case '/social/approvals':
      return labels('Sosialt', 'Social', 'Godkjenninger', 'Approvals')
    case '/social/campaigns':
      return labels('Sosialt', 'Social', 'Kampanjer', 'Campaigns')
    case '/social/competitors':
      return labels('Sosialt', 'Social', 'Konkurrenter', 'Competitors')
    case '/social/trends':
      return labels('Sosialt', 'Social', 'Trender', 'Trends')
    case '/social/evergreen':
      return labels('Sosialt', 'Social', 'Evergreen', 'Evergreen')
    case '/insights':
    case '/insights/overview':
      return labels('Innsikt', 'Insights', 'Oversikt', 'Overview')
    case '/insights/social':
      return labels('Innsikt', 'Insights', 'Sosialt', 'Social')
    case '/insights/inbox':
      return labels('Innsikt', 'Insights', 'Innboks', 'Inbox')
    case '/insights/agents':
      return labels('Innsikt', 'Insights', 'Agenter', 'Agents')
    case '/insights/campaigns':
      return labels('Innsikt', 'Insights', 'Kampanjer', 'Campaigns')
    case '/insights/experiments':
      return labels('Innsikt', 'Insights', 'Eksperimenter', 'Experiments')
    case '/ingestions':
      return labels('Innhenting', 'Ingestions', 'Arbeidsflate', 'Workspace')
    case '/agents':
      return labels('Agenter', 'Agents', 'Studio', 'Studio')
    case '/agents/runs':
      return labels('Agenter', 'Agents', 'Kjørekonsoll', 'Run Console')
    case '/knowledge':
      return labels('Kunnskap', 'Knowledge', 'Kilder', 'Sources')
    case '/account':
      return labels('Konto', 'Account', 'Profil', 'Profile')
    case '/settings':
    case '/settings/workspace':
    case '/settings/members':
    case '/settings/billing':
    case '/settings/sso':
    case '/settings/org-security':
    case '/settings/integrations':
    case '/settings/trust':
      return labels('Innstillinger', 'Settings', 'Arbeidsområde', 'Workspace')
    case '/dashboard':
    default:
      return labels('Oversikt', 'Overview', 'Hjem', 'Home')
  }
}

export function routeFromPath(pathname: string): VelionRoute {
  const path = pathname.replace(/\/+$/, '') || '/dashboard'
  if (path.startsWith('/chat')) return '/chat'
  if (path.startsWith('/studio/campaigns')) return '/studio/campaigns'
  if (path.startsWith('/studio/templates')) return '/studio/templates'
  if (path.startsWith('/studio')) return '/studio/canvas'
  if (path.startsWith('/inbox')) return '/inbox'
  if (path.startsWith('/tickets')) return '/tickets'
  if (path.startsWith('/social/accounts')) return '/social/accounts'
  if (path.startsWith('/social/drafts')) return '/social/drafts'
  if (path.startsWith('/social/approvals')) return '/social/approvals'
  if (path.startsWith('/social/campaigns')) return '/social/campaigns'
  if (path.startsWith('/social/competitors')) return '/social/competitors'
  if (path.startsWith('/social/trends')) return '/social/trends'
  if (path.startsWith('/social/evergreen')) return '/social/evergreen'
  if (path.startsWith('/social')) return '/social/calendar'
  if (path.startsWith('/insights/social')) return '/insights/social'
  if (path.startsWith('/insights/inbox')) return '/insights/inbox'
  if (path.startsWith('/insights/agents')) return '/insights/agents'
  if (path.startsWith('/insights/campaigns')) return '/insights/campaigns'
  if (path.startsWith('/insights/experiments')) return '/insights/experiments'
  if (path.startsWith('/insights')) return '/insights/overview'
  if (path.startsWith('/ingestions')) return '/ingestions'
  if (path.startsWith('/knowledge')) return '/knowledge'
  // The Run Console is a full-bleed surface — keep it out of the /agents config
  // sub-sidebar by routing it to its own value (matched before the /agents catch).
  if (path.startsWith('/agents/runs')) return '/agents/runs'
  if (path.startsWith('/agents')) return '/agents'
  if (path.startsWith('/account')) return '/account'
  if (path.startsWith('/settings/router-policy')) return '/settings/router-policy'
  if (path.startsWith('/settings/finetune')) return '/settings/finetune'
  if (path.startsWith('/settings/mcp')) return '/settings/mcp'
  if (path.startsWith('/settings/integrations')) return '/settings/integrations'
  if (path.startsWith('/settings/trust')) return '/settings/trust'
  if (path.startsWith('/settings/org-security')) return '/settings/org-security'
  if (path.startsWith('/settings/billing')) return '/settings/billing'
  if (path.startsWith('/settings/members')) return '/settings/members'
  if (path.startsWith('/settings/workspace')) return '/settings/workspace'
  if (path.startsWith('/settings/sso')) return '/settings/sso'
  if (path.startsWith('/settings')) return '/settings'
  return '/dashboard'
}
