import {
  createMemo,
  For,
  Match,
  Show,
  Switch,
  type JSX,
} from 'solid-js'
import {
  ChevronRight,
  Gauge,
  Play,
  Puzzle,
  Rocket,
  Sparkles,
} from 'lucide-solid'
import { A } from '@solidjs/router'
import { Button } from '@/shared/ui/Button'
import { VerevonIconButton } from '@/shared/ui/verevon/VerevonIconButton'
import { cn } from '@/shared/lib/cn'
import { RoleCounterpartSurface } from '@/features/agents/components/AgentRoleSurfaces'
import { ChatbotStudio } from '@/features/agents/components/ChatbotStudio'
import {
  AgentMetricStrip,
  RoleCard,
  RoleConversationPreview,
  StageReadinessPanel,
  StageSystemCard,
} from '@/features/agents/components/AgentsWorkspacePrimitives'
import { blueprintBadgeTitle, DesignPreviewBadge } from '@/features/agents/components/DesignPreviewBadge'
import { WorkflowBuilder } from '@/features/agents/components/WorkflowBuilder'
import { agentBlueprints } from '@/features/agents/lib/verevon-agent-blueprints'
import {
  agentFeatureOptionsByRole,
  type AgentFeatureId,
  type AgentRoleId,
} from '@/features/agents/lib/agent-roles'
import {
  controlFocusClass,
  roleEyebrowClass,
  rolePanelClass,
} from '@/features/agents/lib/verevon-agent-page-styles'
import type { AgentBlueprint } from '@/features/agents/lib/verevon-agent-page-types'
import { useAgentFeature, useAgentSelection } from '@/features/agents/lib/use-agent-selection'
import { useI18n } from '@/shared/i18n'

export default function AgentsPage() {
  const [agentSelection, setAgentSelection] = useAgentSelection()
  const [agentFeature, setAgentFeature] = useAgentFeature(agentSelection)
  const activeRoleId = () => (agentSelection() === 'all' ? null : agentSelection())
  const activeRole = () => agentBlueprints.find((role) => role.id === activeRoleId()) ?? null

  return (
    <Switch
      fallback={(
        <AgentsPageFrame>
          <AllRolesOverview onRoleSelect={setAgentSelection} />
        </AgentsPageFrame>
      )}
    >
      <Match when={activeRole()?.id === 'workflow'}>
        <WorkflowBuilder />
      </Match>
      <Match when={activeRole()?.id === 'chatbot'}>
        <ChatbotStudio />
      </Match>
      <Match when={activeRole()}>
        <AgentsPageFrame>
          <Show when={activeRole()}>
            {(role) => (
              <SelectedAgentWorkspace
                feature={agentFeature()}
                onFeatureSelect={setAgentFeature}
                role={role()}
              />
            )}
          </Show>
        </AgentsPageFrame>
      </Match>
    </Switch>
  )
}

function AgentsPageFrame(props: { children: JSX.Element }) {
  return (
    <div class="agents-overview-page">
      <div class="agents-overview-container">
        {props.children}
      </div>
    </div>
  )
}

function AllRolesOverview(props: { onRoleSelect: (role: AgentRoleId) => void }) {
  const i18n = useI18n()
  return (
    <>
      <header class="agents-overview-header">
        <div class="agents-overview-kicker">
          <Sparkles class="agents-overview-kicker__icon" strokeWidth={2} />
        </div>
        <h1
          id="agent-role-heading"
          class="agents-overview-title"
        >
          {i18n.tr('Ett agentsystem for hele kundereisen', 'One agent system for the entire customer journey')}
        </h1>
        <p class="agents-overview-description">
          {i18n.tr(
            'Hver rolle under er et Verevon-blueprint — en referansemodell for drift, ennå ikke konfigurert for denne organisasjonen. Velg én for å se hvordan kunnskap, tester, kanaler og innsiktsløkker henger sammen.',
            'Each role below is a Verevon blueprint — a reference operating model, not yet configured for this org. Choose one to review how its knowledge, tests, channels, and insight loops fit together.',
          )}
        </p>
      </header>

      <section aria-labelledby="agent-role-heading" class="agents-overview-section">
        <TaskConsoleEntry />
        <InstallationsEntry />
        <div class="agents-role-grid">
          <For each={agentBlueprints}>
            {(role) => (
              <RoleCard
                active={false}
                role={role}
                onSelect={() => props.onRoleSelect(role.id)}
              />
            )}
          </For>
        </div>
      </section>
    </>
  )
}

