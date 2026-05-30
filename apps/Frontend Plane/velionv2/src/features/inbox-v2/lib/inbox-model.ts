import type { LucideIcon } from "lucide-react";
import {
  AtSign,
  CheckCheck,
  CircleX,
  GitBranch,
  Headphones,
  Inbox,
  LayoutGrid,
  List,
  Mail,
  MessageSquare,
  MessagesSquare,
  PenLine,
  SlidersHorizontal,
  ShieldAlert,
  UserRound,
} from "lucide-react";

export type InboxTab = "all" | "open" | "pending" | "solved";
export type InboxSidebarView =
  | "mine"
  | "mentions"
  | "created-by-you"
  | "all"
  | "unassigned"
  | "spam"
  | "dashboard"
  | "ai-all"
  | "ai-resolved"
  | "ai-routed"
  | "ai-abandoned"
  | "agent-all"
  | "agent-solved"
  | "agent-forwarded"
  | "agent-cancelled"
  | "team-admin-support"
  | "view-messenger"
  | "view-email"
  | "view-social"
  | "manage";

export type SidebarItem = {
  id: InboxSidebarView;
  label: string;
  href: string;
  icon: LucideIcon;
  badge?: number | string;
  trailing?: boolean;
  subItems?: SidebarSubItem[];
};

export type SidebarChannel = {
  id: string;
  label: string;
  href: string;
};

export type SidebarSubItem = {
  id: string;
  label: string;
  href: string;
};

export type SidebarGroup = {
  id: string;
  label: string;
  defaultExpanded: boolean;
  showAddButton?: boolean;
  alignBottom?: boolean;
  emptyLabel?: string;
  items: SidebarItem[];
};

export type ZammadTicket = {
  id: number;
  number: string;
  title: string;
  state?: { id: number; name: string };
  priority?: { id: number; name: string };
  group?: { id: number; name: string };
  owner?: { id: number; firstname: string; lastname: string; email: string } | null;
  customer?: { id: number; firstname: string; lastname: string; email: string } | null;
  tags?: string[];
  created_at: string;
  updated_at: string;
  article_count?: number;
};

export type ZammadArticle = {
  id: number;
  ticket_id?: number;
  type?: string;
  internal?: boolean;
  body?: string;
  from?: string;
  sender?: string;
  created_at: string;
};

export type Agent = {
  id: number;
  firstname: string;
  lastname: string;
  email: string;
};

export type Group = {
  id: number;
  name: string;
};

export type Macro = {
  id: number;
  name: string;
};

export type CustomerContext = {
  shopify?: {
    orders?: Array<{
      id: string | number;
      name?: string;
      order_number?: string | number;
      fulfillment_status?: string | null;
      created_at?: string;
      total_price?: string;
    }>;
  } | null;
  stripe?: {
    customer?: {
      id?: string;
      email?: string;
    } | null;
    subscription?: {
      status?: string;
      plan?: string;
    } | null;
  } | null;
};

export type TicketSentiment = {
  sentiment: string;
  score: number;
};

export type InboxRouteFilter = {
  activeTab: InboxTab;
  assigned?: "mine" | "unassigned" | "all";
  channel?: string;
  queue?: string;
  agentState?: string;
  label: string;
};

const inboxChannels: SidebarChannel[] = [
  { id: "all", label: "All messages", href: "/inbox?view=mine&channel=all" },
  { id: "messenger", label: "Messenger", href: "/inbox?view=mine&channel=messenger" },
  { id: "instagram", label: "Instagram", href: "/inbox?view=mine&channel=instagram" },
  { id: "whatsapp", label: "WhatsApp", href: "/inbox?view=mine&channel=whatsapp" },
  { id: "email", label: "Email", href: "/inbox?view=mine&channel=email" },
  { id: "twitter", label: "Twitter / X", href: "/inbox?view=mine&channel=twitter" },
  { id: "sms", label: "SMS", href: "/inbox?view=mine&channel=sms" },
];

const inboxMentionChannels: SidebarSubItem[] = [
  { id: "mentions-all", label: "All mentions", href: "/inbox?view=mentions" },
  { id: "mentions-facebook", label: "Facebook", href: "/inbox?view=mentions&channel=facebook" },
  { id: "mentions-instagram", label: "Instagram", href: "/inbox?view=mentions&channel=instagram" },
  { id: "mentions-twitter", label: "Twitter / X", href: "/inbox?view=mentions&channel=twitter" },
  { id: "mentions-linkedin", label: "LinkedIn", href: "/inbox?view=mentions&channel=linkedin" },
];

