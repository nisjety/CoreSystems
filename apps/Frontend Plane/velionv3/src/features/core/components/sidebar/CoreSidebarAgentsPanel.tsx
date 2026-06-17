import {
  BarChart3,
  BookOpen,
  Bot,
  BriefcaseBusiness,
  CalendarClock,
  CheckCircle2,
  ChartNoAxesColumnIncreasing,
  ChevronDown,
  Database,
  Globe2,
  LayoutGrid,
  MessagesSquare,
  Plug,
  Rocket,
  Search,
  Settings,
  ShieldCheck,
  ShoppingBag,
  Sparkles,
  TicketCheck,
  UsersRound,
  Wrench,
} from 'lucide-solid'
import { createMemo, createSignal, For, Show, type Component } from 'solid-js'
import type { LucideProps } from 'lucide-solid'
import { Dynamic } from 'solid-js/web'
import {
  agentFeatureOptionsByRole,
  agentRoleOptions,
  chatbotBuilderSectionOptions,
  isCoreAgentRoleId,
  type AgentFeatureId,
  type ChatbotBuilderSectionId,
  type CoreAgentRoleId,
} from '@/features/agents/lib/agent-roles'
import { WorkflowToolsPanel } from '@/features/agents/components/WorkflowToolsPanel'
import {
  useAgentFeature,
  useAgentSelection,
  useChatbotBuilderSection,
  useWorkflowBuilderTool,
} from '@/features/agents/lib/use-agent-selection'
import { SidebarPanelTitle } from '@/features/core/components/sidebar/CoreSidebarPrimitives'
import { cn } from '@/shared/lib/cn'

type AgentIcon = Component<LucideProps>

const agentFeatureIcons: Record<AgentFeatureId, AgentIcon> = {
  'service-resolution': TicketCheck,
  'service-knowledge': BookOpen,
  'service-actions': Wrench,
  'service-channels': Globe2,
  'service-quality': CheckCircle2,
  'service-insights': BarChart3,
  'sales-lead-capture': Sparkles,
  'sales-qualification': BriefcaseBusiness,
  'sales-objections': ShieldCheck,
  'sales-booking': CalendarClock,
  'sales-crm': Database,
  'sales-insights': ChartNoAxesColumnIncreasing,
  'commerce-shopping': ShoppingBag,
  'commerce-support': TicketCheck,
  'commerce-product-finder': Search,
  'commerce-cart': Rocket,
  'commerce-brand': MessagesSquare,
  'commerce-store': Settings,
  'commerce-insights': BarChart3,
}

const chatbotSidebarTabIcons: Record<ChatbotBuilderSectionId, AgentIcon> = {
  playground: LayoutGrid,
  'chat-logs': MessagesSquare,
  'data-sources': Database,
  integrations: Plug,
  actions: Wrench,
  analytics: BarChart3,
  leads: UsersRound,
  insights: ChartNoAxesColumnIncreasing,
  install: Rocket,
  settings: Settings,
}

const chatbotSidebarTabs = chatbotBuilderSectionOptions.map((option) => ({
  ...option,
  icon: chatbotSidebarTabIcons[option.id],
}))

export function AgentsExpandedSidebarPanel(props: { onCollapse: () => void }) {
  const [agentSelection, , updateAgentSelectionFromValue] = useAgentSelection()
  const [agentFeature, setAgentFeature] = useAgentFeature(agentSelection)
  const [chatbotSection, setChatbotSection] = useChatbotBuilderSection()
  const [workflowTool, setWorkflowTool] = useWorkflowBuilderTool()
  const selectedCoreAgent = createMemo<CoreAgentRoleId | null>(() => (
    isCoreAgentRoleId(agentSelection()) ? (agentSelection() as CoreAgentRoleId) : null
  ))

  return (
    <Show
      when={agentSelection() !== 'chatbot'}
      fallback={
        <div class="core-sidebar-dedicated-panel core-sidebar-dedicated-panel--compact">
          <AgentsSidebarHeader onCollapse={props.onCollapse} />
          <AgentSelector value={agentSelection()} onChange={updateAgentSelectionFromValue} />
          <ChatbotBuilderSidebarNav activeSection={chatbotSection()} onSectionChange={setChatbotSection} />
          <ChatbotCreditCard />
        </div>
      }
    >
      <Show
        when={agentSelection() !== 'workflow'}
        fallback={
          <div class="core-sidebar-dedicated-panel core-sidebar-dedicated-panel--compact">
            <AgentsSidebarHeader onCollapse={props.onCollapse} />
            <AgentSelector value={agentSelection()} onChange={updateAgentSelectionFromValue} />
            <WorkflowToolsPanel activeTool={workflowTool()} onToolChange={setWorkflowTool} />
          </div>
        }
      >
        <div class="core-sidebar-dedicated-panel">
          <AgentsSidebarHeader onCollapse={props.onCollapse} />
          <AgentSelector value={agentSelection()} onChange={updateAgentSelectionFromValue} />
          <AgentFeatureSidebarNav
            activeFeature={agentFeature()}
            disabled={!selectedCoreAgent()}
            onFeatureChange={setAgentFeature}
            role={selectedCoreAgent()}
          />
        </div>
      </Show>
    </Show>
  )
}

