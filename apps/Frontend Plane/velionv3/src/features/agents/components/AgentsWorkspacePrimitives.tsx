/* eslint-disable solid/prefer-for, solid/reactivity */
import {
  CheckCircle2,
  ChevronRight,
  PanelRight,
  Zap,
} from 'lucide-solid'
import type { JSX } from 'solid-js'
import { Button } from '@/shared/ui/Button'
import { cn } from '@/shared/lib/cn'
import type {
  AgentBlueprint,
  RoleFeature,
  RoleMetric,
  RoleOperatingModel,
  StageCard,
  StageSystem,
} from '@/features/agents/lib/velion-agent-page-types'
import {
  controlFocusClass,
  roleEyebrowClass,
  roleInsetClass,
  rolePanelClass,
} from '@/features/agents/lib/velion-agent-page-styles'

export function AgentMetricStrip(props: { metrics: RoleMetric[]; role: AgentBlueprint }) {
  return (
    <div class="grid gap-3 sm:grid-cols-3" aria-label="Agent readiness metrics">
      {props.metrics.map((metric) => (
        <div class={cn('velion-agent-panel', rolePanelClass(props.role))}>
          <p class={cn('velion-agent-eyebrow', roleEyebrowClass(props.role))}>{metric.label}</p>
          <p class="mt-2 text-[28px] font-semibold leading-none tracking-[-0.02em] text-[#202126] dark:text-white">{metric.value}</p>
          <p class="velion-agent-body mt-2">{metric.detail}</p>
        </div>
      ))}
    </div>
  )
}

export function CounterpartPanel(props: {
  children: JSX.Element
  class?: string
  description: string
  eyebrow: string
  icon: AgentBlueprint['Icon']
  role: AgentBlueprint
  title: string
}) {
  const Icon = props.icon

  return (
    <div class={cn('velion-agent-panel', rolePanelClass(props.role), props.class)}>
      <div class="mb-3 flex items-start justify-between gap-3">
        <div class="min-w-0">
          <p class={cn('velion-agent-eyebrow', roleEyebrowClass(props.role))}>{props.eyebrow}</p>
          <h2 class="velion-agent-title mt-1">{props.title}</h2>
          <p class="velion-agent-body mt-1 max-w-[640px]">{props.description}</p>
        </div>
        <span class={cn('grid size-8 shrink-0 place-items-center rounded-[8px] text-white', props.role.accentClass)}>
          <Icon class="size-4" />
        </span>
      </div>
      {props.children}
    </div>
  )
}

export function StatusRow(props: { label: string; role: AgentBlueprint; value: string }) {
  return (
    <div class={cn('velion-agent-inset-row mt-2 justify-between first:mt-0', roleInsetClass(props.role))}>
      <div class="flex min-w-0 items-center gap-2 text-[11px] font-semibold text-[#4B515C] dark:text-[#DCE2EC]">
        <span class={cn('size-1.5 shrink-0 rounded-full', props.role.accentClass)} />
        <span>{props.label}</span>
      </div>
      <span class={cn('velion-agent-chip shrink-0 py-0.5', props.role.ringClass, props.role.iconClass)}>{props.value}</span>
    </div>
  )
}

export function AgentFeatureBoard(props: {
  operatingModel: RoleOperatingModel
  role: AgentBlueprint
}) {
  const sections = [
    { title: 'Knowledge', description: 'What the agent can trust.', items: props.operatingModel.knowledge },
    { title: 'Actions', description: 'What the agent can safely do.', items: props.operatingModel.actions },
    { title: 'Channels', description: 'Where the agent can operate.', items: props.operatingModel.channels },
    { title: 'Guardrails', description: 'How the agent avoids risky behavior.', items: props.operatingModel.guardrails },
  ]

  return (
    <section class="grid gap-3 lg:grid-cols-2" aria-label={`${props.role.shortTitle} configured capabilities`}>
      {sections.map((section) => (
        <div class={cn('velion-agent-panel', rolePanelClass(props.role))}>
          <div class="mb-3 flex items-end justify-between gap-3">
            <div>
              <h2 class="text-[14px] font-semibold text-[#202126] dark:text-white">{section.title}</h2>
              <p class="mt-1 text-[11px] text-[#7A808B] dark:text-[#AEB4C0]">{section.description}</p>
            </div>
            <span class={cn('size-2 rounded-full', props.role.accentClass)} />
          </div>
          <div class="space-y-2">
            {section.items.map((item) => (
              <FeatureRow feature={item} role={props.role} />
            ))}
          </div>
        </div>
      ))}
    </section>
  )
}

export function FeatureRow(props: { feature: RoleFeature; role: AgentBlueprint }) {
  const Icon = props.feature.icon

  return (
    <div class={cn('velion-agent-inset flex gap-3', roleInsetClass(props.role))}>
      <span class="grid size-8 shrink-0 place-items-center rounded-[7px] bg-white text-[#3F444D] shadow-sm dark:bg-[#1B1D22] dark:text-[#DCE2EC]">
        <Icon class="size-4" strokeWidth={2} />
      </span>
      <div class="min-w-0 flex-1">
        <div class="flex items-start justify-between gap-2">
          <h3 class="text-[12px] font-semibold text-[#202126] dark:text-white">{props.feature.title}</h3>
          <span class={cn('velion-agent-chip shrink-0 py-0.5', props.role.ringClass, props.role.iconClass)}>
            {props.feature.status}
          </span>
        </div>
        <p class="mt-1 text-[11px] leading-4 text-[#69707B] dark:text-[#B8BFCA]">{props.feature.description}</p>
      </div>
    </div>
  )
}

