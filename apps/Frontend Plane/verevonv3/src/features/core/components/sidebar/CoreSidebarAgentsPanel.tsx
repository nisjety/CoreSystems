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
} from '@/shared/icons'
import { createMemo, createSignal, For, Show, type Component } from 'solid-js'
import type { LucideProps } from '@/shared/icons'
import { Dynamic } from '@solidjs/web'
import {
  agentFeatureOptionsByRole,
  agentRoleOptions,
  chatbotBuilderSectionOptions,
  isCoreAgentRoleId,
  type AgentFeatureId,
  type AgentSelectionId,
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
import { useI18n } from '@/shared/i18n'
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

const agentRoleLabelText: Record<AgentSelectionId, { en: string; no: string }> = {
  all: { en: 'All roles', no: 'Alle roller' },
  chatbot: { en: 'Chatbot', no: 'Chatbot' },
  ecommerce: { en: 'Ecommerce', no: 'E-handel' },
  sales: { en: 'Sales', no: 'Salg' },
  service: { en: 'Service', no: 'Service' },
  workflow: { en: 'Workflow builder', no: 'Workflow-bygger' },
}

const agentFeatureLabelText: Record<AgentFeatureId, { description: { en: string; no: string }; label: { en: string; no: string } }> = {
  'commerce-brand': {
    description: { en: 'AI Personas, social replies, tone, and autonomous CX.', no: 'AI-personaer, sosiale svar, tone og autonom kundeopplevelse.' },
    label: { en: 'Brand voice', no: 'Merkevarestemme' },
  },
  'commerce-cart': {
    description: { en: 'Checkout concerns and conversion nudges.', no: 'Bekymringer i kassen og konverteringsdytt.' },
    label: { en: 'Cart recovery', no: 'Gjenopprett handlekurv' },
  },
  'commerce-insights': {
    description: { en: 'Intent, friction, and revenue opportunities.', no: 'Intensjon, friksjon og inntektsmuligheter.' },
    label: { en: 'Shopper insights', no: 'Kundeinnsikt' },
  },
  'commerce-product-finder': {
    description: { en: 'Guided Search, discovery, and comparison.', no: 'Guidet søk, oppdagelse og sammenligning.' },
    label: { en: 'Product finder', no: 'Produktfinner' },
  },
  'commerce-shopping': {
    description: { en: 'Product questions, quick replies, upsells, and recommendations.', no: 'Produktspørsmål, raske svar, mersalg og anbefalinger.' },
    label: { en: 'Shopping assistant', no: 'Shoppingassistent' },
  },
  'commerce-store': {
    description: { en: 'Sidekick-style guidance, content, apps, tasks, and approvals.', no: 'Sidekick-lignende veiledning, innhold, apper, oppgaver og godkjenninger.' },
    label: { en: 'Store actions', no: 'Butikkhandlinger' },
  },
  'commerce-support': {
    description: { en: 'Tracking, returns, exchanges, and subscriptions.', no: 'Sporing, returer, bytter og abonnementer.' },
    label: { en: 'Support & orders', no: 'Support og ordre' },
  },
  'sales-booking': {
    description: { en: 'Owner calendars, live-chat booking, and meeting links.', no: 'Eierkalendere, booking i livechat og møtelenker.' },
    label: { en: 'Meeting booking', no: 'Møtebooking' },
  },
  'sales-crm': {
    description: { en: 'Breeze-style research, buying signals, outreach, and CRM context.', no: 'Breeze-lignende research, kjøpssignaler, outreach og CRM-kontekst.' },
    label: { en: 'CRM handoff', no: 'CRM-overlevering' },
  },
  'sales-insights': {
    description: { en: 'Conversion quality, audit trail, automation controls, and objections.', no: 'Konverteringskvalitet, revisjonsspor, automasjonskontroll og innvendinger.' },
    label: { en: 'Pipeline insights', no: 'Pipeline-innsikt' },
  },
  'sales-lead-capture': {
    description: { en: 'Piper-style proactive greetings, guided tours, and visitor engagement.', no: 'Piper-lignende proaktive hilsener, guidede turer og besøksengasjement.' },
    label: { en: 'Lead capture', no: 'Leadfangst' },
  },
  'sales-objections': {
    description: { en: 'Agentforce-style product answers, pricing, security, and timing.', no: 'Agentforce-lignende produktsvar, pris, sikkerhet og timing.' },
    label: { en: 'Objections', no: 'Innvendinger' },
  },
  'sales-qualification': {
    description: { en: 'Drift-style visitor intelligence, fit score, discovery, and routing.', no: 'Drift-lignende besøksintelligens, fit-score, discovery og ruting.' },
    label: { en: 'Qualification', no: 'Kvalifisering' },
  },
  'service-actions': {
    description: { en: 'Ada-style Actions, procedures, ticketing, lookup, and routing tools.', no: 'Ada-lignende handlinger, prosedyrer, ticketing, oppslag og rutingverktøy.' },
    label: { en: 'Service actions', no: 'Servicehandlinger' },
  },
  'service-channels': {
    description: { en: 'Chat, email, voice, social, third-party, and inbox rollout.', no: 'Chat, e-post, tale, sosiale kanaler, tredjepart og innboksutrulling.' },
    label: { en: 'Channels', no: 'Kanaler' },
  },
  'service-insights': {
    description: { en: 'Suggestions, content recommendations, trends, and escalations.', no: 'Forslag, innholdsanbefalinger, trender og eskaleringer.' },
    label: { en: 'Insights', no: 'Innsikt' },
  },
  'service-knowledge': {
    description: { en: 'Trusted sources, Fin-style guidance, attributes, and answer coverage.', no: 'Pålitelige kilder, Fin-lignende veiledning, attributter og svardekning.' },
    label: { en: 'Knowledge', no: 'Kunnskap' },
  },
  'service-quality': {
    description: { en: 'Source fit, policy match, verified resolution, and review queues.', no: 'Kildetreff, policytreff, verifisert løsning og review-køer.' },
    label: { en: 'Quality supervisor', no: 'Kvalitetskontroll' },
  },
  'service-resolution': {
    description: { en: 'Autonomous answers, actions, verified QA, and handoff states.', no: 'Autonome svar, handlinger, verifisert QA og overleveringsstatus.' },
    label: { en: 'Resolution queue', no: 'Løsningskø' },
  },
}

const chatbotSidebarLabelText: Record<ChatbotBuilderSectionId, { en: string; no: string }> = {
  actions: { en: 'Tools', no: 'Verktøy' },
  analytics: { en: 'Analytics', no: 'Analyse' },
  'chat-logs': { en: 'Chat logs', no: 'Chatlogger' },
  'data-sources': { en: 'Fine-tuning', no: 'Finjustering' },
  insights: { en: 'Insights', no: 'Innsikt' },
  install: { en: 'Install', no: 'Installer' },
  integrations: { en: 'Integrations', no: 'Integrasjoner' },
  leads: { en: 'Leads', no: 'Leads' },
  playground: { en: 'Playground', no: 'Testflate' },
  settings: { en: 'Settings', no: 'Innstillinger' },
}

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
  const i18n = useI18n()
  return (
    <SidebarPanelTitle spacing="core-sidebar-title-tight" onCollapse={props.onCollapse}>
      {i18n.tr('Agenter', 'Agents')}
    </SidebarPanelTitle>
  )
}