function TaskConsoleEntry() {
  const i18n = useI18n()
  return (
    <A href="/agents/runs" class={cn('agents-task-console-entry', controlFocusClass)} aria-label={i18n.tr('Åpne Agent Run Console for å kjøre og godkjenne agentoppgaver', 'Open the Agent Run Console to run and approve agent tasks')}>
      <span class="agents-task-console-entry__icon">
        <Gauge class="size-5" strokeWidth={2.1} />
      </span>
      <span class="agents-task-console-entry__copy">
        <span class="agents-task-console-entry__title">{i18n.tr('Oppgavekonsoll — kjør og godkjenn agentoppgaver', 'Task Console — run & approve agent tasks')}</span>
        <span class="agents-task-console-entry__desc">
          {i18n.tr(
            'Start en selvstendig kjøring, følg planen, verktøyene og nettleser-stegene live, og godkjenn risikable handlinger før de skjer.',
            'Launch an autonomous run, watch the live plan, tools, and browser steps, and approve risky actions before they happen.',
          )}
        </span>
      </span>
      <span class="agents-task-console-entry__cta">
        {i18n.tr('Åpne konsoll', 'Open console')}
        <ChevronRight class="size-4" strokeWidth={2.2} />
      </span>
    </A>
  )
}

function InstallationsEntry() {
  const i18n = useI18n()
  return (
    <A href="/agents/installations" class={cn('agents-task-console-entry', controlFocusClass)} aria-label={i18n.tr('Åpne installasjoner for å se agentene dine og hvilke rom de er i', 'Open installations to see your agents and which rooms they are in')}>
      <span class="agents-task-console-entry__icon">
        <Puzzle class="size-5" strokeWidth={2.1} />
      </span>
      <span class="agents-task-console-entry__copy">
        <span class="agents-task-console-entry__title">{i18n.tr('Installasjoner — agenter og hvilke rom de er i', 'Installations — your agents and which rooms they\'re in')}</span>
        <span class="agents-task-console-entry__desc">
          {i18n.tr(
            'Agentene du har opprettet eller lagt til, samlet på tvers av rommene dine — med samme status som hvert roms Agent-fane viser.',
            'The agents you\'ve created or added, gathered across your rooms — with the same status each room\'s Agent tab shows.',
          )}
        </span>
      </span>
      <span class="agents-task-console-entry__cta">
        {i18n.tr('Åpne installasjoner', 'Open installations')}
        <ChevronRight class="size-4" strokeWidth={2.2} />
      </span>
    </A>
  )
}