export function RoleConversationPreview(props: {
  operatingModel: RoleOperatingModel
  role: AgentBlueprint
}) {
  return (
    <div class={cn('velion-agent-inset mt-3', roleInsetClass(props.role))}>
      <div class="ml-auto max-w-[84%] rounded-[14px] bg-[#111111] px-3 py-2 text-[12px] leading-5 text-white dark:bg-white dark:text-[#111111]">
        {props.operatingModel.conversation.customer}
      </div>
      <div class="mt-2 max-w-[90%] rounded-[14px] bg-white px-3 py-2 text-[12px] leading-5 text-[#2F343C] shadow-sm dark:bg-[#1B1D22] dark:text-[#E7EBF1]">
        {props.operatingModel.conversation.agent}
      </div>
      <div class="mt-3 rounded-[7px] border border-dashed border-[#DDE0E5] px-3 py-2 text-[11px] leading-4 text-[#656C78] dark:border-[#343842] dark:text-[#B7BEC9]">
        {props.operatingModel.conversation.note}
      </div>
      <div class="mt-3 flex flex-wrap gap-1.5">
        {props.operatingModel.conversation.quickReplies.map((reply) => (
          <button
            type="button"
            class={cn(
              'rounded-full border border-[#E2E3E8] bg-white px-2.5 py-1 text-[10px] font-semibold text-[#3C414A] transition hover:bg-[#F2F3F5] dark:border-[#2B2D33] dark:bg-[#17181C] dark:text-[#DCE2EC]',
              props.role.ringClass,
              controlFocusClass,
            )}
          >
            {reply}
          </button>
        ))}
      </div>
    </div>
  )
}

export function StageReadinessPanel(props: { role: AgentBlueprint; system: StageSystem }) {
  return (
    <div class={cn('velion-agent-panel velion-agent-panel-strong', rolePanelClass(props.role))}>
      <div class="flex flex-wrap items-start justify-between gap-3">
        <div>
          <p class={cn('velion-agent-eyebrow', roleEyebrowClass(props.role))}>Operational checklist</p>
          <h2 class="velion-agent-title mt-1">{props.role.shortTitle} readiness</h2>
        </div>
        <Button
          variant="primary"
          size="md"
          shape="pill"
          class={cn('min-h-8 px-3 text-[12px] font-semibold', controlFocusClass)}
        >
          <Zap class="size-3.5" />
          {props.system.primaryAction}
        </Button>
      </div>
      <div class="mt-4 grid gap-2">
        {props.system.checklist.map((item) => (
          <div class={cn('velion-agent-inset-row', roleInsetClass(props.role))}>
            <CheckCircle2 class={cn('size-4 shrink-0', props.role.iconClass)} strokeWidth={2.1} />
            <span class="text-[12px] font-medium text-[#333740] dark:text-[#E6EAF0]">{item}</span>
          </div>
        ))}
      </div>
      <Button
        variant="secondary"
        size="md"
        shape="pill"
        class={cn('mt-3 min-h-8 w-full px-3 text-[12px] font-semibold', controlFocusClass)}
      >
        <PanelRight class="size-3.5" />
        {props.system.secondaryAction}
      </Button>
    </div>
  )
}

export function StageSystemCard(props: { card: StageCard; role: AgentBlueprint }) {
  const Icon = props.card.icon

  return (
    <div class={cn('velion-agent-panel velion-agent-panel-strong', rolePanelClass(props.role))}>
      <span class={cn('grid size-9 place-items-center rounded-[8px] text-white', props.role.accentClass)}>
        <Icon class="size-4" strokeWidth={2.1} />
      </span>
      <h2 class="velion-agent-title mt-3">{props.card.title}</h2>
      <p class="velion-agent-body mt-1 min-h-10">{props.card.description}</p>
      <div class="mt-3 space-y-1.5 border-t border-dashed border-[#E3E4E8] pt-3 dark:border-[#2D3037]">
        {props.card.items.map((item) => (
          <div class="flex items-start gap-2 text-[11px] leading-4 text-[#3F444D] dark:text-[#D7DCE4]">
            <span class={cn('mt-1 size-1.5 shrink-0 rounded-full', props.role.accentClass)} />
            <span>{item}</span>
          </div>
        ))}
      </div>
    </div>
  )
}

export function RoleCard(props: {
  active: boolean
  onSelect: () => void
  role: AgentBlueprint
}) {
  const Icon = props.role.Icon
  const Visual = props.role.Visual

  return (
    <button
      type="button"
      aria-label={props.role.title}
      aria-pressed={props.active}
      onClick={props.onSelect}
      class={cn(
        'agent-role-card',
        `agent-role-card--${props.role.id}`,
        props.active && 'agent-role-card--active',
        controlFocusClass,
      )}
    >
      <Visual />

      <div class="agent-role-card__body">
        <div class="agent-role-card__heading">
          <span class="agent-role-card__icon">
            <Icon class="agent-role-card__icon-svg" strokeWidth={2.1} />
          </span>
          <div class="agent-role-card__title-group">
            <h2 class="agent-role-card__title">{props.role.title}</h2>
            <p class="agent-role-card__eyebrow">{props.role.eyebrow}.</p>
          </div>
        </div>

        <p class="agent-role-card__description">{props.role.description}</p>

        <div class="agent-role-card__capabilities">
          {props.role.capabilities.map((capability) => (
            <div class="agent-role-card__capability">
              <span class="agent-role-card__capability-dot" />
              <span>{capability}</span>
            </div>
          ))}
        </div>

        <span class="agent-role-card__cta">
          {props.role.cta}
          <ChevronRight class="agent-role-card__cta-icon" strokeWidth={2.2} />
        </span>
      </div>
    </button>
  )
}
