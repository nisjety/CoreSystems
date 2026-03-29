import type { LucideIcon } from 'lucide-react';
import {
  AtSign,
  BarChart3,
  Bot,
  BookOpen,
  BrainCircuit,
  ChartColumnBig,
  CheckCheck,
  ContactRound,
  FolderKanban,
  GitBranch,
  HandHelping,
  Headphones,
  Home,
  Inbox,
  Layout,
  LayoutDashboard,
  LayoutList,
  LifeBuoy,
  ListChecks,
  Mail,
  MessageSquare,
  MessagesSquare,
  NotebookPen,
  PenLine,
  PlugZap,
  Rocket,
  Search,
  Send,
  Settings,
  ShieldAlert,
  SlidersHorizontal,
  Sparkles,
  UserMinus,
  Users,
  UserCircle2,
  Workflow,
  XCircle,
} from 'lucide-react';

export type SidebarItemStatus = 'live' | 'coming-soon';

export type SharedNavPanelTab = {
  id: string;
  labelKey: string;
  defaultLabel: string;
};

export type NavSubItem = {
  id: string;
  labelKey: string;
  defaultLabel: string;
  href: string;
};

export type SharedNavPanelItem = {
  id: string;
  labelKey: string;
  defaultLabel: string;
  href?: string;
  icon: LucideIcon;
  status?: SidebarItemStatus;
  description?: string;
  aliases?: string[];
  badge?: string | number;
  tabId?: string;
  subItems?: NavSubItem[];
};

export type SharedNavPanelGroup = {
  id: string;
  labelKey: string;
  defaultLabel: string;
  showHeader?: boolean;
  collapsible?: boolean;
  defaultExpanded?: boolean;
  showAddButton?: boolean;
  forceVisible?: boolean;
  alignBottom?: boolean;
  items: SharedNavPanelItem[];
};

export type SharedNavItem = {
  id: string;
  labelKey: string;
  defaultLabel: string;
  href: string;
  icon: LucideIcon;
  description?: string;
  aliases?: string[];
  pinnedBottom?: boolean;
  panelTabs?: SharedNavPanelTab[];
  panelGroups: SharedNavPanelGroup[];
};

