import type { LucideIcon } from 'lucide-react';
import {
  BarChart3,
  BookOpen,
  Bot,
  BriefcaseBusiness,
  FlaskConical,
  Headset,
  Rocket,
  Settings2,
  ShoppingCart,
  Sparkles,
  WandSparkles,
  Workflow,
  Zap,
} from 'lucide-react';
import type { Agent, AgentUseCase } from './types';

export interface AgentRoleCard {
  id: string;
  name: string;
  strapline: string;
  description: string;
  bullets: string[];
  ctaLabel: string;
  status?: 'live' | 'coming-soon';
  icon: LucideIcon;
  image: string;
  accentClassName: string;
}

export type AgentWorkspaceViewId =
  | 'playground'
  | 'train'
  | 'test'
  | 'deploy'
  | 'analyze'
  | 'changelog'
  | 'settings'
  | 'workflows'
  | 'automations';

export interface AgentWorkspaceView {
  id: AgentWorkspaceViewId;
  label: string;
  shortLabel: string;
  eyebrow: string;
  title: string;
  description: string;
}

export type AgentWithMeta = Agent & {
  roleName: string;
  previewImage: string;
  gradient: string;
  badgeLabel: string;
  goal: string;
  greeting: string;
  knowledgeSummary: string;
  instructionSummary: string;
  launchMetric: string;
};

const createAgent = (
  partial: Omit<AgentWithMeta, 'createdAt'> & { createdDaysAgo: number },
): AgentWithMeta => ({
  ...partial,
  createdAt: new Date(Date.now() - partial.createdDaysAgo * 24 * 60 * 60 * 1000).toISOString(),
});

export const AGENT_ROLE_CARDS: AgentRoleCard[] = [
  {
    id: 'service',
    name: 'Service',
    strapline: 'Support your customers.',
    description:
      'Resolve customer questions with a calm, contextual support flow across chat, email, and social.',
    bullets: ['Provide instant support', 'Resolve complex queries', 'Across every channel'],
    ctaLabel: 'Get started',
    icon: Headset,
    image: '/imagens/arched-corridor-1.jpeg',
    accentClassName: 'from-[#f0c9b8] via-[#f9efe8] to-[#d7dce8]',
  },
  {
    id: 'sales',
    name: 'Sales',
    strapline: 'Acquire new customers.',
    description:
      'Guide discovery, qualify intent, and move prospects into the right next step without losing warmth.',
    bullets: ['Engage B2B prospects', 'Guide product discovery', 'Qualify and route leads'],
    ctaLabel: 'Request access',
    icon: BriefcaseBusiness,
    image: '/imagens/abstract.png',
    accentClassName: 'from-[#8dc3d6] via-[#f5f8fb] to-[#4d6a92]',
  },
  {
    id: 'ecommerce',
    name: 'Ecommerce',
    strapline: 'Engage your shoppers.',
    description:
      'Blend product knowledge, recommendations, and guided checkout prompts into a single shopping assistant.',
    bullets: ['Recommend products', 'Drive toward checkout', 'Built for catalog-heavy stores'],
    ctaLabel: 'Coming soon',
    status: 'coming-soon',
    icon: ShoppingCart,
    image: '/imagens/curved-interior-sculpture.png',
    accentClassName: 'from-[#d4f0ea] via-[#f6fbfb] to-[#e8e2d6]',
  },
  {
    id: 'success',
    name: 'Success',
    strapline: 'Retain your customers.',
    description:
      'Coordinate onboarding, activation nudges, and account recovery to improve time-to-value and retention.',
    bullets: ['Accelerate time to value', 'Drive customer retention', 'Expand account value'],
    ctaLabel: 'Coming soon',
    status: 'coming-soon',
    icon: WandSparkles,
    image: '/imagens/arched-hallway-symmetry.jpeg',
    accentClassName: 'from-[#cdc2de] via-[#f8f3fa] to-[#e6d6ce]',
  },
];

