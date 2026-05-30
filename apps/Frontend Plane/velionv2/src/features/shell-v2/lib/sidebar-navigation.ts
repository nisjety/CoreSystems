import {
  BarChart3,
  BookOpen,
  Bot,
  CreditCard,
  CircleUserRound,
  Home,
  Inbox,
  MessageSquare,
  Search,
  ShieldCheck,
  Settings,
  Sparkles,
  type LucideIcon,
} from "lucide-react";
import type { VelionRoute } from "@/features/shell-v2/lib/shell-data";

export type SidebarPanelTab = {
  id: string;
  label: string;
};

export type SidebarPanelItem = {
  id: string;
  label: string;
  href: VelionRoute;
  icon: LucideIcon;
  description: string;
  aliases?: string[];
  tabId?: string;
  subItems?: Array<{
    id: string;
    label: string;
    href: VelionRoute;
  }>;
};

export type SidebarPanelGroup = {
  id: string;
  label: string;
  showHeader?: boolean;
  collapsible?: boolean;
  defaultExpanded?: boolean;
  alignBottom?: boolean;
  items: SidebarPanelItem[];
};

export type SidebarSection = {
  id: string;
  label: string;
  href: VelionRoute;
  icon: LucideIcon;
  description: string;
  pinnedBottom?: boolean;
  skipActiveMatch?: boolean;
  panelTabs?: SidebarPanelTab[];
  panelGroups: SidebarPanelGroup[];
};

export const sidebarSections: SidebarSection[] = [
  {
    id: "overview",
    label: "Oversikt",
    href: "/dashboard",
    icon: BarChart3,
    description: "Arbeidsflate, snarveier og operasjonell status.",
    panelTabs: [
      { id: "my-account", label: "Min konto" },
      { id: "shared", label: "Delt med meg" },
    ],
    panelGroups: [
      {
        id: "overview-core",
        label: "Oversikt",
        items: [
          {
            id: "overview-home",
            label: "Hjem",
            href: "/dashboard",
            icon: Home,
            description: "Tilbake til Velion Home.",
            tabId: "my-account",
          },
          {
            id: "overview-chat",
            label: "Velion Chat",
            href: "/chat",
            icon: MessageSquare,
            description: "Start eller fortsett arbeid med AI.",
            tabId: "my-account",
          },
          {
            id: "overview-inbox",
            label: "Inbox",
            href: "/inbox",
            icon: Inbox,
            description: "Samtaler og meldinger på tvers av kanaler.",
            tabId: "my-account",
          },
          {
            id: "overview-knowledge",
            label: "Kunnskap",
            href: "/knowledge",
            icon: BookOpen,
            description: "Indekserte kilder, status og datakvalitet.",
            tabId: "shared",
          },
          {
            id: "overview-agents",
            label: "Agenter",
            href: "/agents",
            icon: Bot,
            description: "Roller, handlinger og operasjonelle grenser.",
            tabId: "shared",
          },
        ],
      },
    ],
  },
  {
    id: "messages",
    label: "Chat",
    href: "/chat",
    icon: MessageSquare,
    description: "Velion AI-chat og oppgaver.",
    panelGroups: [
      {
        id: "messages-core",
        label: "Chat",
        items: [
          {
            id: "messages-start",
            label: "Ny samtale",
            href: "/chat",
            icon: Sparkles,
            description: "Åpne Velion AI-arbeidsflaten.",
          },
          {
            id: "messages-inbox",
            label: "Samtaler",
            href: "/inbox",
            icon: Inbox,
            description: "Gå til kunde- og internmeldinger.",
          },
        ],
      },
    ],
  },
  {
    id: "agents",
    label: "Agenter",
    href: "/agents",
    icon: Bot,
    description: "Agentroller og styring.",
    panelGroups: [
      {
        id: "agents-core",
        label: "Agenter",
        items: [
          {
            id: "agents-all",
            label: "Alle agenter",
            href: "/agents",
            icon: Bot,
            description: "Se og konfigurer agentroller.",
          },
          {
            id: "agents-chat",
            label: "Arbeidsflate",
            href: "/chat",
            icon: Sparkles,
            description: "Test agenten i chat-arbeidsflaten.",
          },
        ],
      },
    ],
  },
  {
    id: "inbox",
    label: "Inbox",
    href: "/inbox",
    icon: Inbox,
    description: "Omnikanal støtte og meldingsflyt.",
    panelGroups: [
      {
        id: "inbox-core",
        label: "Inbox",
        collapsible: true,
        defaultExpanded: true,
        items: [
          {
            id: "inbox-home",
            label: "Your inbox",
            href: "/inbox",
            icon: Inbox,
            description: "Samtaler som krever oppfølging.",
          },
          {
            id: "inbox-ai",
            label: "AI-samtaler",
            href: "/chat",
            icon: MessageSquare,
            description: "Velion AI-arbeid som kan bli til kundeoppfølging.",
          },
        ],
      },
    ],
  },
  {
    id: "knowledge",
    label: "Kunnskap",
    href: "/knowledge",
    icon: BookOpen,
    description: "Kilder, indeksering og kunnskapsstatus.",
    panelGroups: [
      {
        id: "knowledge-core",
        label: "Kunnskap",
        items: [
          {
            id: "knowledge-overview",
            label: "Datakilder",
            href: "/knowledge",
            icon: BookOpen,
            description: "Koble, vurder og overvåk kilder.",
          },
          {
            id: "knowledge-chat",
            label: "Spør kunnskapen",
            href: "/chat",
            icon: Sparkles,
            description: "Bruk Velion AI mot indeksert innhold.",
          },
        ],
      },
    ],
  },
  {
    id: "settings",
    label: "Innstillinger",
    href: "/settings",
    icon: Settings,
    pinnedBottom: true,
    description: "Arbeidsflate, medlemmer og kontroll.",
    panelGroups: [
      {
        id: "settings-core",
        label: "Innstillinger",
        items: [
          {
            id: "settings-workspace",
            label: "Workspace",
            href: "/settings/workspace",
            icon: Settings,
            description: "Identitet, tilgang og integrasjoner.",
          },
          {
            id: "settings-members",
            label: "Members",
            href: "/settings/members",
            icon: CircleUserRound,
            description: "Roller, seter og invitasjoner.",
          },
          {
            id: "settings-billing",
            label: "Billing",
            href: "/settings/billing",
            icon: CreditCard,
            description: "Plan, bruk og fakturaer.",
          },
          {
            id: "settings-security",
            label: "Security",
            href: "/settings/org-security",
            icon: ShieldCheck,
            description: "SSO, domener og organisasjonssikkerhet.",
          },
        ],
      },
    ],
  },
];

export const sidebarSearchAction = {
  id: "search",
  label: "Søk",
  icon: Search,
  pinnedBottom: true,
} as const;

export function isSidebarPathActive(pathname: string, href: VelionRoute, aliases: string[] = []) {
  const normalizedPathname = normalizePath(pathname);
  const paths = [href, ...aliases].map(normalizePath);

  return paths.some((path) => normalizedPathname === path || normalizedPathname.startsWith(`${path}/`));
}

export function getSidebarSectionForPath(pathname: string, fallbackRoute: VelionRoute) {
  const normalizedPathname = normalizePath(pathname);

  return (
    sidebarSections.find((section) => !section.skipActiveMatch && isSidebarPathActive(normalizedPathname, section.href)) ??
    sidebarSections.find((section) => section.href === fallbackRoute) ??
    sidebarSections[0]
  );
}

function normalizePath(path: string) {
  const trimmed = path.replace(/\/+$/, "");
  return trimmed.length > 0 ? trimmed : "/";
}