export const sharedNavItems: SharedNavItem[] = [
  {
    id: 'overview',
    labelKey: 'overview',
    defaultLabel: 'Overview',
    href: '/overview',
    icon: ChartColumnBig,
    description: 'A crisp summary of activity, recents, and lead momentum.',
    panelTabs: [
      { id: 'my-account', labelKey: 'myAccount', defaultLabel: 'My account' },
      { id: 'shared-with-me', labelKey: 'sharedWithMe', defaultLabel: 'Shared with me' },
    ],
    panelGroups: [
      {
        id: 'overview-personal',
        labelKey: 'overviewGroup',
        defaultLabel: 'Overview',
        items: [
          {
            id: 'overview-home',
            labelKey: 'home',
            defaultLabel: 'Home',
            href: '/dashboard',
            icon: Home,
            description: 'Your workspace home.',
            tabId: 'my-account',
          },
          {
            id: 'overview-dashboard',
            labelKey: 'dashboard',
            defaultLabel: 'Dashboard',
            href: '/overview',
            icon: LayoutDashboard,
            description: 'Personal activity dashboard.',
            tabId: 'my-account',
          },
          {
            id: 'overview-projects',
            labelKey: 'projects',
            defaultLabel: 'Projects',
            href: '/projects',
            icon: FolderKanban,
            description: 'All your active projects.',
            tabId: 'my-account',
          },
          {
            id: 'overview-tasks',
            labelKey: 'tasks',
            defaultLabel: 'Tasks',
            href: '/tasks',
            icon: ListChecks,
            description: 'Your assigned tasks and to-dos.',
            tabId: 'my-account',
          },
        ],
      },
      {
        id: 'overview-shared',
        labelKey: 'sharedGroup',
        defaultLabel: 'Shared',
        items: [
          {
            id: 'overview-shared-spaces',
            labelKey: 'sharedSpaces',
            defaultLabel: 'Shared spaces',
            href: '/overview/shared-spaces',
            icon: Users,
            status: 'coming-soon',
            description: 'Cross-team dashboards and shared operating views.',
            tabId: 'shared-with-me',
          },
        ],
      },
    ],
  },
  {
    id: 'messages',
    labelKey: 'messages',
    defaultLabel: 'Messages',
    href: '/chat',
    icon: MessageSquare,
    description: 'Open conversations, review chat history, and continue prior sessions.',
    panelGroups: [],
  },
  {
    id: 'planner',
    labelKey: 'planner',
    defaultLabel: 'Planner',
    href: '/planner',
    icon: NotebookPen,
    description: 'Write, draw and plan — a collaborative canvas powered by AFFiNE.',
    panelGroups: [
      {
        id: 'planner-core',
        labelKey: 'plannerCore',
        defaultLabel: 'Planner',
        items: [
          {
            id: 'planner-canvas',
            labelKey: 'canvas',
            defaultLabel: 'Canvas',
            href: '/planner',
            icon: Layout,
            description: 'Edgeless whiteboard — shapes, sticky notes, and rich text.',
          },
          {
            id: 'planner-docs',
            labelKey: 'documents',
            defaultLabel: 'Documents',
            href: '/planner?mode=page',
            icon: NotebookPen,
            description: 'Structured document editor with blocks.',
          },
        ],
      },
    ],
  },
  {
    id: 'agents',
    labelKey: 'agents',
    defaultLabel: 'Agents',
    href: '/agents',
    icon: Bot,
    description: 'Configure agent behavior, actions, and operational guardrails.',
    panelGroups: [
      {
        id: 'agents-setup',
        labelKey: 'agentWorkspace',
        defaultLabel: 'Agent workspace',
        items: [
          {
            id: 'agent-settings',
            labelKey: 'agentSettings',
            defaultLabel: 'Agent settings',
            href: '/agents/settings',
            icon: BrainCircuit,
            description: 'Tune prompts, memory, tone, and behavior.',
          },
          {
            id: 'agent-actions',
            labelKey: 'actions',
            defaultLabel: 'Actions',
            href: '/agents/actions',
            icon: Workflow,
            description: 'Manage tools, workflows, and automations.',
          },
        ],
      },
    ],
  },
  {
    id: 'inbox',
    labelKey: 'inbox',
    defaultLabel: 'Inbox',
    href: '/inbox',
    icon: Inbox,
    description: 'Route conversations, review chat history, and manage shared comms.',
    panelGroups: [
      {
        id: 'inbox-core',
        labelKey: 'inbox',
        defaultLabel: 'Inbox',
        showHeader: true,
        collapsible: true,
        defaultExpanded: true,
        items: [
          {
            id: 'inbox-your-inbox',
            labelKey: 'yourInbox',
            defaultLabel: 'Your inbox',
            href: '/inbox',
            icon: Inbox,
            description: 'Conversations assigned to you.',
            badge: '5',
            subItems: [
              { id: 'inbox-channel-all', labelKey: 'allMessages', defaultLabel: 'All messages', href: '/inbox/channels/all' },
              { id: 'inbox-channel-messenger', labelKey: 'messenger', defaultLabel: 'Messenger', href: '/inbox/channels/messenger' },
              { id: 'inbox-channel-instagram', labelKey: 'instagram', defaultLabel: 'Instagram', href: '/inbox/channels/instagram' },
              { id: 'inbox-channel-whatsapp', labelKey: 'whatsapp', defaultLabel: 'WhatsApp', href: '/inbox/channels/whatsapp' },
              { id: 'inbox-channel-email', labelKey: 'email', defaultLabel: 'Email', href: '/inbox/channels/email' },
              { id: 'inbox-channel-twitter', labelKey: 'twitter', defaultLabel: 'Twitter / X', href: '/inbox/channels/twitter' },
              { id: 'inbox-channel-sms', labelKey: 'sms', defaultLabel: 'SMS', href: '/inbox/channels/sms' },
            ],
          },
          {
            id: 'inbox-mentions',
            labelKey: 'mentions',
            defaultLabel: 'Mentions',
            href: '/inbox/mentions',
            icon: AtSign,
            description: 'Conversations where you were mentioned.',
            subItems: [
              { id: 'mentions-all', labelKey: 'allMentions', defaultLabel: 'All mentions', href: '/inbox/mentions' },
              { id: 'mentions-facebook', labelKey: 'facebook', defaultLabel: 'Facebook', href: '/inbox/mentions/facebook' },
              { id: 'mentions-instagram', labelKey: 'instagram', defaultLabel: 'Instagram', href: '/inbox/mentions/instagram' },
              { id: 'mentions-twitter', labelKey: 'twitter', defaultLabel: 'Twitter / X', href: '/inbox/mentions/twitter' },
              { id: 'mentions-linkedin', labelKey: 'linkedin', defaultLabel: 'LinkedIn', href: '/inbox/mentions/linkedin' },
            ],
          },
          {
            id: 'inbox-created-by-you',
            labelKey: 'createdByYou',
            defaultLabel: 'Created by you',
            href: '/inbox/created-by-you',
            icon: PenLine,
            description: 'Conversations you opened.',
          },
          {
            id: 'inbox-all',
            labelKey: 'allConversations',
            defaultLabel: 'All',
            href: '/inbox/all',
            icon: LayoutList,
            description: 'Every conversation in the workspace.',
            badge: '5',
          },
          {
            id: 'inbox-unassigned',
            labelKey: 'unassigned',
            defaultLabel: 'Unassigned',
            href: '/inbox/unassigned',
            icon: UserMinus,
            description: 'Conversations with no assigned agent.',
          },
          {
            id: 'inbox-spam',
            labelKey: 'spam',
            defaultLabel: 'Spam',
            href: '/inbox/spam',
            icon: ShieldAlert,
            description: 'Flagged and filtered conversations.',
          },
          {
            id: 'inbox-dashboard',
            labelKey: 'inboxDashboard',
            defaultLabel: 'Dashboard',
            href: '/inbox/dashboard',
            icon: LayoutDashboard,
            description: 'Inbox performance overview.',
          },
        ],
      },
      {
        id: 'inbox-ai-agent',
        labelKey: 'agents',
        defaultLabel: 'Agents',
        showHeader: true,
        collapsible: true,
        defaultExpanded: true,
        showAddButton: true,
        items: [
          {
            id: 'inbox-ai-all',
            labelKey: 'allConversations',
            defaultLabel: 'All conversations',
            href: '/inbox/ai/all',
            icon: MessagesSquare,
            description: 'All AI-handled conversations.',
          },
          {
            id: 'inbox-ai-resolved',
            labelKey: 'resolved',
            defaultLabel: 'Resolved',
            href: '/inbox/ai/resolved',
            icon: CheckCheck,
            description: 'Conversations resolved by AI.',
          },
          {
            id: 'inbox-ai-routed',
            labelKey: 'routed',
            defaultLabel: 'Routed',
            href: '/inbox/ai/routed',
            icon: GitBranch,
            description: 'Conversations routed to a human agent.',
          },
          {
            id: 'inbox-ai-abandoned',
            labelKey: 'abandoned',
            defaultLabel: 'Abandoned',
            href: '/inbox/ai/abandoned',
            icon: XCircle,
            description: 'Conversations dropped without resolution.',
          },
        ],
      },
      {
        id: 'inbox-team-inboxes',
        labelKey: 'teamInboxes',
        defaultLabel: 'Team inboxes',
        showHeader: true,
        collapsible: true,
        defaultExpanded: true,
        items: [
          {
            id: 'inbox-team-admin-support',
            labelKey: 'adminSupport',
            defaultLabel: 'Admin Support',
            href: '/inbox/teams/admin-support',
            icon: Headphones,
            description: 'Shared inbox for admin and support teams.',
          },
        ],
      },
      {
        id: 'inbox-teammates',
        labelKey: 'teammates',
        defaultLabel: 'Teammates',
        showHeader: true,
        collapsible: true,
        defaultExpanded: false,
        showAddButton: true,
        forceVisible: true,
        items: [],
      },
      {
        id: 'inbox-views',
        labelKey: 'views',
        defaultLabel: 'Views',
        showHeader: true,
        collapsible: true,
        defaultExpanded: true,
        items: [
          {
            id: 'inbox-views-messenger',
            labelKey: 'messenger',
            defaultLabel: 'Messenger',
            href: '/inbox/views/messenger',
            icon: MessageSquare,
            description: 'Messenger channel conversations.',
            badge: '1',
          },
          {
            id: 'inbox-views-email',
            labelKey: 'email',
            defaultLabel: 'Email',
            href: '/inbox/views/email',
            icon: Mail,
            description: 'Email channel conversations.',
            badge: '1',
          },
        ],
      },
      {
        id: 'inbox-manage',
        labelKey: 'manage',
        defaultLabel: 'Manage',
        showHeader: false,
        alignBottom: true,
        forceVisible: true,
        items: [
          {
            id: 'inbox-manage-settings',
            labelKey: 'manage',
            defaultLabel: 'Manage',
            href: '/inbox/manage',
            icon: SlidersHorizontal,
            description: 'Configure inboxes, routing, and integrations.',
          },
        ],
      },
    ],
  },
  {
    id: 'knowledge',
    labelKey: 'knowledge',
    defaultLabel: 'Knowledge',
    href: '/knowledge',
    icon: BookOpen,
    description: 'Your indexed data, sources, integrations, and training layer.',
    panelGroups: [
      {
        id: 'knowledge-core',
        labelKey: 'knowledgeCore',
        defaultLabel: 'Knowledge',
        items: [
          {
            id: 'knowledge-data',
            labelKey: 'data',
            defaultLabel: 'Data',
            href: '/knowledge/data',
            icon: FolderKanban,
            description: 'Structured records, documents, and indexed entities.',
          },
          {
            id: 'knowledge-sources',
            labelKey: 'sources',
            defaultLabel: 'Sources',
            href: '/knowledge/sources',
            icon: Sparkles,
            description: 'Connected websites, drives, and imported repositories.',
          },
          {
            id: 'knowledge-api-integrations',
            labelKey: 'apiIntegrations',
            defaultLabel: 'API integrations',
            href: '/knowledge/api-integrations',
            icon: PlugZap,
            description: 'External APIs and product connectors for live context.',
          },
          {
            id: 'knowledge-training',
            labelKey: 'training',
            defaultLabel: 'Training',
            href: '/knowledge/training',
            icon: BrainCircuit,
            description: 'Refinement loops, review sets, and learning controls.',
          },
        ],
      },
    ],
  },
  {
    id: 'reports',
    labelKey: 'reports',
    defaultLabel: 'Reports',
    href: '/reports',
    icon: BarChart3,
    description: 'Performance analytics, trends, and AI-generated insights.',
    panelGroups: [
      {
        id: 'reports-core',
        labelKey: 'reportsCore',
        defaultLabel: 'Reports',
        items: [
          {
            id: 'reports-analytics',
            labelKey: 'analytics',
            defaultLabel: 'Analytics',
            href: '/reports/analytics',
            icon: BarChart3,
            description: 'Volume, funnel, and channel metrics.',
          },
          {
            id: 'reports-insights',
            labelKey: 'insights',
            defaultLabel: 'Insights',
            href: '/reports/insights',
            icon: Sparkles,
            description: 'AI-synthesized observations and recommended actions.',
          },
        ],
      },
    ],
  },
  {
    id: 'outbound',
    labelKey: 'outbound',
    defaultLabel: 'Outbound',
    href: '/outbound',
    icon: Send,
    description: 'Campaign orchestration, outbound sequences, and follow-through.',
    panelGroups: [
      {
        id: 'outbound-core',
        labelKey: 'outboundCore',
        defaultLabel: 'Outbound',
        items: [
          {
            id: 'outbound-campaigns',
            labelKey: 'campaigns',
            defaultLabel: 'Campaigns',
            href: '/outbound/campaigns',
            icon: Send,
            status: 'coming-soon',
            description: 'Launch coordinated outbound plays and nurture sequences.',
          },
        ],
      },
    ],
  },
  {
    id: 'people',
    labelKey: 'people',
    defaultLabel: 'People',
    href: '/people',
    icon: Users,
    description: 'Contacts, team structure, and lead ownership.',
    aliases: ['/team'],
    panelGroups: [
      {
        id: 'people-core',
        labelKey: 'peopleCore',
        defaultLabel: 'People',
        items: [
          {
            id: 'people-contacts',
            labelKey: 'contacts',
            defaultLabel: 'Contacts',
            href: '/people/contacts',
            icon: ContactRound,
            description: 'Customer records, account owners, and context.',
          },
          {
            id: 'people-teams',
            labelKey: 'teams',
            defaultLabel: 'Teams',
            href: '/team',
            aliases: ['/people/teams'],
            icon: Users,
            description: 'Internal members, permissions, and collaboration roles.',
          },
          {
            id: 'people-lead',
            labelKey: 'lead',
            defaultLabel: 'Lead',
            href: '/people/lead',
            icon: Sparkles,
            description: 'Lead assignment, qualification, and progression.',
          },
        ],
      },
    ],
  },
  {
    id: 'deployment',
    labelKey: 'deployment',
    defaultLabel: 'Deployment',
    href: '/deployment',
    icon: Rocket,
    description: 'Release readiness, rollout controls, and runtime status.',
    panelGroups: [
      {
        id: 'deployment-core',
        labelKey: 'deploymentCore',
        defaultLabel: 'Deployment',
        items: [
          {
            id: 'deployment-checklists',
            labelKey: 'releaseChecklist',
            defaultLabel: 'Release checklist',
            href: '/deployment/checklist',
            icon: Rocket,
            status: 'coming-soon',
            description: 'Readiness checks for shipping product changes safely.',
          },
        ],
      },
    ],
  },
  {
    id: 'profiles',
    labelKey: 'profiles',
    defaultLabel: 'Account',
    href: '/profile',
    icon: UserCircle2,
    pinnedBottom: true,
    description: 'Personal profile, sign-in methods, and notification preferences.',
    panelGroups: [
      {
        id: 'profiles-core',
        labelKey: 'profilesCore',
        defaultLabel: 'Account',
        items: [
          {
            id: 'profiles-account',
            labelKey: 'accountSettings',
            defaultLabel: 'Account settings',
            href: '/profile',
            icon: UserCircle2,
            description: 'Personal details, sign-in methods, and notifications.',
            subItems: [
              { id: 'profiles-account-profile', labelKey: 'profile', defaultLabel: 'Profile', href: '/profile' },
              { id: 'profiles-account-security', labelKey: 'security', defaultLabel: 'Security', href: '/profile/security' },
              { id: 'profiles-account-linked', labelKey: 'linkedAccounts', defaultLabel: 'Linked accounts', href: '/profile/linked-accounts' },
              { id: 'profiles-account-notifications', labelKey: 'notifications', defaultLabel: 'Notifications', href: '/profile/notifications' },
            ],
          },
        ],
      },
    ],
  },
  {
    id: 'settings',
    labelKey: 'settings',
    defaultLabel: 'Settings',
    href: '/settings',
    icon: Settings,
    pinnedBottom: true,
    description: 'Workspace configuration, defaults, and operational controls.',
    panelGroups: [
      {
        id: 'settings-core',
        labelKey: 'settingsCore',
        defaultLabel: 'Settings',
        items: [
          {
            id: 'settings-workspace',
            labelKey: 'workspaceSettings',
            defaultLabel: 'Workspace settings',
            href: '/settings',
            icon: Settings,
            description: 'Workspace defaults, members, and billing.',
            subItems: [
              { id: 'settings-general', labelKey: 'settingsGeneral', defaultLabel: 'General', href: '/settings' },
              { id: 'settings-members', labelKey: 'settingsMembers', defaultLabel: 'Members', href: '/settings/members' },
              { id: 'settings-billing', labelKey: 'settingsBilling', defaultLabel: 'Billing', href: '/settings/billing' },
            ],
          },
        ],
      },
    ],
  },
  {
    id: 'helpdesk',
    labelKey: 'helpdesk',
    defaultLabel: 'Helpdesk',
    href: '/helpdesk',
    icon: LifeBuoy,
    pinnedBottom: true,
    description: 'Support operations, routing, macros, and knowledge handoff.',
    aliases: ['/answers'],
    panelGroups: [
      {
        id: 'helpdesk-core',
        labelKey: 'helpdeskCore',
        defaultLabel: 'Helpdesk',
        items: [
          {
            id: 'helpdesk-knowledge-hub',
            labelKey: 'knowledgeHub',
            defaultLabel: 'Knowledge hub',
            href: '/answers',
            aliases: ['/helpdesk/knowledge-hub'],
            icon: BookOpen,
            description: 'Support answers, playbooks, and operator guidance.',
          },
          {
            id: 'helpdesk-macros',
            labelKey: 'macros',
            defaultLabel: 'Macros',
            href: '/helpdesk/macros',
            icon: Workflow,
            status: 'coming-soon',
            description: 'Reusable responses and workflow shortcuts.',
          },
          {
            id: 'helpdesk-escalations',
            labelKey: 'escalations',
            defaultLabel: 'Escalations',
            href: '/helpdesk/escalations',
            icon: HandHelping,
            status: 'coming-soon',
            description: 'Escalation rules, ownership, and SLA monitoring.',
          },
        ],
      },
    ],
  },
  {
    id: 'search',
    labelKey: 'search',
    defaultLabel: 'Search',
    href: '/search',
    icon: Search,
    pinnedBottom: true,
    description: 'Search across your entire workspace.',
    panelGroups: [
      {
        id: 'search-core',
        labelKey: 'searchCore',
        defaultLabel: 'Search',
        items: [
          {
            id: 'search-global',
            labelKey: 'globalSearch',
            defaultLabel: 'Global search',
            href: '/search',
            icon: Search,
            description: 'Find anything across your workspace.',
          },
        ],
      },
    ],
  },
];

