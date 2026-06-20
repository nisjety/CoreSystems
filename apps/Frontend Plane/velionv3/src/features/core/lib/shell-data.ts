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
    pro: 'Pro',
    standard: 'Standard',
    trial: 'Trial',
  }

  return labels[normalized] ?? normalized.replace(/(^|[-_\s])(\w)/g, (_match, prefix: string, char: string) => `${prefix === '_' ? ' ' : prefix}${char.toUpperCase()}`)
}

export function getNavbarLabels(activeRoute: VelionRoute) {
  switch (activeRoute) {
    case '/chat':
      return { moduleLabel: 'Chat', tabLabel: 'Oppgaver' }
    case '/studio':
    case '/studio/canvas':
      return { moduleLabel: 'Studio', tabLabel: 'Canvas' }
    case '/studio/campaigns':
      return { moduleLabel: 'Studio', tabLabel: 'Campaigns' }
    case '/studio/templates':
      return { moduleLabel: 'Studio', tabLabel: 'Templates' }
    case '/inbox':
      return { moduleLabel: 'Inbox', tabLabel: 'Your inbox' }
    case '/tickets':
      return { moduleLabel: 'Ticketing', tabLabel: 'My tickets' }
    case '/social':
    case '/social/calendar':
      return { moduleLabel: 'Social', tabLabel: 'Calendar' }
    case '/social/accounts':
      return { moduleLabel: 'Social', tabLabel: 'Accounts' }
    case '/social/drafts':
      return { moduleLabel: 'Social', tabLabel: 'Drafts' }
    case '/social/approvals':
      return { moduleLabel: 'Social', tabLabel: 'Approvals' }
    case '/social/campaigns':
      return { moduleLabel: 'Social', tabLabel: 'Campaigns' }
    case '/social/competitors':
      return { moduleLabel: 'Social', tabLabel: 'Competitors' }
    case '/social/trends':
      return { moduleLabel: 'Social', tabLabel: 'Trends' }
    case '/social/evergreen':
      return { moduleLabel: 'Social', tabLabel: 'Evergreen' }
    case '/insights':
    case '/insights/overview':
      return { moduleLabel: 'Insights', tabLabel: 'Overview' }
    case '/insights/social':
      return { moduleLabel: 'Insights', tabLabel: 'Social' }
    case '/insights/inbox':
      return { moduleLabel: 'Insights', tabLabel: 'Inbox' }
    case '/insights/agents':
      return { moduleLabel: 'Insights', tabLabel: 'Agents' }
    case '/insights/campaigns':
      return { moduleLabel: 'Insights', tabLabel: 'Campaigns' }
    case '/insights/experiments':
      return { moduleLabel: 'Insights', tabLabel: 'Experiments' }
    case '/ingestions':
      return { moduleLabel: 'Ingestions', tabLabel: 'Workspace' }
    case '/agents':
      return { moduleLabel: 'Agenter', tabLabel: 'Studio' }
    case '/agents/runs':
      return { moduleLabel: 'Agenter', tabLabel: 'Run Console' }
    case '/knowledge':
      return { moduleLabel: 'Kunnskap', tabLabel: 'Kilder' }
    case '/account':
      return { moduleLabel: 'Account', tabLabel: 'Profile' }
    case '/settings':
    case '/settings/workspace':
    case '/settings/members':
    case '/settings/billing':
    case '/settings/sso':
    case '/settings/org-security':
    case '/settings/integrations':
    case '/settings/trust':
      return { moduleLabel: 'Innstillinger', tabLabel: 'Workspace' }
    case '/dashboard':
    default:
      return { moduleLabel: 'Oversikt', tabLabel: 'Hjem' }
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
