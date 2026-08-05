import type { LucideProps } from 'lucide-solid'
import type { Component } from 'solid-js'
import type {
  AgentFeatureId,
  AgentRoleId,
  AgentStageId,
} from '@/features/agents/lib/agent-roles'

export type AgentVisualProps = Record<string, never>

export type AgentIcon = Component<LucideProps>

export type StageCard = {
  title: string
  description: string
  items: string[]
  icon: AgentIcon
}

export type StageSystem = {
  eyebrow: string
  title: string
  description: string
  cards: StageCard[]
  checklist: string[]
  primaryAction: string
  secondaryAction: string
  previewTitle: string
  previewDescription: string
}

export type RoleFeature = {
  title: string
  description: string
  status: string
  icon: AgentIcon
}

export type RoleMetric = {
  label: string
  value: string
  detail: string
}

export type RoleConversation = {
  customer: string
  agent: string
  note: string
  quickReplies: string[]
}

export type RoleOperatingModel = {
  activationLabel: string
  activationStatus: string
  activationSummary: string
  model: string
  confidence: string
  automationTarget: string
  metrics: RoleMetric[]
  knowledge: RoleFeature[]
  actions: RoleFeature[]
  channels: RoleFeature[]
  guardrails: RoleFeature[]
  conversation: RoleConversation
}

export type AgentBlueprint = {
  id: AgentRoleId
  title: string
  shortTitle: string
  eyebrow: string
  description: string
  detailIntro: string
  cta: string
  accentClass: string
  iconClass: string
  ringClass: string
  Icon: AgentIcon
  Visual: Component<AgentVisualProps>
  capabilities: string[]
  system: Record<AgentStageId, StageSystem>
  featureWorkspaces?: Partial<Record<AgentFeatureId, StageSystem>>
  operatingModel?: RoleOperatingModel
}
