import {
  MonitorPlay,
  BarChart3,
  BookOpen,
  Bot,
  CalendarDays,
  CheckCheck,
  CircleUserRound,
  Coins,
  CreditCard,
  Fingerprint,
  Flame,
  Gauge,
  Home,
  Inbox,
  HatGlasses,
  Icon,
  MessageSquare,
  Megaphone,
  PanelsTopLeft,
  PenLine,
  Plug,
  Presentation,
  Radar,
  Repeat2,
  Route,
  Search,
  ServerCog,
  Settings,
  ShieldCheck,
  Telescope,
  TestTubeDiagonal,
  TicketCheck,
  TrendingUp,
  Users,
  LayoutTemplate,
  type IconNode,
  type LucideProps,
} from '@/shared/icons'
import { createComponent, type Component } from 'solid-js'
import type { VerevonRoute } from '@/features/core/lib/shell-data'

export type SidebarIcon = Component<LucideProps>
export type SidebarHref = VerevonRoute | `${VerevonRoute}?${string}`

const layoutFreeformIconNode: IconNode = [
  ['rect', { width: '7', height: '7', x: '3', y: '3', rx: '1' }],
  ['rect', { width: '7', height: '7', x: '14', y: '4', rx: '1' }],
  ['rect', { width: '7', height: '7', x: '4', y: '14', rx: '1' }],
]

const LayoutFreeform: SidebarIcon = (props) => createComponent(Icon, {
  ...props,
  iconNode: layoutFreeformIconNode,
  name: 'layout-freeform',
})

export type SidebarPanelTab = {
  id: string
  label: string
}

export type SidebarPanelItem = {
  id: string
  label: string
  href: SidebarHref
  icon: SidebarIcon
  description: string
  aliases?: string[]
  /** Do not mark this base route active when another Support view is selected. */
  exactPathMatch?: boolean
  tabId?: string
  subItems?: Array<{
    id: string
    label: string
    href: SidebarHref
  }>
}

export type SidebarPanelGroup = {
  id: string
  label: string
  showHeader?: boolean
  collapsible?: boolean
  defaultExpanded?: boolean
  alignBottom?: boolean
  items: SidebarPanelItem[]
}

export type SidebarSection = {
  id: string
  label: string
  href: SidebarHref
  icon: SidebarIcon
  description: string
  pinnedBottom?: boolean
  skipActiveMatch?: boolean
  panelTabs?: SidebarPanelTab[]
  panelGroups: SidebarPanelGroup[]
}

