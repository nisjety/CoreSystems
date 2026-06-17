/* eslint-disable solid/prefer-for */
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
} from 'lucide-solid'
import type { JSX } from 'solid-js'
import { cn } from '@/shared/lib/cn'

export type AgentVisualProps = {
  large?: boolean
}

function visualClass(variant: string, large?: boolean) {
  return cn('agent-visual', `agent-visual--${variant}`, large && 'agent-visual--large')
}

export function ServiceVisual(props: AgentVisualProps) {
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
            <span>Velion Service</span>
          </div>
          <span class="agent-visual__status-dot" />
        </div>
        <div class="agent-visual__service-message agent-visual__service-message--dark">Can you help with my order?</div>
        <div class="agent-visual__service-message agent-visual__service-message--light">
          I found the order and can create a support case if needed.
        </div>
      </div>
      <div class="agent-visual__floating-action">
        <MessageSquareText class="agent-visual__floating-icon" />
      </div>
    </div>
  )
}

export function SalesVisual(props: AgentVisualProps) {
  return (
    <div aria-hidden="true" class={visualClass('sales', props.large)}>
      <div class="agent-visual__calendar">
        <div class="agent-visual__calendar-header">
          <span>Calendar rules</span>
          <span>Availability</span>
        </div>
        <div class="agent-visual__calendar-grid">
          {Array.from({ length: 21 }).map((_, index) => (
            <span class={cn('agent-visual__calendar-day', index === 10 && 'agent-visual__calendar-day--active')}>
              {index + 1}
            </span>
          ))}
        </div>
        <div class="agent-visual__calendar-slots">
          {['Owner', 'Team', 'Fallback'].map((slot, index) => (
            <span class={cn('agent-visual__calendar-slot', index === 1 && 'agent-visual__calendar-slot--active')}>
              {slot}
            </span>
          ))}
        </div>
      </div>
      <div class="agent-visual__sales-pill">
        <MessageSquareText class="agent-visual__sales-pill-icon" />
        Book a demo with sales
      </div>
    </div>
  )
}

export function EcommerceVisual(props: AgentVisualProps) {
  return (
    <div aria-hidden="true" class={visualClass('ecommerce', props.large)}>
      <div class="agent-visual__product-window">
        <div class="agent-visual__product-query">Looking for running shoes?</div>
        <div class="agent-visual__product-grid">
          {['#ECEFF3', '#D9D0BE', '#1F2428'].map((color, index) => (
            <div class="agent-visual__product-card">
              <div class="agent-visual__product-swatch" style={{ 'background-color': color }}>
                <span />
              </div>
              <div class="agent-visual__product-line agent-visual__product-line--wide" />
              <div class="agent-visual__product-line agent-visual__product-line--short" />
              <div class="agent-visual__product-label">{index === 0 ? 'Data' : 'Rule'}</div>
            </div>
          ))}
        </div>
      </div>
      <div class="agent-visual__floating-action">
        <ShoppingBag class="agent-visual__floating-icon" />
      </div>
    </div>
  )
}

export function ChatbotVisual(props: AgentVisualProps) {
  return (
    <div aria-hidden="true" class={visualClass('dotted', props.large)}>
      <div class="agent-visual__chatbot-sidebar">
        <div class="agent-visual__chatbot-sidebar-title">
          <Bot class="agent-visual__tiny-icon" />
          Playground
        </div>
        <div class="agent-visual__chatbot-source">Source mapped</div>
        {['Model', 'Actions', 'Instructions'].map((item) => (
          <div class="agent-visual__chatbot-field">{item}</div>
        ))}
      </div>

      <div class="agent-visual__chatbot-device">
        <div class="agent-visual__chatbot-header">
          <div class="agent-visual__label-row">
            <span class="agent-visual__brand-icon agent-visual__brand-icon--round">
              <Sparkles class="agent-visual__tiny-icon" />
            </span>
            <span>Velion Chatbot</span>
          </div>
          <CircleDashed class="agent-visual__muted-icon" />
        </div>
        <div class="agent-visual__chatbot-body">
          <div class="agent-visual__chatbot-bubble">Hi. What can I help you with?</div>
        </div>
        <div class="agent-visual__chatbot-input">
          <span>Message…</span>
          <Send class="agent-visual__tiny-icon" />
        </div>
      </div>
    </div>
  )
}

export function WorkflowVisual(props: AgentVisualProps) {
  return (
    <div aria-hidden="true" class={visualClass('dotted', props.large)}>
      <div class="agent-visual__workflow-tools">
        <div class="agent-visual__workflow-tools-title">
          <Search class="agent-visual__tiny-icon" />
          Tools
        </div>
        <div class="agent-visual__workflow-tool-grid">
          {[Bot, Database, Globe2, TicketCheck, Split, Code2].map((ToolIcon) => (
            <span class="agent-visual__workflow-tool">
              <ToolIcon class="agent-visual__small-icon" />
            </span>
          ))}
        </div>
      </div>

      <div class="agent-visual__workflow-flow">
        <WorkflowNode icon={<CalendarClock class="agent-visual__small-icon" />} label={props.large ? 'Trigger' : undefined} />
        <Connector />
        <WorkflowNode icon={<Bot class="agent-visual__small-icon" />} label={props.large ? 'AI step' : undefined} />
        <Connector />
        <WorkflowNode icon={<GitBranch class="agent-visual__small-icon" />} label={props.large ? 'Branch' : undefined} />
      </div>

      {props.large ? (
        <div class="agent-visual__workflow-prompt">
          Describe your workflow to Velion
          <span>
            <Send class="agent-visual__tiny-icon" />
          </span>
        </div>
      ) : null}

      {props.large ? (
        <div class="agent-visual__workflow-inspector">
          <div class="agent-visual__workflow-inspector-title">
            Generate caption
            <CircleDashed class="agent-visual__muted-icon" />
          </div>
          {['Provider', 'Model', 'Prompt'].map((item, index) => (
            <div class={cn('agent-visual__workflow-field', index === 2 && 'agent-visual__workflow-field--large')}>
              {item}
            </div>
          ))}
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