const inboxPrimarySidebarItems: SidebarItem[] = [
  { id: "mentions", label: "Mentions", icon: AtSign, href: "/inbox?view=mentions", trailing: true, subItems: inboxMentionChannels },
  { id: "created-by-you", label: "Created by you", icon: PenLine, href: "/inbox?view=created-by-you" },
  { id: "all", label: "All", icon: List, href: "/inbox?view=all", badge: 5 },
  { id: "unassigned", label: "Unassigned", icon: UserRound, href: "/inbox?view=unassigned" },
  { id: "spam", label: "Spam", icon: ShieldAlert, href: "/inbox?view=spam" },
  { id: "dashboard", label: "Dashboard", icon: LayoutGrid, href: "/inbox?view=dashboard" },
];

const inboxAgentSidebarItems: SidebarItem[] = [
  { id: "ai-all", label: "All conversations", icon: MessagesSquare, href: "/inbox?view=ai-all" },
  { id: "ai-resolved", label: "Resolved", icon: CheckCheck, href: "/inbox?view=ai-resolved" },
  { id: "ai-routed", label: "Routed", icon: GitBranch, href: "/inbox?view=ai-routed" },
  { id: "ai-abandoned", label: "Abandoned", icon: CircleX, href: "/inbox?view=ai-abandoned" },
];

const inboxHomeItem: SidebarItem = {
  id: "mine",
  label: "Your inbox",
  icon: Inbox,
  href: "/inbox?view=mine",
  badge: 5,
  subItems: inboxChannels,
};

const inboxTeamSidebarItems: SidebarItem[] = [
  {
    id: "team-admin-support",
    label: "Admin Support",
    icon: Headphones,
    href: "/inbox?view=team-admin-support",
  },
];

const inboxViewSidebarItems: SidebarItem[] = [
  {
    id: "view-messenger",
    label: "Messenger",
    icon: MessageSquare,
    href: "/inbox?view=view-messenger",
    badge: 1,
  },
  {
    id: "view-email",
    label: "Email",
    icon: Mail,
    href: "/inbox?view=view-email",
    badge: 1,
  },
  {
    id: "view-social",
    label: "WhatsApp & Social",
    icon: MessageSquare,
    href: "/inbox?view=view-social",
    badge: 2,
  },
];

const inboxManageSidebarItem: SidebarItem = {
  id: "manage",
  label: "Manage",
  icon: SlidersHorizontal,
  href: "/inbox?view=manage",
};

export const inboxSidebarGroups: SidebarGroup[] = [
  {
    id: "inbox-core",
    label: "Inbox",
    defaultExpanded: true,
    items: [inboxHomeItem, ...inboxPrimarySidebarItems],
  },
  {
    id: "inbox-ai-agent",
    label: "Velion AI Agent",
    defaultExpanded: true,
    showAddButton: true,
    items: inboxAgentSidebarItems,
  },
  {
    id: "inbox-team-inboxes",
    label: "Team inboxes",
    defaultExpanded: true,
    items: inboxTeamSidebarItems,
  },
  {
    id: "inbox-teammates",
    label: "Teammates",
    defaultExpanded: false,
    showAddButton: true,
    emptyLabel: "No teammates added yet.",
    items: [],
  },
  {
    id: "inbox-views",
    label: "Views",
    defaultExpanded: true,
    items: inboxViewSidebarItems,
  },
  {
    id: "inbox-manage",
    label: "Manage",
    defaultExpanded: true,
    alignBottom: true,
    items: [inboxManageSidebarItem],
  },
];