export const sidebarSections: SidebarSection[] = [
  {
    id: 'overview',
    label: 'Home',
    href: '/dashboard',
    icon: Home,
    description: 'Arbeidsflate, snarveier og operasjonell status.',
    panelTabs: [
      { id: 'my-account', label: 'Min konto' },
      { id: 'shared', label: 'Delt med meg' },
    ],
    panelGroups: [
      {
        id: 'overview-core',
        label: 'Oversikt',
        items: [
          { id: 'overview-home', label: 'Hjem', href: '/dashboard', icon: Home, description: 'Tilbake til Verevon Home.', tabId: 'my-account' },
          // Points at the /spaces resolver, not /spaces/:spaceId — the sidebar
          // has no Space ref to build a direct link from. `aliases` keeps the
          // item highlighted once the resolver has forwarded to a real room.
          { id: 'overview-spaces', label: 'Rom', href: '/spaces', icon: Users, description: 'Delt rom for samtaler, arbeid, kunnskap og medlemmer.', tabId: 'my-account', aliases: ['/spaces'] },
          { id: 'overview-studio', label: 'Studio', href: '/studio/canvas', icon: PanelsTopLeft, description: 'Bygg kampanjer, pakker og visuelle planer på canvas.', tabId: 'my-account' },
          { id: 'overview-inbox', label: 'Support', href: '/support', icon: Inbox, description: 'Samtaler, saker, AI-gjennomgang og SLA i én arbeidsflyt.', tabId: 'my-account' },
          { id: 'overview-social', label: 'Social', href: '/social/calendar', icon: CalendarDays, description: 'Planlegg postkalender, godkjenninger og kanaler.', tabId: 'shared' },
          { id: 'overview-knowledge', label: 'Kunnskap', href: '/knowledge', icon: BookOpen, description: 'Indekserte kilder, status og datakvalitet.', tabId: 'shared' },
          { id: 'overview-insights', label: 'Insights', href: '/insights/overview', icon: BarChart3, description: 'Mål sosialt, inbox, agenter, kampanjer og eksperimenter.', tabId: 'shared' },
          { id: 'overview-leads', label: 'Leads', href: '/leads', icon: Search, description: 'Finn norske bedrifter i Enhetsregisteret etter bransje, sted og størrelse.', tabId: 'shared' },
          { id: 'overview-agents', label: 'Agenter', href: '/agents/runs', icon: Bot, description: 'Start en kjøring, følg planen og godkjenn risikofylte steg.', tabId: 'shared' },
        ],
      },
    ],
  },
  {
    // Its own rail section, not only a shortcut in the Hjem panel. The Hjem
    // panel is a customizable shortcut list — a surface whose only entry point
    // is a shortcut someone can remove has no home of its own.
    id: 'spaces',
    label: 'Rom',
    href: '/spaces',
    icon: Users,
    description: 'Delt rom for samtaler, arbeid, kunnskap og medlemmer.',
    panelGroups: [],
  },
  {
    // The ONE sidebar destination for Chat. Five further `/chat` shortcuts used
    // to sit inside unrelated sections (Overview, Agents, Ingestions,
    // Knowledge, plus a redundant "Ny samtale" inside this very section),
    // turning Chat into the app's dumping ground and making the rail imply
    // five different chats. New chats start here or from a composer/search
    // launch; a surface that wants Verevon's help launches a thread of its own
    // (see `writePendingChatLaunch`) rather than linking sideways into Chat.
    id: 'messages',
    label: 'Chat',
    href: '/chat',
    icon: MessageSquare,
    description: 'Verevon AI-chat og oppgaver.',
    panelGroups: [
      {
        id: 'messages-core',
        label: 'Chat',
        items: [
          { id: 'messages-inbox', label: 'Support', href: '/support', icon: Inbox, description: 'Gå til kundesamtaler og oppfølging.' },
        ],
      },
    ],
  },
  {
    id: 'inbox',
    label: 'Support',
    href: '/support',
    icon: Inbox,
    description: 'Samtaler, saker, AI-gjennomgang og SLA i én arbeidsflyt.',
    panelGroups: [
      {
        id: 'inbox-core',
        label: 'Support',
        collapsible: true,
        defaultExpanded: true,
        items: [
          { id: 'inbox-home', label: 'Conversations', href: '/support', icon: Inbox, description: 'Samtaler som krever oppfølging.', exactPathMatch: true },
          { id: 'support-tickets', label: 'Tickets', href: '/support?surface=tickets', icon: TicketCheck, description: 'Saker, SLA og eierskap.' },
          { id: 'support-remote', label: 'Remote support', href: '/support?surface=remote', icon: MonitorPlay, description: 'Se og styre kundens skjerm etter samtykke.' },
        ],
      },
    ],
  },
  {
    id: 'marketing',
    label: 'Marketing',
    href: '/studio/campaigns',
    icon: Presentation,
    description: 'Kampanjer, innholdspakker og publiseringsklare arbeidsflyter.',
    panelGroups: [
      {
        id: 'marketing-core',
        label: 'Marketing',
        defaultExpanded: true,
        items: [
          { id: 'marketing-campaigns', label: 'Campaigns', href: '/studio/campaigns', icon: Presentation, description: 'Planlegg kampanjepakker og lanseringsløp.' },
          { id: 'marketing-social-campaigns', label: 'Social campaigns', href: '/social/campaigns', icon: Megaphone, description: 'Samle og følge opp sosiale kampanjer.' },
          { id: 'marketing-drafts', label: 'Drafts', href: '/social/drafts', icon: PenLine, description: 'Utkast, varianter og kanaltilpasning.' },
        ],
      },
    ],
  },
  {
    id: 'agents',
    label: 'Agents',
    href: '/agents',
    icon: HatGlasses,
    description: 'Agentroller og styring.',
    panelGroups: [
      {
        id: 'agents-core',
        label: 'Agents',
        items: [
          { id: 'agents-all', label: 'Alle agenter', href: '/agents', icon: HatGlasses, description: 'Se og konfigurer agentroller.' },
          { id: 'agents-cost', label: 'Kostnad', href: '/agents/cost', icon: Coins, description: 'Reell modellbruk og kostnad per kjøring fra hovedboken.' },
          { id: 'agents-quality', label: 'Kvalitet', href: '/agents/quality', icon: Gauge, description: 'Nøyaktighet, drift og kvalitetssignal fra kjøringshistorikken.' },
        ],
      },
    ],
  },
  {
    id: 'studio',
    label: 'Studio',
    href: '/studio',
    icon: LayoutFreeform,
    description: 'Canvas for kampanjer, kreativer, scripts og innholdspakker.',
    panelGroups: [
      {
        id: 'studio-create',
        label: 'Create',
        collapsible: true,
        defaultExpanded: true,
        items: [
          { id: 'studio-canvas', label: 'Canvas', href: '/studio/canvas', aliases: ['/studio'], icon: PanelsTopLeft, description: 'Visuell arbeidsflate for kampanjer og innhold.' },
          { id: 'studio-campaigns', label: 'Campaign planner', href: '/studio/campaigns', icon: Megaphone, description: 'Planlegg kampanjepakker og lanseringsløp.' },
          { id: 'studio-templates', label: 'Templates', href: '/studio/templates', icon: LayoutTemplate, description: 'Start fra godkjente maler og formatpakker.' },
        ],
      },
      {
        id: 'studio-linked',
        label: 'Linked systems',
        collapsible: true,
        defaultExpanded: true,
        items: [
          { id: 'studio-social-drafts', label: 'Social drafts', href: '/social/drafts', icon: PenLine, description: 'Send valgte blokker til sosial draft-kø.' },
          { id: 'studio-knowledge-assets', label: 'Knowledge assets', href: '/knowledge', icon: BookOpen, description: 'Bruk godkjente kilder og medier i canvas.' },
        ],
      },
    ],
  },
  {
    id: 'social',
    label: 'Calendar',
    href: '/social',
    icon: CalendarDays,
    description: 'Postkalender, kontoer og publiseringsflyt.',
    panelGroups: [
      {
        id: 'social-channels',
        label: 'Channels',
        defaultExpanded: true,
        items: [
          { id: 'social-accounts', label: 'Accounts', href: '/social/accounts', icon: Plug, description: 'Kontoer, kanaler og publiseringsklarhet.' },
        ],
      },
      {
        id: 'social-plan',
        label: 'Plan',
        defaultExpanded: true,
        items: [
          { id: 'social-calendar', label: 'Calendar', href: '/social/calendar', aliases: ['/social'], icon: CalendarDays, description: 'Vis, planlegg og godkjenn poster.' },
          { id: 'social-drafts', label: 'Drafts', href: '/social/drafts', icon: PenLine, description: 'Utkast, varianter og plattformtilpasning.' },
          { id: 'social-approvals', label: 'Approvals', href: '/social/approvals', icon: CheckCheck, description: 'Menneskelig review før publisering.' },
          { id: 'social-campaigns', label: 'Campaigns', href: '/social/campaigns', icon: Megaphone, description: 'Sosiale kampanjer og kanalpakker.' },
        ],
      },
      {
        id: 'social-intelligence',
        label: 'Intelligence',
        defaultExpanded: true,
        items: [
          { id: 'social-competitors', label: 'Competitor watch', href: '/social/competitors', icon: Telescope, description: 'Overvåk konkurrenter og breakout-innhold.' },
          { id: 'social-trends', label: 'Trends', href: '/social/trends', icon: TrendingUp, description: 'Trender, virale formater og lagrede hooks.' },
        ],
      },
      {
        id: 'social-reuse',
        label: 'Reuse',
        defaultExpanded: true,
        items: [
          { id: 'social-evergreen', label: 'Evergreen queue', href: '/social/evergreen', icon: Repeat2, description: 'Gjenbrukbart innhold og aktive workflow-lenker.' },
        ],
      },
      {
        id: 'social-commerce',
        label: 'Commerce',
        defaultExpanded: true,
        items: [
          { id: 'social-commerce-metrics', label: 'Ad metrics & catalogs', href: '/social/commerce', icon: BarChart3, description: 'Annonsemålinger per kanal og Meta Commerce-kataloger.' },
        ],
      },
    ],
  },
  {
    id: 'ingestions',
    label: 'Ingestions',
    href: '/ingestions',
    icon: Radar,
    description: 'Crawler, ekstraksjon, evidens og tidsplaner.',
    panelGroups: [
      {
        id: 'ingestions-core',
        label: 'Ingestions',
        items: [
          { id: 'ingestions-home', label: 'Workspace', href: '/ingestions', icon: Radar, description: 'Operasjonell arbeidsflate for ingest og bevis.' },
          { id: 'ingestions-knowledge', label: 'Knowledge', href: '/knowledge', icon: BookOpen, description: 'Se kuraterte kilder og chunk-inspeksjon.' },
        ],
      },
    ],
  },
  {
    id: 'knowledge',
    label: 'Kunnskap',
    href: '/knowledge',
    icon: BookOpen,
    description: 'Kilder, indeksering og kunnskapsstatus.',
    panelGroups: [
      {
        id: 'knowledge-core',
        label: 'Kunnskap',
        items: [
          { id: 'knowledge-overview', label: 'Datakilder', href: '/knowledge', icon: BookOpen, description: 'Koble, vurder og overvåk kilder.' },
        ],
      },
    ],
  },
  {
    id: 'insights',
    label: 'Insights',
    href: '/insights',
    icon: BarChart3,
    description: 'Måling, eksperimenter og ytelse på tvers av Verevon.',
    panelGroups: [
      {
        id: 'insights-core',
        label: 'Measure',
        defaultExpanded: true,
        items: [
          { id: 'insights-overview', label: 'Overview', href: '/insights/overview', aliases: ['/insights'], icon: BarChart3, description: 'Samlet operasjonell og kommersiell måling.' },
          { id: 'insights-social', label: 'Social', href: '/insights/social', icon: CalendarDays, description: 'Sosial rekkevidde, publisering og kanalhelse.' },
          { id: 'insights-inbox', label: 'Inbox', href: '/insights/inbox', icon: Inbox, description: 'Svar, SLA, routing og konvertering fra samtaler.' },
          { id: 'insights-agents', label: 'Agents', href: '/insights/agents', icon: Bot, description: 'Agenthandlinger, kost, kvalitet og godkjenninger.' },
          { id: 'insights-campaigns', label: 'Campaigns', href: '/insights/campaigns', icon: Megaphone, description: 'Kampanjeytelse på tvers av kanaler.' },
          { id: 'insights-experiments', label: 'Experiments', href: '/insights/experiments', icon: Flame, description: 'Eksperimenter, vinnere og læring.' },
        ],
      },
    ],
  },
  {
    id: 'settings',
    label: 'Innstillinger',
    href: '/settings',
    icon: Settings,
    pinnedBottom: true,
    description: 'Arbeidsflate, medlemmer og kontroll.',
    panelGroups: [
      {
        id: 'settings-core',
        label: 'Innstillinger',
        items: [
          { id: 'settings-workspace', label: 'Workspace', href: '/settings/workspace', icon: Settings, description: 'Identitet, tilgang og integrasjoner.' },
          { id: 'settings-members', label: 'Members', href: '/settings/members', icon: CircleUserRound, description: 'Roller, seter og invitasjoner.' },
          { id: 'settings-billing', label: 'Billing', href: '/settings/billing', icon: CreditCard, description: 'Plan, bruk og fakturaer.' },
          { id: 'settings-security', label: 'Security', href: '/settings/org-security', icon: ShieldCheck, description: 'SSO, domener og organisasjonssikkerhet.' },
          { id: 'settings-trust', label: 'Trust Center', href: '/settings/trust', icon: Fingerprint, description: 'App-tilganger, AI-datatilgang og oppbevaring.' },
          { id: 'settings-router-policy', label: 'Router policy', href: '/settings/router-policy', icon: Route, description: 'Intent-laget: kompleksitet, budsjett og modellrute.' },
          { id: 'settings-finetune', label: 'Fine-tune jobs', href: '/settings/finetune', icon: TestTubeDiagonal, description: 'Azure-finjustering: opplasting, jobber og status.' },
          { id: 'settings-mcp', label: 'MCP-servere', href: '/settings/mcp', icon: ServerCog, description: 'Eksterne MCP-tjenere og tillatte verktøy for agenten.' },
        ],
      },
    ],
  },
]

