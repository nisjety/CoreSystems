import {
  BarChart3,
  BookOpen,
  Bot,
  CalendarDays,
  CheckCheck,
  CircleUserRound,
  CreditCard,
  Fingerprint,
  Flame,
  Home,
  Inbox,
  MessageSquare,
  Megaphone,
  PanelsTopLeft,
  PenLine,
  Plug,
  Radar,
  Repeat2,
  Route,
  Search,
  Settings,
  ShieldCheck,
  Sparkles,
  Telescope,
  TestTubeDiagonal,
  TicketCheck,
  TrendingUp,
  UserRound,
  LayoutTemplate,
  type LucideProps,
} from 'lucide-solid'
import type { Component } from 'solid-js'
import type { VelionRoute } from '@/features/core/lib/shell-data'

export type SidebarIcon = Component<LucideProps>

export type SidebarPanelTab = {
  id: string
  label: string
}

export type SidebarPanelItem = {
  id: string
  label: string
  href: VelionRoute
  icon: SidebarIcon
  description: string
  aliases?: string[]
  tabId?: string
  subItems?: Array<{
    id: string
    label: string
    href: VelionRoute
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
  href: VelionRoute
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
    label: 'Oversikt',
    href: '/dashboard',
    icon: BarChart3,
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
          { id: 'overview-home', label: 'Hjem', href: '/dashboard', icon: Home, description: 'Tilbake til Velion Home.', tabId: 'my-account' },
          { id: 'overview-chat', label: 'Velion Chat', href: '/chat', icon: MessageSquare, description: 'Start eller fortsett arbeid med AI.', tabId: 'my-account' },
          { id: 'overview-studio', label: 'Studio', href: '/studio/canvas', icon: PanelsTopLeft, description: 'Bygg kampanjer, pakker og visuelle planer på canvas.', tabId: 'my-account' },
          { id: 'overview-inbox', label: 'Inbox', href: '/inbox', icon: Inbox, description: 'Samtaler og meldinger på tvers av kanaler.', tabId: 'my-account' },
          { id: 'overview-ticketing', label: 'Ticketing', href: '/tickets', icon: TicketCheck, description: 'Saker, SLA, eskalering og eierskap.', tabId: 'my-account' },
          { id: 'overview-social', label: 'Social', href: '/social/calendar', icon: CalendarDays, description: 'Planlegg postkalender, godkjenninger og kanaler.', tabId: 'shared' },
          { id: 'overview-knowledge', label: 'Kunnskap', href: '/knowledge', icon: BookOpen, description: 'Indekserte kilder, status og datakvalitet.', tabId: 'shared' },
          { id: 'overview-insights', label: 'Insights', href: '/insights/overview', icon: BarChart3, description: 'Mål sosialt, inbox, agenter, kampanjer og eksperimenter.', tabId: 'shared' },
          { id: 'overview-agents', label: 'Agenter', href: '/agents', icon: Bot, description: 'Roller, handlinger og operasjonelle grenser.', tabId: 'shared' },
        ],
      },
    ],
  },
  {
    id: 'messages',
    label: 'Chat',
    href: '/chat',
    icon: MessageSquare,
    description: 'Velion AI-chat og oppgaver.',
    panelGroups: [
      {
        id: 'messages-core',
        label: 'Chat',
        items: [
          { id: 'messages-start', label: 'Ny samtale', href: '/chat', icon: Sparkles, description: 'Åpne Velion AI-arbeidsflaten.' },
          { id: 'messages-inbox', label: 'Samtaler', href: '/inbox', icon: Inbox, description: 'Gå til kunde- og internmeldinger.' },
        ],
      },
    ],
  },
  {
    id: 'studio',
    label: 'Studio',
    href: '/studio',
    icon: PanelsTopLeft,
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
    id: 'inbox',
    label: 'Inbox',
    href: '/inbox',
    icon: Inbox,
    description: 'Omnikanal støtte og meldingsflyt.',
    panelGroups: [
      {
        id: 'inbox-core',
        label: 'Inbox',
        collapsible: true,
        defaultExpanded: true,
        items: [
          { id: 'inbox-home', label: 'Your inbox', href: '/inbox', icon: Inbox, description: 'Samtaler som krever oppfølging.' },
          { id: 'inbox-ai', label: 'AI-samtaler', href: '/chat', icon: MessageSquare, description: 'Velion AI-arbeid som kan bli til kundeoppfølging.' },
        ],
      },
    ],
  },
  {
    id: 'ticketing',
    label: 'Ticketing',
    href: '/tickets',
    icon: TicketCheck,
    description: 'Saker, SLA og operasjonell supportkø.',
    panelGroups: [
      {
        id: 'ticketing-core',
        label: 'Queues',
        collapsible: true,
        defaultExpanded: true,
        items: [
          { id: 'ticketing-suggested', label: 'Suggested by AI', href: '/tickets', icon: Bot, description: 'AI-forslag klare for review.' },
          { id: 'ticketing-my', label: 'My tickets', href: '/tickets', icon: UserRound, description: 'Saker som er tildelt deg.' },
          { id: 'ticketing-unassigned', label: 'Unassigned', href: '/tickets', icon: Inbox, description: 'Saker uten eier.' },
          { id: 'ticketing-sla-risk', label: 'SLA risk', href: '/tickets', icon: ShieldCheck, description: 'Saker med tidsrisiko.' },
        ],
      },
    ],
  },
  {
    id: 'social',
    label: 'Social',
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
    ],
  },
  {
    id: 'agents',
    label: 'Agenter',
    href: '/agents',
    icon: Bot,
    description: 'Agentroller og styring.',
    panelGroups: [
      {
        id: 'agents-core',
        label: 'Agenter',
        items: [
          { id: 'agents-all', label: 'Alle agenter', href: '/agents', icon: Bot, description: 'Se og konfigurer agentroller.' },
          { id: 'agents-chat', label: 'Arbeidsflate', href: '/chat', icon: Sparkles, description: 'Test agenten i chat-arbeidsflaten.' },
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
          { id: 'ingestions-chat', label: 'Ask Velion', href: '/chat', icon: MessageSquare, description: 'Planlegg eller trigge ingest-arbeid via chat.' },
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
          { id: 'knowledge-chat', label: 'Spør kunnskapen', href: '/chat', icon: Sparkles, description: 'Bruk Velion AI mot indeksert innhold.' },
        ],
      },
    ],
  },
  {
    id: 'insights',
    label: 'Insights',
    href: '/insights',
    icon: BarChart3,
    description: 'Måling, eksperimenter og ytelse på tvers av Velion.',
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

export function isSidebarPathActive(pathname: string, href: VelionRoute, aliases: string[] = []) {
  const normalizedPathname = normalizePath(pathname)
  const paths = [href, ...aliases].map(normalizePath)

  return paths.some((path) => normalizedPathname === path || normalizedPathname.startsWith(`${path}/`))
}

export function getSidebarSectionForPath(
  pathname: string,
  fallbackRoute: VelionRoute,
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
  const trimmed = path.replace(/\/+$/, '')
  return trimmed.length > 0 ? trimmed : '/'
}
