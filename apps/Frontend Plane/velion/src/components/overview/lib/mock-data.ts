import type {
  ActivityEvent,
  DashboardSnapshot,
  LeadOpportunity,
  OverviewMetrics,
  RecentItem,
  SharedSpace,
} from './overview-types';

export const mockMetrics: OverviewMetrics = {
  activeAgents: {
    label: 'Active Agents',
    value: 12,
    trend: { value: 3, direction: 'up' },
  },
  inProgressConversations: {
    label: 'In Progress',
    value: 47,
    trend: { value: 12, direction: 'up' },
  },
  leadQueueSize: {
    label: 'Lead Queue',
    value: 23,
    trend: { value: 5, direction: 'down' },
  },
  recentActivityCount: {
    label: 'Recent Updates',
    value: 156,
    unit: 'today',
    trend: { value: 22, direction: 'up' },
  },
  teamCapacity: {
    label: 'Team Capacity',
    value: '78%',
    trend: { value: 2, direction: 'stable' },
  },
  systemHealth: {
    label: 'System Health',
    value: '100%',
    trend: { value: 0, direction: 'stable' },
  },
};

export const mockActivityEvents: ActivityEvent[] = [
  {
    id: 'evt-1',
    type: 'agent_action',
    title: 'Agent Fin completed conversation analysis',
    description: 'Analyzed 3 customer conversations for sentiment and intent.',
    actor: { id: 'agent-1', name: 'Fin' },
    timestamp: new Date(Date.now() - 5 * 60000),
    severity: 'info',
    href: '/agents',
  },
  {
    id: 'evt-2',
    type: 'lead_update',
    title: 'New lead qualified: Acme Corp',
    description: 'Lead moved from new to qualified status.',
    actor: { id: 'user-1', name: 'Sarah Chen' },
    timestamp: new Date(Date.now() - 15 * 60000),
    resource: { id: 'lead-1', type: 'lead', name: 'Acme Corp' },
    severity: 'info',
    href: '/people/lead',
  },
  {
    id: 'evt-3',
    type: 'conversation',
    title: 'Customer support ticket resolved',
    description: 'Ticket #4521 marked as resolved by support team.',
    timestamp: new Date(Date.now() - 25 * 60000),
    severity: 'info',
    href: '/inbox',
  },
  {
    id: 'evt-4',
    type: 'document_indexed',
    title: 'Knowledge base updated',
    description: '5 new documents indexed and searchable.',
    actor: { id: 'user-2', name: 'Marcus Reid' },
    timestamp: new Date(Date.now() - 45 * 60000),
    severity: 'info',
    href: '/knowledge/documents',
  },
  {
    id: 'evt-5',
    type: 'deployment',
    title: 'Agent behavior update deployed',
    description: 'New prompt configuration deployed to 4 agents.',
    timestamp: new Date(Date.now() - 60 * 60000),
    severity: 'info',
    href: '/deployment',
  },
];

export const mockRecentItems: RecentItem[] = [
  {
    id: 'rec-1',
    type: 'conversation',
    title: 'Q4 Sales Strategy Discussion',
    description: 'Conversation with sales team about Q4 targets.',
    lastTouched: new Date(Date.now() - 2 * 60000),
    actor: { id: 'user-1', name: 'Sarah' },
    tags: ['sales', 'strategy'],
    href: '/chat/rec-1',
  },
  {
    id: 'rec-2',
    type: 'record',
    title: 'Acme Corp Customer Profile',
    description: 'Key account with $500K ARR, renewal due Q2.',
    lastTouched: new Date(Date.now() - 15 * 60000),
    tags: ['account', 'enterprise'],
    href: '/people/contacts/rec-2',
  },
  {
    id: 'rec-3',
    type: 'task',
    title: 'Review agent training results',
    description: 'Analyze performance metrics from latest training batch.',
    lastTouched: new Date(Date.now() - 45 * 60000),
    actor: { id: 'user-2', name: 'Marcus' },
    tags: ['agents', 'training'],
    href: '/agents/train',
  },
];

export const mockLeads: LeadOpportunity[] = [
  {
    id: 'lead-1',
    name: 'TechStartup Inc',
    company: 'TechStartup Inc',
    stage: 'new',
    score: 85,
    source: 'LinkedIn',
    assignedTo: { id: 'user-1', name: 'Sarah Chen' },
    lastActivity: new Date(Date.now() - 3 * 60000),
    nextAction: 'Send intro email',
    href: '/people/lead/lead-1',
  },
  {
    id: 'lead-2',
    name: 'Enterprise Solutions Ltd',
    company: 'Enterprise Solutions Ltd',
    stage: 'qualified',
    score: 92,
    source: 'Referral',
    assignedTo: { id: 'user-2', name: 'Marcus Reid' },
    lastActivity: new Date(Date.now() - 30 * 60000),
    nextAction: 'Schedule demo',
    href: '/people/lead/lead-2',
  },
];

export const mockSharedSpaces: SharedSpace[] = [
  {
    id: 'space-1',
    name: 'Sales Operations Hub',
    type: 'team',
    members: 8,
    lastUpdated: new Date(Date.now() - 30 * 60000),
    description: 'Shared dashboard for cross-functional sales team.',
    status: 'active',
    href: '/overview/shared-spaces/space-1',
  },
  {
    id: 'space-2',
    name: 'Q4 Product Launch',
    type: 'project',
    members: 12,
    lastUpdated: new Date(Date.now() - 2 * 3600000),
    description: 'Cross-team project view for launch coordination.',
    status: 'active',
    href: '/overview/shared-spaces/space-2',
  },
];