export const sidebarSearchAction = {
  id: 'search',
  label: 'Søk',
  icon: Search,
  pinnedBottom: true,
} as const

// Keep the navigation gate available for future incomplete surfaces, while
// making Agents discoverable again in the primary shell.
export const DEMO_MODE_HIDE_UNFINISHED_NAV = true

const DEMO_MODE_HIDDEN_SECTION_IDS = new Set<string>()

const DEMO_MODE_HIDDEN_ITEM_IDS = new Set<string>([
  'insights-social',
  'insights-inbox',
  'insights-agents',
  'insights-campaigns',
  'insights-experiments',
])

export function applyDemoModeNavGate(sections: SidebarSection[]): SidebarSection[] {
  if (!DEMO_MODE_HIDE_UNFINISHED_NAV) return sections

  return sections
    .filter((section) => !DEMO_MODE_HIDDEN_SECTION_IDS.has(section.id))
    .map((section) => ({
      ...section,
      panelGroups: section.panelGroups
        .map((group) => ({
          ...group,
          items: group.items.filter((item) => !DEMO_MODE_HIDDEN_ITEM_IDS.has(item.id)),
        }))
        .filter((group) => group.items.length > 0),
    }))
}

export function isSidebarPathActive(pathname: string, href: SidebarHref, aliases: string[] = [], exactPathMatch = false) {
  const normalizedPathname = normalizePath(pathname)
  const paths = [href, ...aliases].map(normalizePath)

  return paths.some((path) => {
    if (path.includes('?')) return normalizedPathname === path
    if (exactPathMatch && normalizedPathname.includes('?')) return false
    const pathnameWithoutSearch = normalizedPathname.split('?')[0]!
    return pathnameWithoutSearch === path || pathnameWithoutSearch.startsWith(`${path}/`)
  })
}

export function getSidebarSectionForPath(
  pathname: string,
  fallbackRoute: VerevonRoute,
  sections: SidebarSection[] = sidebarSections,
): SidebarSection {
  const normalizedPathname = normalizePath(pathname)

  return (
    sections.find((section) => !section.skipActiveMatch && isSidebarPathActive(normalizedPathname, section.href)) ??
    sections.find((section) => section.href === fallbackRoute) ??
    sections[0]!
  )
}

function normalizePath(path: string) {
  const [pathname = '/', search = ''] = path.split('?')
  const trimmed = pathname.replace(/\/+$/, '') || '/'
  return search ? `${trimmed}?${search}` : trimmed
}