function AgentsSidebarHeader(props: { onCollapse: () => void }) {
  return (
    <SidebarPanelTitle spacing="core-sidebar-title-tight" onCollapse={props.onCollapse}>
      Agents
    </SidebarPanelTitle>
  )
}

function AgentSelector(props: {
  onChange: (value: string) => void
  value: string
}) {
  const [open, setOpen] = createSignal(false)
  const defaultAgentRoleOption = agentRoleOptions[0]!
  const selectedOption = () => agentRoleOptions.find((option) => option.id === props.value) ?? defaultAgentRoleOption

  return (
    <div class="core-sidebar-select core-sidebar-agent-select">
      <span class="core-sidebar-select__icon">
        <Bot class="size-3" strokeWidth={2.1} />
      </span>
      <button
        type="button"
        aria-label="Select agent type"
        aria-haspopup="menu"
        aria-expanded={open()}
        onClick={() => setOpen((current) => !current)}
        class="core-sidebar-select__button"
      >
        <span>{selectedOption().label}</span>
      </button>
      <ChevronDown class={cn('core-sidebar-select__chevron', open() && 'rotate-180')} strokeWidth={2.2} />
      <Show when={open()}>
        <menu class="velion-popover core-sidebar-select__menu" aria-label="Agent type options">
          <For each={agentRoleOptions}>
            {(option) => {
              const selected = () => option.id === props.value
              return (
                <li role="presentation">
                  <button
                    type="button"
                    role="menuitemradio"
                    aria-checked={selected()}
                    class={cn('core-sidebar-select__option', selected() && 'core-sidebar-select__option--selected')}
                    onClick={() => {
                      props.onChange(option.id)
                      setOpen(false)
                    }}
                  >
                    <span>{option.label}</span>
                    <Show when={selected()}>
                      <CheckCircle2 class="size-3.5 core-sidebar-success" strokeWidth={2} />
                    </Show>
                  </button>
                </li>
              )
            }}
          </For>
        </menu>
      </Show>
    </div>
  )
}

function AgentFeatureSidebarNav(props: {
  activeFeature: AgentFeatureId
  disabled: boolean
  onFeatureChange: (feature: AgentFeatureId) => void
  role: CoreAgentRoleId | null
}) {
  const tabs = () => (props.role ? agentFeatureOptionsByRole[props.role] : [])

  return (
    <nav class="core-sidebar-agent-nav" aria-label="Agent feature tabs">
      <Show when={tabs().length === 0}>
        <div class="core-sidebar-agent-empty velion-sidebar-secondary">
          Select Service, Sales, or Ecommerce to configure its features.
        </div>
      </Show>
      <For each={tabs()}>
        {(tab) => {
          const active = () => !props.disabled && props.activeFeature === tab.id
          return (
            <button
              type="button"
              aria-pressed={active()}
              disabled={props.disabled}
              onClick={() => props.onFeatureChange(tab.id)}
              class={cn('core-sidebar-agent-feature', active() && 'core-sidebar-agent-feature--active')}
            >
              <Dynamic component={agentFeatureIcons[tab.id]} class="core-sidebar-dedicated-icon" strokeWidth={2} />
              <span>
                <span>{tab.label}</span>
                <small>{tab.description}</small>
              </span>
            </button>
          )
        }}
      </For>
    </nav>
  )
}

function ChatbotBuilderSidebarNav(props: {
  activeSection: ChatbotBuilderSectionId
  onSectionChange: (section: ChatbotBuilderSectionId) => void
}) {
  return (
    <nav class="core-sidebar-link-list" aria-label="Chatbot builder navigation">
      <For each={chatbotSidebarTabs}>
        {(tab) => {
          const active = () => props.activeSection === tab.id
          return (
            <button
              type="button"
              aria-pressed={active()}
              onClick={() => props.onSectionChange(tab.id)}
              class={cn('core-sidebar-section-link', active() && 'core-sidebar-source-link--active')}
            >
              <Dynamic component={tab.icon} class="core-sidebar-dedicated-icon" strokeWidth={2} />
              <span>{tab.label}</span>
            </button>
          )
        }}
      </For>
    </nav>
  )
}

function ChatbotCreditCard() {
  return (
    <div class="core-sidebar-credit-card">
      <div>
        <span />
        <p>
          <strong>Runtime checks live</strong>
          <small>Support actions, datasets, logs, and analytics show only connected data.</small>
        </p>
      </div>
    </div>
  )
}