export const navTranslations: Record<string, Record<string, string>> = {
  en: {
    home: 'Home',
    startHere: 'Start here',
    workspaceHome: 'Workspace home',
    globalSearch: 'Global search',
    notifications: 'Notifications',
    overview: 'Overview',
    myAccount: 'My account',
    sharedWithMe: 'Shared with me',
    overviewGroup: 'Overview',
    activity: 'Activity',
    recents: 'Recents',
    leads: 'Leads',
    sharedGroup: 'Shared',
    sharedSpaces: 'Shared spaces',
    agents: 'Agents',
    agentWorkspace: 'Agent workspace',
    agentSettings: 'Agent settings',
    actions: 'Actions',
    inbox: 'Inbox',
    yourInbox: 'Your inbox',
    messages: 'Messages',
    chatLogs: 'Chat logs',
    supportOps: 'Support ops',
    escalations: 'Escalations',
    knowledge: 'Knowledge',
    knowledgeCore: 'Knowledge',
    data: 'Data',
    sources: 'Sources',
    apiIntegrations: 'API integrations',
    training: 'Training',
    reports: 'Reports',
    reportsCore: 'Reports',
    analytics: 'Analytics',
    insights: 'Insights',
    outbound: 'Outbound',
    outboundCore: 'Outbound',
    campaigns: 'Campaigns',
    people: 'People',
    peopleCore: 'People',
    contacts: 'Contacts',
    teams: 'Teams',
    lead: 'Lead',
    deployment: 'Deployment',
    deploymentCore: 'Deployment',
    releaseChecklist: 'Release checklist',
    profiles: 'Account',
    profilesCore: 'Account',
    myProfile: 'My profile',
    accountSettings: 'Account settings',
    profile: 'Profile',
    security: 'Security',
    linkedAccounts: 'Linked accounts',
    settings: 'Settings',
    settingsCore: 'Settings',
    workspaceSettings: 'Workspace settings',
    settingsGeneral: 'General',
    settingsMembers: 'Members',
    settingsBilling: 'Billing',
    dashboard: 'Dashboard',
    projects: 'Projects',
    tasks: 'Tasks',
    myDetails: 'My details',
    allMessages: 'All messages',
    instagram: 'Instagram',
    whatsapp: 'WhatsApp',
    twitter: 'Twitter / X',
    sms: 'SMS',
    allMentions: 'All mentions',
    facebook: 'Facebook',
    linkedin: 'LinkedIn',
    mentions: 'Mentions',
    createdByYou: 'Created by you',
    allConversations: 'All',
    unassigned: 'Unassigned',
    spam: 'Spam',
    inboxDashboard: 'Dashboard',
    resolved: 'Resolved',
    routed: 'Routed',
    abandoned: 'Abandoned',
    teamInboxes: 'Team inboxes',
    teammates: 'Teammates',
    adminSupport: 'Admin Support',
    views: 'Views',
    messenger: 'Messenger',
    email: 'Email',
    manage: 'Manage',
    helpdesk: 'Helpdesk',
    helpdeskCore: 'Helpdesk',
    knowledgeHub: 'Knowledge hub',
    macros: 'Macros',
    search: 'Search',
    searchCore: 'Search',
  },
  no: {
    home: 'Hjem',
    startHere: 'Start her',
    workspaceHome: 'Arbeidsflate',
    globalSearch: 'Globalt søk',
    notifications: 'Varsler',
    overview: 'Oversikt',
    myAccount: 'Min konto',
    sharedWithMe: 'Delt med meg',
    overviewGroup: 'Oversikt',
    activity: 'Aktivitet',
    recents: 'Nylig',
    leads: 'Leads',
    sharedGroup: 'Delt',
    sharedSpaces: 'Delte flater',
    agents: 'Agenter',
    agentWorkspace: 'Agentområde',
    agentSettings: 'Agentinnstillinger',
    actions: 'Handlinger',
    inbox: 'Innboks',
    yourInbox: 'Din innboks',
    messages: 'Meldinger',
    chatLogs: 'Chatlogger',
    supportOps: 'Supportdrift',
    escalations: 'Eskaleringer',
    knowledge: 'Kunnskap',
    knowledgeCore: 'Kunnskap',
    data: 'Data',
    sources: 'Kilder',
    apiIntegrations: 'API-integrasjoner',
    training: 'Trening',
    reports: 'Rapporter',
    reportsCore: 'Rapporter',
    analytics: 'Analyse',
    insights: 'Innsikt',
    outbound: 'Outbound',
    outboundCore: 'Outbound',
    campaigns: 'Kampanjer',
    people: 'Personer',
    peopleCore: 'Personer',
    contacts: 'Kontakter',
    teams: 'Team',
    lead: 'Lead',
    deployment: 'Deploy',
    deploymentCore: 'Deploy',
    releaseChecklist: 'Utgivelsessjekkliste',
    profiles: 'Konto',
    profilesCore: 'Konto',
    myProfile: 'Min profil',
    accountSettings: 'Kontoinnstillinger',
    profile: 'Profil',
    security: 'Sikkerhet',
    linkedAccounts: 'Tilknyttede kontoer',
    settings: 'Innstillinger',
    settingsCore: 'Innstillinger',
    workspaceSettings: 'Arbeidsflateinnstillinger',
    settingsGeneral: 'Generelt',
    settingsMembers: 'Medlemmer',
    settingsBilling: 'Fakturering',
    dashboard: 'Dashboard',
    projects: 'Prosjekter',
    tasks: 'Oppgaver',
    myDetails: 'Min informasjon',
    allMessages: 'Alle meldinger',
    instagram: 'Instagram',
    whatsapp: 'WhatsApp',
    twitter: 'Twitter / X',
    sms: 'SMS',
    allMentions: 'Alle omtaler',
    facebook: 'Facebook',
    linkedin: 'LinkedIn',
    mentions: 'Omtaler',
    createdByYou: 'Opprettet av deg',
    allConversations: 'Alle',
    unassigned: 'Ikke tildelt',
    spam: 'Spam',
    inboxDashboard: 'Dashboard',
    resolved: 'Løst',
    routed: 'Videresendt',
    abandoned: 'Avbrutt',
    teamInboxes: 'Teaminntbokser',
    teammates: 'Lagkamerater',
    adminSupport: 'Adminstøtte',
    views: 'Visninger',
    messenger: 'Messenger',
    email: 'E-post',
    manage: 'Administrer',
    helpdesk: 'Helpdesk',
    helpdeskCore: 'Helpdesk',
    knowledgeHub: 'Kunnskapshub',
    macros: 'Makroer',
    search: 'Søk',
    searchCore: 'Søk',
  },
};