function SelectedAgentWorkspace(props: {
  feature: AgentFeatureId
  onFeatureSelect: (feature: AgentFeatureId) => void
  role: AgentBlueprint
}) {
  const i18n = useI18n()
  const workspace = createMemo(() => {
    const role = props.role
    const operatingModel = role.operatingModel

    if (!operatingModel || (role.id !== 'service' && role.id !== 'sales' && role.id !== 'ecommerce')) {
      return null
    }

    const featureOptions = agentFeatureOptionsByRole[role.id]
    const activeFeature = featureOptions.some((option) => option.id === props.feature)
      ? props.feature
      : featureOptions[0]!.id
    const system = role.featureWorkspaces?.[activeFeature] ?? role.system.train

    return {
      activeFeature,
      featureOptions,
      Icon: role.Icon,
      operatingModel,
      role,
      system,
      Visual: role.Visual,
    }
  })

  return (
    <Show when={workspace()}>
      {(view) => {
        const Icon = view().Icon
        const Visual = view().Visual

        return (
          <div class="pb-8">
            <header class="grid gap-4 lg:grid-cols-[minmax(0,1fr)_390px] lg:items-start">
              <div class="min-w-0">
                <div class={cn('inline-flex items-center gap-2 rounded-full px-3 py-1 text-[12px] font-semibold', view().role.ringClass, view().role.iconClass)}>
                  <Icon class="size-3.5" strokeWidth={2.1} />
                  {view().role.shortTitle}
                </div>
                <h1 class="mt-3 max-w-[780px] text-[30px] font-semibold leading-[1.04] tracking-[-0.02em] text-[#1C1C1E] dark:text-white sm:text-[38px]">
                  {view().system.title}
                </h1>
                <p class="mt-3 max-w-[760px] text-[14px] leading-6 text-[#636873] dark:text-[#AEB4C0]">{view().system.description}</p>
                <div class="mt-4 flex flex-wrap gap-2">
                  <For each={[view().operatingModel.model, view().operatingModel.confidence, view().operatingModel.automationTarget]}>
                    {(item) => (
                      <span class="rounded-full border border-[#E3E4E8] bg-white px-3 py-1.5 text-[11px] font-medium text-[#4D535D] shadow-sm dark:border-[#2B2D33] dark:bg-[#17181C] dark:text-[#C8CED8]">
                        {item}
                      </span>
                    )}
                  </For>
                </div>
              </div>

              <div class={cn('rounded-[8px] border p-4 shadow-[0_14px_34px_rgba(20,21,24,0.055)]', rolePanelClass(view().role))}>
                <div class="flex items-center justify-between gap-3">
                  <div>
                    <p class={cn('text-[11px] font-semibold uppercase', roleEyebrowClass(view().role))}>{i18n.tr('Aktivering', 'Activation')}</p>
                    <p class="mt-1 text-[15px] font-semibold text-[#202126] dark:text-white">{view().operatingModel.activationStatus}</p>
                  </div>
                  <span class={cn('grid size-9 place-items-center rounded-[8px] text-white', view().role.accentClass)}>
                    <Icon class="size-4" strokeWidth={2.1} />
                  </span>
                </div>
                <p class="mt-3 text-[12px] leading-5 text-[#626873] dark:text-[#AEB4C0]">{view().operatingModel.activationSummary}</p>
                {/* Phase 3 PR-1 (honesty sweep) + Phase 4 PR-3 (A5): agent
                    activation/readiness has no deploy or per-org config backend yet
                    (the real per-org agent-config store is deferred to Phase 5).
                    Labelled "Blueprint / not yet configured for this org" and the
                    activate/readiness controls are disabled so nothing implies a
                    configured or deployable agent. No Active/Private/Live badge. */}
                <DesignPreviewBadge class="mt-3" label={i18n.tr('Blueprint', 'Blueprint')} title={blueprintBadgeTitle(i18n.tr)} />
                <p class="mt-2 text-[11px] font-medium leading-4 text-[#8A909B] dark:text-[#7C828C]">{i18n.tr('Ikke konfigurert for denne organisasjonen ennå', 'Not yet configured for this org')}</p>
                <div class="mt-4 flex gap-2">
                  <Button variant="primary" size="md" shape="pill" disabled class={cn('min-h-8 flex-1 px-4 text-[12px] font-semibold', controlFocusClass)}>
                    <Rocket class="size-3.5" />
                    {view().operatingModel.activationLabel}
                  </Button>
                  <VerevonIconButton
                    type="button"
                    size="md"
                    shape="circle"
                    disabled
                    aria-label={i18n.tr(`Kjør beredskapstest for ${view().role.shortTitle}`, `Run ${view().role.shortTitle} readiness test`)}
                    class={cn('border border-[#E2E3E8] bg-white dark:border-[#2B2D33] dark:bg-[#17181C]', controlFocusClass)}
                  >
                    <Play class="size-3.5" />
                  </VerevonIconButton>
                </div>
                <div class="mt-4 grid gap-1 sm:grid-cols-2" aria-label={i18n.tr(`${view().role.shortTitle}-funksjonsområder`, `${view().role.shortTitle} feature areas`)}>
                  <For each={view().featureOptions}>
                    {(option) => (
                      <button
                        type="button"
                        aria-label={i18n.tr(`Åpne arbeidsområdet ${option.label}`, `Open ${option.label} workspace`)}
                        aria-pressed={option.id === view().activeFeature}
                        onClick={() => props.onFeatureSelect(option.id)}
                        class={cn(
                          'min-h-8 rounded-[7px] px-2 py-1 text-[11px] font-semibold leading-4 transition-colors',
                          option.id === view().activeFeature
                            ? 'bg-[#111111] text-white dark:bg-white dark:text-[#111111]'
                            : 'bg-[#F2F3F5] text-[#68707D] hover:bg-[#EAECF0] dark:bg-[#202228] dark:text-[#C0C6D0]',
                          controlFocusClass,
                        )}
                      >
                        {option.label}
                      </button>
                    )}
                  </For>
                </div>
              </div>
            </header>

            <section class="mt-5 grid gap-4 xl:grid-cols-[minmax(0,1fr)_400px]">
              <div class="space-y-4">
                <AgentMetricStrip metrics={view().operatingModel.metrics} role={view().role} />

                <RoleCounterpartSurface feature={view().activeFeature} operatingModel={view().operatingModel} role={view().role} />

                <div class="grid gap-3 md:grid-cols-3" aria-label={`${view().role.shortTitle} ${view().system.eyebrow.toLowerCase()} system`}>
                  <For each={view().system.cards}>
                    {(card) => <StageSystemCard card={card} role={view().role} />}
                  </For>
                </div>
              </div>

              <aside class="space-y-4">
                <div class={cn('rounded-[8px] border p-3 shadow-[0_18px_48px_rgba(20,21,24,0.055)]', rolePanelClass(view().role))}>
                  <div class="mb-3 flex items-center justify-between gap-3">
                    <div>
                      <p class={cn('text-[11px] font-semibold uppercase', roleEyebrowClass(view().role))}>{i18n.tr('Forhåndsvisning', 'Preview')}</p>
                      <h2 class="mt-1 text-[16px] font-semibold text-[#202126] dark:text-white">{view().system.previewTitle}</h2>
                    </div>
                    <span class={cn('grid size-8 place-items-center rounded-[8px] text-white', view().role.accentClass)}>
                      <Sparkles class="size-4" />
                    </span>
                  </div>
                  <Visual />
                  <RoleConversationPreview operatingModel={view().operatingModel} role={view().role} />
                  <p class="mt-3 text-[12px] leading-5 text-[#636873] dark:text-[#AEB4C0]">{view().system.previewDescription}</p>
                </div>

                <StageReadinessPanel role={view().role} system={view().system} />
              </aside>
            </section>
          </div>
        )
      }}
    </Show>
  )
}