export const AGENT_WORKSPACE_VIEWS: AgentWorkspaceView[] = [
  {
    id: 'playground',
    label: 'Playground',
    shortLabel: 'Playground',
    eyebrow: 'Playground',
    title: 'Shape the live experience before you publish it',
    description:
      'Tune instructions, model behavior, and automation readiness while seeing the agent preview update in context.',
  },
  {
    id: 'train',
    label: 'Train',
    shortLabel: 'Train',
    eyebrow: 'Training',
    title: 'Feed the agent the patterns it should recognize',
    description:
      'Curate examples, upload source material, and tighten retrieval so responses feel grounded and brand-specific.',
  },
  {
    id: 'test',
    label: 'Test',
    shortLabel: 'Test',
    eyebrow: 'Validation',
    title: 'Pressure-test tone, edge cases, and routing',
    description:
      'Run scenario packs, compare outputs, and make sure the agent can recover gracefully before launch.',
  },
  {
    id: 'deploy',
    label: 'Deploy',
    shortLabel: 'Deploy',
    eyebrow: 'Deployment',
    title: 'Roll the agent out deliberately',
    description:
      'Choose channels, release progressively, and control how the live experience reaches customers.',
  },
  {
    id: 'analyze',
    label: 'Analyze',
    shortLabel: 'Analyze',
    eyebrow: 'Insights',
    title: 'Understand where the agent is helping and where it leaks',
    description:
      'Track resolution quality, response confidence, and the moments where human support still matters most.',
  },
  {
    id: 'changelog',
    label: 'Changelog',
    shortLabel: 'Changelog',
    eyebrow: 'Changelog',
    title: 'Keep a clean record of every iteration',
    description:
      'Review model changes, prompt edits, and automation adjustments so teams always know what shipped.',
  },
  {
    id: 'settings',
    label: 'Fin settings',
    shortLabel: 'Settings',
    eyebrow: 'Configuration',
    title: 'Set the rules the agent operates within',
    description:
      'Manage behavior defaults, fallback patterns, escalation thresholds, and workspace-wide constraints.',
  },
  {
    id: 'workflows',
    label: 'Workflows',
    shortLabel: 'Workflows',
    eyebrow: 'Flows',
    title: 'Chain actions into repeatable customer journeys',
    description:
      'Define multi-step automations that combine agent reasoning with business actions and approvals.',
  },
  {
    id: 'automations',
    label: 'Simple automations',
    shortLabel: 'Automations',
    eyebrow: 'Automation',
    title: 'Ship lighter-weight no-code helpers',
    description:
      'Use triggers and guardrailed rules for straightforward cases that should not require a full workflow.',
  },
];