export const getNavLabel = (labelKey: string, defaultLabel: string, locale: string) => {
  const translations = navTranslations[locale] ?? navTranslations.en;
  return translations[labelKey] ?? defaultLabel;
};

function normalizePath(pathname: string): string {
  if (!pathname || pathname === '/') {
    return '/';
  }

  return pathname.replace(/\/+$/, '') || '/';
}

export function isNavPathActive(pathname: string, href?: string, aliases: string[] = []): boolean {
  if (!href) {
    return false;
  }

  const normalizedPath = normalizePath(pathname);
  const candidates = [href, ...aliases].map(normalizePath);

  return candidates.some((candidate) => normalizedPath === candidate || normalizedPath.startsWith(`${candidate}/`));
}

export function getActiveSidebarSection(pathname: string): SharedNavItem | null {
  return sharedNavItems.find((section) => {
    if (isNavPathActive(pathname, section.href, section.aliases)) {
      return true;
    }

    return section.panelGroups.some((group) => (
      group.items.some((item) => isNavPathActive(pathname, item.href, item.aliases))
    ));
  }) ?? null;
}

export function getActiveSidebarItem(pathname: string): SharedNavPanelItem | null {
  const section = getActiveSidebarSection(pathname);

  if (!section) {
    return null;
  }

  for (const group of section.panelGroups) {
    const item = group.items.find((candidate) => isNavPathActive(pathname, candidate.href, candidate.aliases));

    if (item) {
      return item;
    }
  }

  return null;
}