export function resolveInboxRouteFilter(searchParams: URLSearchParams): InboxRouteFilter {
  const view = (searchParams.get("view") || "mine") as InboxSidebarView;
  const channel = searchParams.get("channel") || undefined;

  if (view === "mentions") {
    return {
      activeTab: "open",
      queue: "mentions",
      channel,
      label: channel ? `Mentions · ${capitalize(channel.replace(/-/g, " "))}` : "Mentions",
    };
  }

  if (channel) {
    return {
      activeTab: channel === "all" ? "all" : "open",
      assigned: "mine",
      channel,
      label: channel === "all" ? "All messages" : capitalize(channel.replace(/-/g, " ")),
    };
  }

  switch (view) {
    case "created-by-you":
      return { activeTab: "all", queue: "created-by-you", label: "Created by you" };
    case "all":
      return { activeTab: "all", assigned: "all", label: "All conversations" };
    case "unassigned":
      return { activeTab: "open", assigned: "unassigned", label: "Unassigned" };
    case "spam":
      return { activeTab: "all", queue: "spam", label: "Spam" };
    case "dashboard":
      return { activeTab: "all", queue: "dashboard", label: "Dashboard" };
    case "ai-all":
    case "agent-all":
      return { activeTab: "all", agentState: "all", label: "Agent conversations" };
    case "ai-resolved":
    case "agent-solved":
      return { activeTab: "solved", agentState: "resolved", label: "Solved" };
    case "ai-routed":
    case "agent-forwarded":
      return { activeTab: "all", agentState: "routed", label: "Routed" };
    case "ai-abandoned":
    case "agent-cancelled":
      return { activeTab: "all", agentState: "abandoned", label: "Abandoned" };
    case "team-admin-support":
      return { activeTab: "open", queue: "Admin Support", label: "Admin Support" };
    case "view-messenger":
      return { activeTab: "open", assigned: "all", channel: "messenger", label: "Messenger" };
    case "view-email":
      return { activeTab: "open", assigned: "all", channel: "email", label: "Email" };
    case "view-social":
      return { activeTab: "open", assigned: "all", channel: "whatsapp", label: "WhatsApp & Social" };
    case "manage":
      return { activeTab: "all", queue: "manage", label: "Manage" };
    case "mine":
    default:
      return { activeTab: "open", assigned: "mine", label: "Your inbox" };
  }
}

export function searchParamsFromInboxSlug(slug: string[] = []) {
  const [section, value] = slug;
  const params = new URLSearchParams();

  if (!section) {
    params.set("view", "mine");
    return params;
  }

  if (section === "channels") {
    params.set("view", "mine");
    if (value) params.set("channel", value);
    return params;
  }

  if (section === "mentions") {
    params.set("view", "mentions");
    if (value) params.set("channel", value);
    return params;
  }

  if (section === "ai") {
    const aiViewByValue: Record<string, InboxSidebarView> = {
      all: "ai-all",
      resolved: "ai-resolved",
      solved: "ai-resolved",
      routed: "ai-routed",
      forwarded: "ai-routed",
      abandoned: "ai-abandoned",
      cancelled: "ai-abandoned",
    };
    params.set("view", aiViewByValue[value ?? "all"] ?? "ai-all");
    return params;
  }

  if (section === "teams") {
    params.set("view", value === "admin-support" ? "team-admin-support" : "team-admin-support");
    return params;
  }

  if (section === "views") {
    if (value === "email") params.set("view", "view-email");
    else if (value === "social" || value === "whatsapp-social") params.set("view", "view-social");
    else params.set("view", "view-messenger");
    return params;
  }

  params.set("view", section);
  return params;
}

export function customerName(ticket: ZammadTicket) {
  if (!ticket.customer) return "Unknown";
  return `${ticket.customer.firstname} ${ticket.customer.lastname}`.trim() || ticket.customer.email || "Unknown";
}

export function customerInitials(ticket: ZammadTicket) {
  if (!ticket.customer) return "?";
  return `${ticket.customer.firstname.charAt(0)}${ticket.customer.lastname.charAt(0)}`.toUpperCase() || "?";
}


export function formatRelativeTime(iso: string) {
  const diffMs = Date.now() - new Date(iso).getTime();
  const mins = Math.max(0, Math.floor(diffMs / 60_000));
  if (mins < 60) return `${mins}m`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}h`;
  return `${Math.floor(hours / 24)}d`;
}

export function formatDate(iso: string) {
  return new Date(iso).toLocaleDateString("en-GB", {
    day: "numeric",
    month: "short",
    year: "numeric",
  });
}

export function formatTimestamp(iso: string) {
  return new Date(iso).toLocaleTimeString("en-GB", {
    hour: "2-digit",
    minute: "2-digit",
  });
}

export function stripHtml(value = "") {
  return value.replace(/<[^>]*>/g, " ").replace(/\s+/g, " ").trim();
}

function capitalize(value: string) {
  return value.charAt(0).toUpperCase() + value.slice(1);
}
