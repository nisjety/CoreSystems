import {
  BarChart3,
  BookOpen,
  Bot,
  ChevronDown,
  FlaskConical,
  History,
  Rocket,
  Settings,
  Sparkles,
  Workflow,
  Zap,
  type LucideIcon,
} from 'lucide-react';
import type { SharedNavItem } from '@/components/core/sidebar/config/nav-items';
import { getAgentById } from './data';

type AgentSidebarContext = {
  agentId: string;
  agentName: string;
  section: SharedNavItem;
  footerCta: {
    label: string;
    href: string;
    icon: LucideIcon;
  };
};

const AGENT_ROUTE_PATTERN = /^\/agents\/([^/]+)(?:\/(.*))?$/;

export function getAgentSidebarContext(pathname: string): AgentSidebarContext | null {
  const match = AGENT_ROUTE_PATTERN.exec(pathname);

  if (!match) {
    return null;
  }

  const agentId = match[1];
  const agent = getAgentById(agentId);

  if (!agent) {
    return null;
  }

  return {
    agentId,
    agentName: agent.name,
    footerCta: {
      label: `${agent.name} Studio`,
      href: `/agents/${agentId}`,
      icon: Sparkles,
    },
    section: {
      id: 'agent-detail',
      labelKey: 'agentDetail',
      defaultLabel: agent.name,
      href: `/agents/${agentId}`,
      icon: Bot,
      aliases: [`/agents/${agentId}`],
      panelGroups: [
        {
          id: 'agent-detail-get-started',
          labelKey: 'agentStarter',
          defaultLabel: 'Start',
          showHeader: false,
          items: [
            {
              id: 'agent-detail-playground',
              labelKey: 'playground',
              defaultLabel: 'Playground',
              href: `/agents/${agentId}`,
              aliases: [`/agents/${agentId}/playground`],
              icon: Sparkles,
              description: 'Configure the live workspace for this agent.',
            },
          ],
        },
        {
          id: 'agent-detail-core',
          labelKey: 'agentCore',
          defaultLabel: 'Core',
          showHeader: false,
          items: [
            {
              id: 'agent-detail-train',
              labelKey: 'train',
              defaultLabel: 'Train',
              href: `/agents/${agentId}/train`,
              icon: BookOpen,
              subItems: [
                { id: `${agentId}-train-content`, labelKey: 'content', defaultLabel: 'Content', href: `/agents/${agentId}/train/content` },
                { id: `${agentId}-train-guidance`, labelKey: 'guidance', defaultLabel: 'Guidance', href: `/agents/${agentId}/train/guidance` },
                { id: `${agentId}-train-attributes`, labelKey: 'attributes', defaultLabel: 'Attributes', href: `/agents/${agentId}/train/attributes` },
                { id: `${agentId}-train-escalation`, labelKey: 'escalation', defaultLabel: 'Escalation', href: `/agents/${agentId}/train/escalation` },
                { id: `${agentId}-train-procedures`, labelKey: 'procedures', defaultLabel: 'Procedures', href: `/agents/${agentId}/train/procedures` },
              ],
            },
            {
              id: 'agent-detail-test',
              labelKey: 'test',
              defaultLabel: 'Test',
              href: `/agents/${agentId}/test`,
              icon: FlaskConical,
            },
            {
              id: 'agent-detail-deploy',
              labelKey: 'deploy',
              defaultLabel: 'Deploy',
              href: `/agents/${agentId}/deploy`,
              icon: Rocket,
              subItems: [
                { id: `${agentId}-deploy-chat`, labelKey: 'chat', defaultLabel: 'Chat', href: `/agents/${agentId}/deploy/chat`, badge: 'live' },
                { id: `${agentId}-deploy-email`, labelKey: 'email', defaultLabel: 'Email', href: `/agents/${agentId}/deploy/email`, badge: 'live' },
                { id: `${agentId}-deploy-phone`, labelKey: 'phone', defaultLabel: 'Phone', href: `/agents/${agentId}/deploy/phone`, badge: 'live' },
              ],
            },
            {
              id: 'agent-detail-analyze',
              labelKey: 'analyze',
              defaultLabel: 'Analyze',
              href: `/agents/${agentId}/analyze`,
              icon: BarChart3,
              subItems: [
                { id: `${agentId}-analyze-performance`, labelKey: 'performance', defaultLabel: 'Performance', href: `/agents/${agentId}/analyze/performance` },
                { id: `${agentId}-analyze-recommendations`, labelKey: 'recommendations', defaultLabel: 'Recommendations', href: `/agents/${agentId}/analyze/recommendations` },
                { id: `${agentId}-analyze-topics`, labelKey: 'topicsExplorer', defaultLabel: 'Topics Explorer', href: `/agents/${agentId}/analyze/topics` },
                { id: `${agentId}-analyze-trends`, labelKey: 'trends', defaultLabel: 'Trends', href: `/agents/${agentId}/analyze/trends` },
                { id: `${agentId}-analyze-monitors`, labelKey: 'monitors', defaultLabel: 'Monitors', href: `/agents/${agentId}/analyze/monitors` },
              ],
            },
            {
              id: 'agent-detail-changelog',
              labelKey: 'changelog',
              defaultLabel: 'Changelog',
              href: `/agents/${agentId}/changelog`,
              icon: History,
            },
          ],
        },
        {
          id: 'agent-detail-settings',
          labelKey: 'agentSettingsGroup',
          defaultLabel: 'Settings',
          showHeader: false,
          alignBottom: true,
          items: [
            {
              id: 'agent-detail-settings-link',
              labelKey: 'finSettings',
              defaultLabel: 'Agent settings',
              href: `/agents/${agentId}/settings`,
              icon: Settings,
              subItems: [
                { id: `${agentId}-settings-general`, labelKey: 'settingsGeneral', defaultLabel: 'General', href: `/agents/${agentId}/settings/general` },
                { id: `${agentId}-settings-audiences`, labelKey: 'audiences', defaultLabel: 'Audiences', href: `/agents/${agentId}/settings/audiences` },
              ],
            },
            {
              id: 'agent-detail-workflows',
              labelKey: 'workflows',
              defaultLabel: 'Workflows',
              href: `/agents/${agentId}/workflows`,
              icon: Workflow,
            },
            {
              id: 'agent-detail-automations',
              labelKey: 'simpleAutomations',
              defaultLabel: 'Simple automations',
              href: `/agents/${agentId}/automations`,
              icon: Zap,
            },
          ],
        },
      ],
    },
  };
}

export const AgentSidebarChevronIcon = ChevronDown;