export const MOCK_AGENTS: AgentWithMeta[] = [
  createAgent({
    id: 'service-oslo',
    name: 'Service',
    roleName: 'Customer support agent',
    description:
      'Handles order questions, returns, and delivery delays with grounded answers from the support knowledge layer.',
    useCase: 'customer_support',
    status: 'active',
    // U2-2/U3-2: aligned with the live capability-core model registry.
    model: 'gpt-4o-mini',
    tools: ['knowledge_search', 'escalate', 'refund_status'],
    createdDaysAgo: 31,
    previewImage: '/imagens/arched-corridor-1.jpeg',
    gradient: 'from-[#f3ded7] via-[#fbf6f2] to-[#d8ddea]',
    badgeLabel: 'Live on chat + email',
    goal: 'Support customers across every touchpoint with one consistent voice.',
    greeting: 'Hi! What can I help you with?',
    knowledgeSummary: '129 help articles, 42 policy snippets, and order-status lookup.',
    instructionSummary:
      'Warm, decisive, and concise. Always resolve first. Escalate only when account changes or refunds require a human.',
    launchMetric: '89% resolution rate',
  }),
  createAgent({
    id: 'sales-labs',
    name: 'Sales',
    roleName: 'Lead qualification agent',
    description:
      'Qualifies inbound prospects, handles top-of-funnel questions, and routes demo-ready leads into the sales queue.',
    useCase: 'sales',
    status: 'active',
    // U2-2/U3-2: aligned with the live capability-core model registry.
    model: 'gpt-5-mini',
    tools: ['knowledge_search', 'schedule_meeting', 'crm_lookup'],
    createdDaysAgo: 12,
    previewImage: '/imagens/abstract.png',
    gradient: 'from-[#d6eef7] via-[#f6fbfd] to-[#d8e1f3]',
    badgeLabel: 'Pipeline assistant',
    goal: 'Acquire new customers with sharper qualification and cleaner handoff.',
    greeting: 'Tell me about your team and what you’re trying to solve.',
    knowledgeSummary: 'Pricing library, objection handling, CRM routing rules, and demo calendar sync.',
    instructionSummary:
      'Ask one deliberate follow-up at a time, identify urgency, then route to demo, nurture, or self-serve paths.',
    launchMetric: '41% demo conversion',
  }),
  createAgent({
    id: 'faq-core',
    name: 'FAQ Bot',
    roleName: 'Always-on FAQ layer',
    description:
      'Answers simple recurring questions from the company knowledge base and keeps low-complexity conversations out of the queue.',
    useCase: 'faq',
    status: 'draft',
    // U2-2/U3-2: aligned with the live capability-core model registry.
    model: 'gpt-4o-mini',
    tools: ['knowledge_search', 'mark_resolved'],
    createdDaysAgo: 4,
    previewImage: '/imagens/curved-concrete-space.png',
    gradient: 'from-[#ece6dc] via-[#fbfaf8] to-[#e1e5ed]',
    badgeLabel: 'Draft agent',
    goal: 'Absorb repetitive questions without making the experience feel robotic.',
    greeting: 'Ask me anything about shipping, returns, or product availability.',
    knowledgeSummary: 'FAQ blocks, shipping policy, stock feed summaries, and store hours.',
    instructionSummary:
      'Stay short, cite the source category implicitly, and invite escalation if the user needs account-specific help.',
    launchMetric: 'Draft workspace',
  }),
];

export const USE_CASE_TO_ROLE: Record<AgentUseCase, string> = {
  customer_support: 'Service',
  sales: 'Sales',
  marketing: 'Growth',
  hr: 'People',
  faq: 'Support FAQs',
  onboarding: 'Success',
  other: 'Custom',
};

export const AGENT_DETAIL_ICONS = {
  playground: Sparkles,
  train: BookOpen,
  test: FlaskConical,
  deploy: Rocket,
  analyze: BarChart3,
  changelog: Bot,
  settings: Settings2,
  workflows: Workflow,
  automations: Zap,
} satisfies Record<AgentWorkspaceViewId, LucideIcon>;

/**
 * Wave 8 (ui-ux-velion-gap.md §18): `getAgentById` now serves the
 * demo fixtures ONLY when the caller is explicitly looking up one of
 * the well-known mock ids that ship in `MOCK_AGENTS` (e.g. so the
 * `/api/agents/seed` route can reuse them). For any other id, return
 * null so the page handler responds with notFound() rather than
 * silently rendering a misleading fixture.
 */
export function getAgentById(agentId: string): AgentWithMeta | null {
  return MOCK_AGENTS.find((agent) => agent.id === agentId) ?? null;
}

/** Set of mock-fixture agent ids — handy for callers that want to
 * distinguish "this is a demo id we ship in MOCK_AGENTS" from
 * "the user hit a real agent we couldn't find". */
export const MOCK_AGENT_IDS: ReadonlySet<string> = new Set(
  MOCK_AGENTS.map((a) => a.id),
);

export function getWorkspaceView(viewId?: string): AgentWorkspaceView {
  return AGENT_WORKSPACE_VIEWS.find((view) => view.id === viewId) ?? AGENT_WORKSPACE_VIEWS[0];
}
