import {
  Bot,
  CalendarClock,
  CircleDashed,
  Code2,
  Database,
  GitBranch,
  Globe2,
  MessageSquareText,
  Search,
  Send,
  ShoppingBag,
  Sparkles,
  Split,
  TicketCheck,
} from '@/shared/icons'
import type { JSX } from '@solidjs/web'
import { For } from 'solid-js'
import { cn } from '@/shared/lib/cn'
import { useI18n } from '@/shared/i18n'

export type AgentVisualProps = {
  large?: boolean
}

function visualClass(variant: string, large?: boolean) {
  return cn('agent-visual', `agent-visual--${variant}`, large && 'agent-visual--large')
}

export function ServiceVisual(props: AgentVisualProps) {
  const i18n = useI18n()
  return (
    <div aria-hidden="true" class={visualClass('service', props.large)}>
      <div class="agent-visual__service-ring agent-visual__service-ring--left" />
      <div class="agent-visual__service-ring agent-visual__service-ring--right" />
      <div class="agent-visual__service-window">
        <div class="agent-visual__service-header">
          <div class="agent-visual__label-row">
            <span class="agent-visual__brand-icon">
              <Sparkles class="agent-visual__tiny-icon" />
            </span>
            <span>{i18n.tr('Verevon Service', 'Verevon Service')}</span>
          </div>
          <span class="agent-visual__status-dot" />
        </div>
        <div class="agent-visual__service-message agent-visual__service-message--dark">{i18n.tr('Kan du hjelpe meg med bestillingen min?', 'Can you help with my order?')}</div>
        <div class="agent-visual__service-message agent-visual__service-message--light">
          {i18n.tr('Jeg fant bestillingen og kan opprette en støttesak om nødvendig.', 'I found the order and can create a support case if needed.')}
        </div>
      </div>
      <div class="agent-visual__floating-action">
        <MessageSquareText class="agent-visual__floating-icon" />
      </div>
    </div>
  )
}

export function SalesVisual(props: AgentVisualProps) {
  const i18n = useI18n()
  return (
    <div aria-hidden="true" class={visualClass('sales', props.large)}>
      <div class="agent-visual__calendar">
        <div class="agent-visual__calendar-header">
          <span>{i18n.tr('Kalenderregler', 'Calendar rules')}</span>
          <span>{i18n.tr('Tilgjengelighet', 'Availability')}</span>
        </div>
        <div class="agent-visual__calendar-grid">
          <For each={Array.from({ length: 21 })} keyed={false}>
            {(_, index) => (
              <span class={cn('agent-visual__calendar-day', index === 10 && 'agent-visual__calendar-day--active')}>
                {index + 1}
              </span>
            )}
          </For>
        </div>
        <div class="agent-visual__calendar-slots">
          <For each={[i18n.tr('Eier', 'Owner'), i18n.tr('Team', 'Team'), i18n.tr('Reserve', 'Fallback')]} keyed={false}>
            {(slot, index) => (
              <span class={cn('agent-visual__calendar-slot', index === 1 && 'agent-visual__calendar-slot--active')}>
                {slot()}
              </span>
            )}
          </For>
        </div>
      </div>
      <div class="agent-visual__sales-pill">
        <MessageSquareText class="agent-visual__sales-pill-icon" />
        {i18n.tr('Book en demo med salg', 'Book a demo with sales')}
      </div>
    </div>
  )
}

export function EcommerceVisual(props: AgentVisualProps) {
  const i18n = useI18n()
  return (
    <div aria-hidden="true" class={visualClass('ecommerce', props.large)}>
      <div class="agent-visual__product-window">
        <div class="agent-visual__product-query">{i18n.tr('Leter du etter løpesko?', 'Looking for running shoes?')}</div>
        <div class="agent-visual__product-grid">
          <For each={['#ECEFF3', '#D9D0BE', '#1F2428']} keyed={false}>
            {(color, index) => (
              <div class="agent-visual__product-card">
                <div class="agent-visual__product-swatch" style={{ 'background-color': color() }}>
                  <span />
                </div>
                <div class="agent-visual__product-line agent-visual__product-line--wide" />
                <div class="agent-visual__product-line agent-visual__product-line--short" />
                <div class="agent-visual__product-label">{index === 0 ? i18n.tr('Data', 'Data') : i18n.tr('Regel', 'Rule')}</div>
              </div>
            )}
          </For>
        </div>
      </div>
      <div class="agent-visual__floating-action">
        <ShoppingBag class="agent-visual__floating-icon" />
      </div>
    </div>
  )
}