function AgentSelector(props: {
  onChange: (value: string) => void
  value: string
}) {
  const i18n = useI18n()
  const [open, setOpen] = createSignal(false)
  const defaultAgentRoleOption = agentRoleOptions[0]!
  const selectedOption = () => agentRoleOptions.find((option) => option.id === props.value) ?? defaultAgentRoleOption
  const selectedLabel = () => agentRoleLabel(selectedOption().id, i18n)

  return (
    <div class="core-sidebar-select core-sidebar-agent-select">
      <span class="core-sidebar-select__icon">
        <Bot class="size-3" strokeWidth={2.1} />
      </span>
      <button
        type="button"
        aria-label={i18n.tr('Velg agenttype', 'Select agent type')}
        aria-haspopup="menu"
        aria-expanded={open() ? 'true' : 'false'}
        onClick={() => setOpen((current) => !current)}
        class="core-sidebar-select__button"
      >
        <span>{selectedLabel()}</span>
      </button>
      <ChevronDown class={cn('core-sidebar-select__chevron', open() && 'rotate-180')} strokeWidth={2.2} />
      <Show when={open()}>
        <menu class="verevon-popover core-sidebar-select__menu" aria-label={i18n.tr('Valg for agenttype', 'Agent type options')}>
          <For each={agentRoleOptions}>
            {(option) => {
              const selected = () => option.id === props.value
              return (
                <li role="presentation">
                  <button
                    type="button"
                    role="menuitemradio"
                    aria-checked={selected() ? 'true' : 'false'}
                    class={cn('core-sidebar-select__option', selected() && 'core-sidebar-select__option--selected')}
                    onClick={() => {
                      props.onChange(option.id)
                      setOpen(false)
                    }}
                  >
                    <span>{agentRoleLabel(option.id, i18n)}</span>
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
  const i18n = useI18n()
  const tabs = () => (props.role ? agentFeatureOptionsByRole[props.role] : [])

  return (
    <nav class="core-sidebar-agent-nav" aria-label={i18n.tr('Agentfaner', 'Agent feature tabs')}>
      <Show when={tabs().length === 0}>
        <div class="core-sidebar-agent-empty verevon-sidebar-secondary">
          {i18n.tr('Velg Service, Salg eller E-handel for å konfigurere funksjoner.', 'Select Service, Sales, or Ecommerce to configure its features.')}
        </div>
      </Show>
      <For each={tabs()}>
        {(tab) => {
          const active = () => !props.disabled && props.activeFeature === tab.id
          return (
            <button
              type="button"
              aria-pressed={active() ? 'true' : 'false'}
              disabled={props.disabled}
              onClick={() => props.onFeatureChange(tab.id)}
              class={cn('core-sidebar-agent-feature', active() && 'core-sidebar-agent-feature--active')}
            >
              <Dynamic component={agentFeatureIcons[tab.id]} class="core-sidebar-dedicated-icon" strokeWidth={2} />
              <span>
                <span>{agentFeatureLabel(tab.id, 'label', i18n)}</span>
                <small>{agentFeatureLabel(tab.id, 'description', i18n)}</small>
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
  const i18n = useI18n()
  return (
    <nav class="core-sidebar-link-list" aria-label={i18n.tr('Chatbot-byggernavigasjon', 'Chatbot builder navigation')}>
      <For each={chatbotSidebarTabs}>
        {(tab) => {
          const active = () => props.activeSection === tab.id
          return (
            <button
              type="button"
              aria-pressed={active() ? 'true' : 'false'}
              onClick={() => props.onSectionChange(tab.id)}
              class={cn('core-sidebar-section-link', active() && 'core-sidebar-source-link--active')}
            >
              <Dynamic component={tab.icon} class="core-sidebar-dedicated-icon" strokeWidth={2} />
              <span>{chatbotSidebarLabel(tab.id, i18n)}</span>
            </button>
          )
        }}
      </For>
    </nav>
  )
}

function agentRoleLabel(id: AgentSelectionId, i18n: ReturnType<typeof useI18n>): string {
  const label = agentRoleLabelText[id]
  return i18n.tr(label.no, label.en)
}

function agentFeatureLabel(id: AgentFeatureId, field: 'description' | 'label', i18n: ReturnType<typeof useI18n>): string {
  const label = agentFeatureLabelText[id][field]
  return i18n.tr(label.no, label.en)
}

function chatbotSidebarLabel(id: ChatbotBuilderSectionId, i18n: ReturnType<typeof useI18n>): string {
  const label = chatbotSidebarLabelText[id]
  return i18n.tr(label.no, label.en)
}

function ChatbotCreditCard() {
  const i18n = useI18n()
  return (
    <div class="core-sidebar-credit-card">
      <div>
        <span />
        <p>
          <strong>{i18n.tr('Runtime-sjekker er aktive', 'Runtime checks live')}</strong>
          <small>{i18n.tr('Supporthandlinger, datasett, logger og analyse viser bare tilkoblet data.', 'Support actions, datasets, logs, and analytics show only connected data.')}</small>
        </p>
      </div>
    </div>
  )
}
