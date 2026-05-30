// Snapshots for the overview landing page
export interface DashboardSnapshot {
  label: string;
  value: string | number;
  unit?: string;
  trend?: {
    value: number;
    direction: 'up' | 'down' | 'stable';
  };
  href?: string;
}

export interface OverviewMetrics {
  activeAgents: DashboardSnapshot;
  inProgressConversations: DashboardSnapshot;
  leadQueueSize: DashboardSnapshot;
  recentActivityCount: DashboardSnapshot;
  teamCapacity: DashboardSnapshot;
  systemHealth: DashboardSnapshot;
}

// Activity stream for recent changes
export interface ActivityEvent {
  id: string;
  type: 'conversation' | 'agent_action' | 'lead_update' | 'document_indexed' | 'team_update' | 'deployment';
  title: string;
  description?: string;
  actor?: {
    id: string;
    name: string;
    avatar?: string;
  };
  timestamp: Date;
  resource?: {
    id: string;
    type: string;
    name: string;
  };
  severity?: 'info' | 'warning' | 'critical';
  href?: string;
}

// Recent items (work, records, conversations)
export interface RecentItem {
  id: string;
  type: 'conversation' | 'record' | 'agent_memory' | 'document' | 'task';
  title: string;
  description?: string;
  lastTouched: Date;
  actor?: {
    id: string;
    name: string;
  };
  previewUrl?: string;
  tags?: string[];
  href: string;
}

// Lead flow information
export interface LeadOpportunity {
  id: string;
  name: string;
  company?: string;
  stage: 'new' | 'qualified' | 'contacted' | 'in_progress' | 'ready_for_followup';
  score: number;
  source: string;
  assignedTo?: {
    id: string;
    name: string;
  };
  lastActivity: Date;
  nextAction?: string;
  href: string;
}

// Shared workspace view
export interface SharedSpace {
  id: string;
  name: string;
  type: 'team' | 'project' | 'workflow';
  members: number;
  lastUpdated: Date;
  description?: string;
  href?: string;
  status: 'active' | 'archived';
}

// Unified data loader response type
export interface DataResponse<T> {
  data: T | null;
  isLoading: boolean;
  error: Error | null;
  lastUpdated: Date;
}