export function ChatbotVisual(props: AgentVisualProps) {
  const i18n = useI18n()
  return (
    <div aria-hidden="true" class={visualClass('dotted', props.large)}>
      <div class="agent-visual__chatbot-sidebar">
        <div class="agent-visual__chatbot-sidebar-title">
          <Bot class="agent-visual__tiny-icon" />
          {i18n.tr('Testmiljø', 'Playground')}
        </div>
        <div class="agent-visual__chatbot-source">{i18n.tr('Kilde tilordnet', 'Source mapped')}</div>
        <For each={[i18n.tr('Modell', 'Model'), i18n.tr('Handlinger', 'Actions'), i18n.tr('Instruksjoner', 'Instructions')]}>
          {(item) => <div class="agent-visual__chatbot-field">{item}</div>}
        </For>
      </div>

      <div class="agent-visual__chatbot-device">
        <div class="agent-visual__chatbot-header">
          <div class="agent-visual__label-row">
            <span class="agent-visual__brand-icon agent-visual__brand-icon--round">
              <Sparkles class="agent-visual__tiny-icon" />
            </span>
            <span>{i18n.tr('Verevon Chatbot', 'Verevon Chatbot')}</span>
          </div>
          <CircleDashed class="agent-visual__muted-icon" />
        </div>
        <div class="agent-visual__chatbot-body">
          <div class="agent-visual__chatbot-bubble">{i18n.tr('Hei. Hva kan jeg hjelpe deg med?', 'Hi. What can I help you with?')}</div>
        </div>
        <div class="agent-visual__chatbot-input">
          <span>{i18n.tr('Melding …', 'Message…')}</span>
          <Send class="agent-visual__tiny-icon" />
        </div>
      </div>
    </div>
  )
}

export function WorkflowVisual(props: AgentVisualProps) {
  const i18n = useI18n()
  return (
    <div aria-hidden="true" class={visualClass('dotted', props.large)}>
      <div class="agent-visual__workflow-tools">
        <div class="agent-visual__workflow-tools-title">
          <Search class="agent-visual__tiny-icon" />
          {i18n.tr('Verktøy', 'Tools')}
        </div>
        <div class="agent-visual__workflow-tool-grid">
          <For each={[Bot, Database, Globe2, TicketCheck, Split, Code2]}>
            {(ToolIcon) => (
              <span class="agent-visual__workflow-tool">
                <ToolIcon class="agent-visual__small-icon" />
              </span>
            )}
          </For>
        </div>
      </div>

      <div class="agent-visual__workflow-flow">
        <WorkflowNode icon={<CalendarClock class="agent-visual__small-icon" />} label={props.large ? i18n.tr('Utløser', 'Trigger') : undefined} />
        <Connector />
        <WorkflowNode icon={<Bot class="agent-visual__small-icon" />} label={props.large ? i18n.tr('AI-steg', 'AI step') : undefined} />
        <Connector />
        <WorkflowNode icon={<GitBranch class="agent-visual__small-icon" />} label={props.large ? i18n.tr('Forgrening', 'Branch') : undefined} />
      </div>

      {props.large ? (
        <div class="agent-visual__workflow-prompt">
          {i18n.tr('Beskriv arbeidsflyten din til Verevon', 'Describe your workflow to Verevon')}
          <span>
            <Send class="agent-visual__tiny-icon" />
          </span>
        </div>
      ) : null}

      {props.large ? (
        <div class="agent-visual__workflow-inspector">
          <div class="agent-visual__workflow-inspector-title">
            {i18n.tr('Generer bildetekst', 'Generate caption')}
            <CircleDashed class="agent-visual__muted-icon" />
          </div>
          <For each={[i18n.tr('Leverandør', 'Provider'), i18n.tr('Modell', 'Model'), i18n.tr('Prompt', 'Prompt')]} keyed={false}>
            {(item, index) => (
              <div class={cn('agent-visual__workflow-field', index === 2 && 'agent-visual__workflow-field--large')}>
                {item()}
              </div>
            )}
          </For>
        </div>
      ) : null}
    </div>
  )
}

function WorkflowNode(props: {
  icon: JSX.Element
  label?: string
}) {
  return (
    <div class="agent-visual__workflow-node-wrap">
      <span class="agent-visual__workflow-node">{props.icon}</span>
      {props.label ? <span class="agent-visual__workflow-node-label">{props.label}</span> : null}
    </div>
  )
}

function Connector() {
  return (
    <span class="agent-visual__connector">
      <span />
    </span>
  )
}
