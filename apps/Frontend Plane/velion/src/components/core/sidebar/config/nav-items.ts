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
  FlaskConical,
  FolderKanban,
  GitBranch,
  GraduationCap,
  HandHelping,
  Headphones,
  Home,
  Inbox,
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
  Zap,
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
  badge?: string;
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
            // U7-1 (ui-ux-verevon-gap.md §U7): `/tasks` renders hardcoded
            // sample data from `dashboard/product-section-pages.tsx`. Hidden
            // from production sidebar (set NEXT_PUBLIC_VEREVON_PREVIEW_ROUTES=1
            // to surface as a dimmed "Soon" item for stakeholder demos).
            status: 'coming-soon',
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
    defaultLabel: 'Verevon Chat',
    href: '/chat',
    icon: MessageSquare,
    description: 'AI chat, operator drafting, and exploratory conversations outside the shared inbox.',
    panelGroups: [],
  },
  {
    id: 'planner',
    labelKey: 'planner',
    defaultLabel: 'Planner',
    href: '/planner',
    icon: NotebookPen,
    description: 'Write, draw, and plan in a collaborative AFFiNE-powered workspace.',
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
            icon: PenLine,
            description: 'Edgeless planning canvas for rich notes and structured thinking.',
          },
          {
            id: 'planner-docs',
            labelKey: 'documents',
            defaultLabel: 'Documents',
            href: '/planner?mode=page',
            icon: LayoutList,
            description: 'Structured documents backed by the planner document model.',
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
        id: 'agents-overview',
        labelKey: 'agentsOverview',
        defaultLabel: 'Agents',
        items: [
          {
            id: 'agents-all-roles',
            labelKey: 'allRoles',
            defaultLabel: 'All roles',
            href: '/agents',
            icon: Sparkles,
            description: 'Browse and configure all prebuilt and custom agent roles.',
          },
        ],
      },
      {
        id: 'agents-lifecycle',
        labelKey: 'agentLifecycle',
        defaultLabel: 'Lifecycle',
        showHeader: true,
        items: [
          // U3-4 (ui-ux-verevon-gap.md §14): these lifecycle items are
          // valid per-agent surfaces (`/agents/{id}/train` etc.) but
          // **don't make sense without an agent selected** — the
          // top-level `/agents/{slug}` route just falls through to the
          // showcase view. Hidden from the sidebar by default; users
          // navigate to them by clicking an agent first and switching
          // tabs inside the workspace. Re-surface with the preview flag
          // for stakeholder demos.
          {
            id: 'agents-train',
            labelKey: 'train',
            defaultLabel: 'Train',
            href: '/agents/train',
            icon: GraduationCap,
            status: 'coming-soon',
            description: 'Refine agent behavior with training data and feedback loops.',
          },
          {
            id: 'agents-test',
            labelKey: 'test',
            defaultLabel: 'Test',
            href: '/agents/test',
            icon: FlaskConical,
            status: 'coming-soon',
            description: 'Run test conversations and validate agent responses.',
          },
          {
            id: 'agents-deploy',
            labelKey: 'agentDeploy',
            defaultLabel: 'Deploy',
            href: '/agents/deploy',
            icon: Rocket,
            status: 'coming-soon',
            description: 'Publish agents to live channels and environments.',
          },
          {
            id: 'agents-analyze',
            labelKey: 'agentAnalyze',
            defaultLabel: 'Analyze',
            href: '/agents/analyze',
            icon: BarChart3,
            status: 'coming-soon',
            description: 'Review performance metrics and conversation outcomes.',
          },
        ],
      },
      {
        id: 'agents-configuration',
        labelKey: 'agentConfiguration',
        defaultLabel: 'Configuration',
        showHeader: true,
        items: [
          // U3-4: same as the lifecycle items above — these belong inside
          // a per-agent workspace, not at the top of the nav.
          {
            id: 'agents-fin-settings',
            labelKey: 'finSettings',
            defaultLabel: 'Fin settings',
            href: '/agents/fin-settings',
            icon: SlidersHorizontal,
            status: 'coming-soon',
            description: 'Tune prompts, memory, tone, and agent behavior.',
          },
          {
            id: 'agents-workflows',
            labelKey: 'agentWorkflows',
            defaultLabel: 'Workflows',
            href: '/agents/workflows',
            icon: Workflow,
            status: 'coming-soon',
            description: 'Manage automated workflows triggered by agent activity.',
          },
          {
            id: 'agents-automations',
            labelKey: 'simpleAutomations',
            defaultLabel: 'Simple automations',
            href: '/agents/automations',
            icon: Zap,
            status: 'coming-soon',
            description: 'Set up lightweight if-this-then-that automations.',
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
          // Wave 11 (ui-ux-verevon-gap.md §19/wave11-knowledge): new IA.
          // Replaces the stale Documents/Sources/API-integrations/Training
          // quartet. Graph + Wiki are Wave 11.C surfaces — their pages
          // 404 when `KNOWLEDGE_GRAPH_ENABLED`/`_WIKI_ENABLED` is unset,
          // so they're safe to surface here unconditionally.
          {
            id: 'knowledge-files',
            labelKey: 'documents',
            defaultLabel: 'Files',
            href: '/knowledge/files',
            icon: FolderKanban,
            description: 'PDFs, Word docs, Markdown, plain text, and CSVs.',
          },
          {
            id: 'knowledge-text',
            labelKey: 'sources',
            defaultLabel: 'Text snippets',
            href: '/knowledge/text',
            icon: Sparkles,
            description: 'Pasted snippets — short rules, brand voice, quick facts.',
          },
          {
            id: 'knowledge-website',
            labelKey: 'training',
            defaultLabel: 'Website',
            href: '/knowledge/website',
            icon: BrainCircuit,
            description: 'Crawl pages from a URL via Quarry. Preview before commit.',
          },
          {
            id: 'knowledge-qa',
            labelKey: 'knowledgeHub',
            defaultLabel: 'Q&A',
            href: '/knowledge/qa',
            icon: PlugZap,
            description: 'Operator-curated questions and answers.',
          },
          {
            id: 'knowledge-graph',
            labelKey: 'knowledgeCore',
            defaultLabel: 'Graph',
            href: '/knowledge/graph',
            icon: Sparkles,
            description: 'GraphRAG entities + relationships (Beta).',
          },
          {
            id: 'knowledge-wiki',
            labelKey: 'knowledgeCore',
            defaultLabel: 'Wiki',
            href: '/knowledge/wiki',
            icon: BookOpen,
            description: 'Logseq-style synthesized wiki pages (Beta).',
          },
          {
            id: 'knowledge-integrations',
            labelKey: 'apiIntegrations',
            defaultLabel: 'Integrations',
            href: '/knowledge/integrations',
            icon: PlugZap,
            description: 'Sync from Google Drive, OneDrive, Notion, Slack, and more.',
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
            // U7-1: /reports/* renders mock data — no Data Plane analytics
            // wiring exists yet. Hidden by default.
            status: 'coming-soon',
            description: 'Volume, funnel, and channel metrics.',
          },
          {
            id: 'reports-insights',
            labelKey: 'insights',
            defaultLabel: 'Insights',
            href: '/reports/insights',
            icon: Sparkles,
            // U7-1: see reports-analytics — same mock-only status.
            status: 'coming-soon',
            description: 'AI-synthesized observations and recommended actions.',
          },
        ],
      },
    ],
  },
  {
    id: 'outbound',
    labelKey: 'outbound',
    defaultLabel: 'Automations',
    href: '/outbound',
    icon: Send,
    description: 'Rules, triggers, macros, routing, and escalation automation.',
    panelGroups: [
      {
        id: 'outbound-core',
        labelKey: 'outboundCore',
        defaultLabel: 'Automations',
        items: [
          {
            id: 'outbound-campaigns',
            labelKey: 'campaigns',
            defaultLabel: 'Automation rules',
            href: '/outbound/campaigns',
            icon: Send,
            status: 'coming-soon',
            description: 'Manage routing, macro, and escalation rule sets.',
          },
        ],
      },
    ],
  },
  {
    id: 'people',
    labelKey: 'people',
    defaultLabel: 'Contacts',
    href: '/people',
    icon: ContactRound,
    description: 'Customer profiles, account context, assignments, and conversation history.',
    aliases: ['/team'],
    panelGroups: [
      {
        id: 'people-core',
        labelKey: 'peopleCore',
        defaultLabel: 'Contacts',
        items: [
          {
            id: 'people-contacts',
            labelKey: 'contacts',
            defaultLabel: 'Contacts',
            href: '/people/contacts',
            icon: ContactRound,
            // U7-1: /people/* is currently a mock CRM surface; org-core
            // members + identity-graph backend exists but is not yet wired.
            status: 'coming-soon',
            description: 'Customer records, account owners, and context.',
          },
          {
            id: 'people-teams',
            labelKey: 'teams',
            defaultLabel: 'Teams',
            href: '/team',
            aliases: ['/people/teams'],
            icon: Users,
            // U7-1: /team renders mock data; org-core has members but the
            // UI does not yet consume it. The richer experience lives in
            // /settings/members for now.
            status: 'coming-soon',
            description: 'Internal members, permissions, and collaboration roles.',
          },
          {
            id: 'people-lead',
            labelKey: 'lead',
            defaultLabel: 'Lead',
            href: '/people/lead',
            icon: Sparkles,
            // U7-1: /people/lead is pure mock — no lead-qualification
            // backend exists in CoreSystem today.
            status: 'coming-soon',
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
              { id: 'settings-integrations', labelKey: 'settingsIntegrations', defaultLabel: 'Integrations', href: '/settings/integrations' },
              { id: 'settings-members', labelKey: 'settingsMembers', defaultLabel: 'Members', href: '/settings/members' },
              { id: 'settings-billing', labelKey: 'settingsBilling', defaultLabel: 'Billing', href: '/settings/billing' },
              // Phase A · A3 — three new sub-pages backed by audit-core +
              // auth-core. Same parent group as the rest of the workspace
              // settings so they sit next to billing/members in the side nav.
              { id: 'settings-usage', labelKey: 'settingsUsage', defaultLabel: 'Usage', href: '/settings/usage' },
              { id: 'settings-api-keys', labelKey: 'settingsApiKeys', defaultLabel: 'API keys', href: '/settings/api-keys' },
              { id: 'settings-audit-log', labelKey: 'settingsAuditLog', defaultLabel: 'Audit log', href: '/settings/audit-log' },
            ],
          },
          {
            id: 'settings-controls',
            labelKey: 'settingsControls',
            defaultLabel: 'Access & control',
            href: '/settings/security',
            icon: SlidersHorizontal,
            description: 'Security, privacy, notification, and advanced workspace controls.',
            subItems: [
              { id: 'settings-security', labelKey: 'settingsSecurity', defaultLabel: 'Security', href: '/settings/security' },
              { id: 'settings-privacy', labelKey: 'settingsPrivacy', defaultLabel: 'Privacy', href: '/settings/privacy' },
              { id: 'settings-permissions', labelKey: 'settingsPermissions', defaultLabel: 'Permissions', href: '/settings/permissions' },
              { id: 'settings-notifications', labelKey: 'settingsNotifications', defaultLabel: 'Notifications', href: '/settings/notifications' },
              { id: 'settings-advanced', labelKey: 'settingsAdvanced', defaultLabel: 'Advanced', href: '/settings/advanced' },
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
            // U7-1: /answers is a mock-only knowledge-base UI — the
            // Model Plane chat path covers the same intent (asking
            // questions against indexed knowledge) and is real.
            status: 'coming-soon',
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

const navTranslations: Record<string, Record<string, string>> = {
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
    agentsOverview: 'Agents',
    allRoles: 'All roles',
    agentLifecycle: 'Lifecycle',
    train: 'Train',
    test: 'Test',
    agentDeploy: 'Deploy',
    agentAnalyze: 'Analyze',
    agentConfiguration: 'Configuration',
    finSettings: 'Fin settings',
    agentWorkflows: 'Workflows',
    simpleAutomations: 'Simple automations',
    agentWorkspace: 'Agent workspace',
    agentSettings: 'Agent settings',
    actions: 'Actions',
    inbox: 'Inbox',
    yourInbox: 'Your inbox',
    messages: 'Verevon Chat',
    chatLogs: 'Chat logs',
    supportOps: 'Support ops',
    escalations: 'Escalations',
    knowledge: 'Knowledge',
    knowledgeCore: 'Knowledge',
    data: 'Data',
    documents: 'Documents',
    sources: 'Sources',
    apiIntegrations: 'API integrations',
    training: 'Training',
    reports: 'Reports',
    reportsCore: 'Reports',
    analytics: 'Analytics',
    insights: 'Insights',
    outbound: 'Automations',
    outboundCore: 'Automations',
    campaigns: 'Automation rules',
    people: 'Contacts',
    peopleCore: 'Contacts',
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
    settingsControls: 'Access & control',
    settingsGeneral: 'General',
    settingsIntegrations: 'Integrations',
    settingsMembers: 'Members',
    settingsBilling: 'Billing',
    settingsSecurity: 'Security',
    settingsPrivacy: 'Privacy',
    settingsPermissions: 'Permissions',
    settingsNotifications: 'Notifications',
    settingsAdvanced: 'Advanced',
    dashboard: 'Dashboard',
    projects: 'Projects',
    tasks: 'Tasks',
    planner: 'Planner',
    plannerCore: 'Planner',
    canvas: 'Canvas',
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
    agentsOverview: 'Agenter',
    allRoles: 'Alle roller',
    agentLifecycle: 'Livssyklus',
    train: 'Tren',
    test: 'Test',
    agentDeploy: 'Distribuer',
    agentAnalyze: 'Analyser',
    agentConfiguration: 'Konfigurasjon',
    finSettings: 'Fin-innstillinger',
    agentWorkflows: 'Arbeidsflyter',
    simpleAutomations: 'Enkle automatiseringer',
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
    documents: 'Dokumenter',
    sources: 'Kilder',
    apiIntegrations: 'API-integrasjoner',
    training: 'Trening',
    reports: 'Rapporter',
    reportsCore: 'Rapporter',
    analytics: 'Analyse',
    insights: 'Innsikt',
    outbound: 'Automatiseringer',
    outboundCore: 'Automatiseringer',
    campaigns: 'Automatiseringsregler',
    people: 'Kontakter',
    peopleCore: 'Kontakter',
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
    settingsControls: 'Tilgang og kontroll',
    settingsGeneral: 'Generelt',
    settingsIntegrations: 'Integrasjoner',
    settingsMembers: 'Medlemmer',
    settingsBilling: 'Fakturering',
    settingsSecurity: 'Sikkerhet',
    settingsPrivacy: 'Personvern',
    settingsPermissions: 'Tillatelser',
    settingsNotifications: 'Varsler',
    settingsAdvanced: 'Avansert',
    dashboard: 'Dashboard',
    projects: 'Prosjekter',
    tasks: 'Oppgaver',
    planner: 'Planner',
    plannerCore: 'Planner',
    canvas: 'Canvas',
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

// ── U7-1 ────────────────────────────────────────────────────────────────────
//
// `ui-ux-verevon-gap.md §U7` flagged 15 mock-only routes — pages that render
// hardcoded sample data and have no `/api/*` proxy or real upstream call.
// Today every one of them appears in the sidebar with `status: 'coming-soon'`
// (so the click is disabled and a "Soon" pill renders), but they still
// surface visually, which forms the wrong mental model for new users.
//
// The closure: filter `status === 'coming-soon'` items out of the rendered
// sidebar by default, AND drop groups that become empty after the filter so
// the layout stays clean. A `NEXT_PUBLIC_VEREVON_PREVIEW_ROUTES=1` env flag
// re-surfaces them as the historical "Soon" preview pills — useful for
// stakeholder demos and the design-review build.
//
// We DON'T delete the entries from `sharedNavItems` because:
//   1. The route components still exist; a designer might want to land on a
//      route directly via URL.
//   2. Re-enabling a route when the backend lands is a one-line
//      `status: 'coming-soon'` removal — much cheaper than re-adding the
//      whole entry from git history.
//   3. The pinned-bottom items (Profile, Settings, Helpdesk, Search) are
//      live and the filter is a no-op for them.
const PREVIEW_ROUTES_FLAG_VALUE = (
  process.env.NEXT_PUBLIC_VEREVON_PREVIEW_ROUTES ?? ''
)
  .trim()
  .toLowerCase();

const PREVIEW_ROUTES_ENABLED =
  PREVIEW_ROUTES_FLAG_VALUE === '1' ||
  PREVIEW_ROUTES_FLAG_VALUE === 'true' ||
  PREVIEW_ROUTES_FLAG_VALUE === 'yes' ||
  PREVIEW_ROUTES_FLAG_VALUE === 'on';

/**
 * Returns true when the item should be displayed in the current build.
 *
 * - When the preview flag is set → always true (renders dimmed via
 *   the existing `isDisabled` path in Navigation.tsx).
 * - Otherwise → filter out `coming-soon` items so the sidebar shows
 *   only routes with a real backend.
 *
 * The function is intentionally tolerant of partial item shapes so it
 * can be called from both the panel-group iteration and the icon-strip
 * iteration (MinimizedNavigation).
 */
export function isNavItemVisibleInCurrentBuild(
  item: { status?: SidebarItemStatus | undefined },
): boolean {
  if (PREVIEW_ROUTES_ENABLED) {
    return true;
  }
  return item.status !== 'coming-soon';
}

/**
 * Apply {@link isNavItemVisibleInCurrentBuild} to a list of panel groups,
 * dropping items that fail the check and discarding any group that ends up
 * empty (so the section's spacing doesn't render as a phantom block).
 *
 * `forceVisible` groups are preserved even when empty — they're typically
 * action affordances (Add teammate, etc.) that should render their
 * showAddButton chrome regardless of item count.
 */
export function filterVisibleGroups(
  groups: readonly SharedNavPanelGroup[],
): SharedNavPanelGroup[] {
  const filtered: SharedNavPanelGroup[] = [];
  for (const group of groups) {
    const items = group.items.filter(isNavItemVisibleInCurrentBuild);
    if (items.length === 0 && !group.forceVisible) {
      continue;
    }
    filtered.push({ ...group, items });
  }
  return filtered;
}

// Top-level sections whose `href` points at a route that currently renders
// hardcoded mock data (per `ui-ux-verevon-gap.md §U7`). The section's panel
// items may not all be mock individually, but the *landing page* the icon
// jumps to is — so hiding the icon prevents a click that lands on stale
// scaffolding. When all the section's panel items are also coming-soon,
// the section is hidden automatically by the empty-groups check below; this
// extra set covers sections whose icon target is the mock surface even when
// some inner items are live (rare today, but useful as the registry grows).
const MOCK_LANDING_SECTION_IDS = new Set<string>([
  'reports',
  'outbound',
  'people',
  'deployment',
]);

/**
 * Decide whether a top-level section icon should render in the minimised
 * sidebar / icon strip. A section is hidden when:
 *   - every one of its panel items is `status: 'coming-soon'` AND the
 *     preview flag isn't set (so we'd just show an empty panel), OR
 *   - the section's landing route is itself mock (per
 *     {@link MOCK_LANDING_SECTION_IDS}).
 *
 * The preview flag short-circuits both checks so demos still see every
 * top-level icon.
 */
export function isNavSectionVisibleInCurrentBuild(
  section: SharedNavItem,
): boolean {
  if (PREVIEW_ROUTES_ENABLED) {
    return true;
  }
  if (MOCK_LANDING_SECTION_IDS.has(section.id)) {
    return false;
  }
  // Compute remaining items after applying the per-item filter. If at least
  // one panel item survives — OR the section has no panel groups at all
  // (e.g. Chat, Search) — keep the icon. Empty panel-groups + zero items
  // means the panel will only render "Nothing is configured here yet", so
  // hide it.
  if (section.panelGroups.length === 0) {
    return true;
  }
  for (const group of section.panelGroups) {
    if (group.items.some(isNavItemVisibleInCurrentBuild)) {
      return true;
    }
  }
  return false;
}
