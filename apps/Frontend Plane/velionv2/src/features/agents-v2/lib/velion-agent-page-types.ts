import type { ReactNode } from "react";
import type { LucideIcon } from "lucide-react";
import type {
  AgentFeatureId,
  AgentRoleId,
  AgentStageId,
} from "@/features/agents-v2/lib/agent-roles";
import type { AgentVisualProps } from "@/features/agents-v2/components/VerevonAgentsVisuals";

export type StageCard = {
  title: string;
  description: string;
  items: string[];
  icon: LucideIcon;
};

export type StageSystem = {
  eyebrow: string;
  title: string;
  description: string;
  cards: StageCard[];
  checklist: string[];
  primaryAction: string;
  secondaryAction: string;
  previewTitle: string;
  previewDescription: string;
};

export type RoleFeature = {
  title: string;
  description: string;
  status: string;
  icon: LucideIcon;
};

export type RoleMetric = {
  label: string;
  value: string;
  detail: string;
};

export type RoleConversation = {
  customer: string;
  agent: string;
  note: string;
  quickReplies: string[];
};

export type RoleOperatingModel = {
  activationLabel: string;
  activationStatus: string;
  activationSummary: string;
  model: string;
  confidence: string;
  automationTarget: string;
  metrics: RoleMetric[];
  knowledge: RoleFeature[];
  actions: RoleFeature[];
  channels: RoleFeature[];
  guardrails: RoleFeature[];
  conversation: RoleConversation;
};

export type AgentBlueprint = {
  id: AgentRoleId;
  title: string;
  shortTitle: string;
  eyebrow: string;
  description: string;
  detailIntro: string;
  cta: string;
  accentClass: string;
  iconClass: string;
  ringClass: string;
  Icon: LucideIcon;
  Visual: (props: AgentVisualProps) => ReactNode;
  capabilities: string[];
  system: Record<AgentStageId, StageSystem>;
  featureWorkspaces?: Partial<Record<AgentFeatureId, StageSystem>>;
  operatingModel?: RoleOperatingModel;
};
