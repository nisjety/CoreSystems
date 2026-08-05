import type { Component } from 'solid-js'
import {
  Bot,
  BrainCircuit,
  ChartNoAxesCombined,
  CalendarDays,
  BarChart3,
  Inbox,
  MessageSquareText,
  Network,
  PanelsTopLeft,
  Settings,
  Sparkles,
} from 'lucide-solid'

type SurfaceId =
  | 'dashboard'
  | 'chat'
  | 'studio'
  | 'inbox'
  | 'social'
  | 'insights'
  | 'agents'
  | 'knowledge'
  | 'onboarding'
  | 'settings'

export type NavItem = {
  id: SurfaceId
  href: string
  label: string
  description: string
  icon: Component<{ class?: string; size?: number }>
}

export const navItems: readonly NavItem[] = [
  {
    id: 'dashboard',
    href: '/dashboard',
    label: 'Dashboard',
    description: 'Operational health, risk, and active AI work.',
    icon: ChartNoAxesCombined,
  },
  {
    id: 'chat',
    href: '/chat',
    label: 'Chat',
    description: 'Plan work with Verevon and inspect grounded actions.',
    icon: MessageSquareText,
  },
  {
    id: 'studio',
    href: '/studio',
    label: 'Studio',
    description: 'Visual canvas for campaigns, content packs, scripts, and assets.',
    icon: PanelsTopLeft,
  },
  {
    id: 'inbox',
    href: '/support',
    label: 'Support',
    description: 'Conversations, cases, AI review, ownership, and SLA in one workspace.',
    icon: Inbox,
  },
  {
    id: 'social',
    href: '/social/calendar',
    label: 'Social',
    description: 'Post calendar, account readiness, drafts, and approvals.',
    icon: CalendarDays,
  },
  {
    id: 'agents',
    href: '/agents',
    label: 'Agents',
    description: 'Roles, tools, policies, and deployment controls.',
    icon: Bot,
  },
  {
    id: 'knowledge',
    href: '/knowledge',
    label: 'Knowledge',
    description: 'Sources, graph health, chunks, and answer coverage.',
    icon: Network,
  },
  {
    id: 'insights',
    href: '/insights',
    label: 'Insights',
    description: 'Measure social, inbox, agent, campaign, and experiment performance.',
    icon: BarChart3,
  },
  {
    id: 'onboarding',
    href: '/onboarding',
    label: 'Onboarding',
    description: 'Company discovery, crawl proof, graph preview, and plan.',
    icon: Sparkles,
  },
  {
    id: 'settings',
    href: '/settings',
    label: 'Settings',
    description: 'Organization, security, billing, and approval policy.',
    icon: Settings,
  },
]

export const takeoverIcon = BrainCircuit
