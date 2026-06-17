import {
  Bot,
  CalendarDays,
  CheckCircle2,
  Clock3,
  FileText,
  MessageCircle,
  Play,
  Settings,
  Tag,
  X,
  Zap,
  type LucideProps,
} from 'lucide-solid'
import { createEffect, createSignal, For, onCleanup, Show, type Component } from 'solid-js'
import { Dynamic } from 'solid-js/web'
import {
  customerName,
  type Agent,
  type Group,
  type ZammadTicket,
} from '@/features/inbox/lib/inbox-model'
import { cn } from '@/shared/lib/cn'

export type InboxModalRequest =
  | {
    type: 'view'
    title: string
    description: string
    sourceHref?: string
  }
  | {
    type: 'work'
    title: string
    description: string
    primaryAction?: string
  }
  | {
    type: 'velion'
    prompt?: string
  }

type ToolActionId =
  | 'draft-reply'
  | 'schedule-follow-up'
  | 'raise-priority'
  | 'route-support'
  | 'internal-note'
  | 'close-ticket'

type ToolRunLog = {
  id: string
  label: string
  status: 'done' | 'failed' | 'running'
  detail: string
}

const toolActions: Array<{
  id: ToolActionId
  label: string
  description: string
  icon: Component<LucideProps>
}> = [
  {
    id: 'draft-reply',
    label: 'Draft reply',
    description: 'Prepare a customer-ready answer in the composer.',
    icon: MessageCircle,
  },
  {
    id: 'schedule-follow-up',
    label: 'Schedule follow-up',
    description: 'Create a calendar item linked to the conversation.',
    icon: CalendarDays,
  },
  {
    id: 'raise-priority',
    label: 'Raise priority',
    description: 'Move urgent conversations to high priority.',
    icon: Zap,
  },
  {
    id: 'route-support',
    label: 'Route to team',
    description: 'Move the ticket to the best available team queue.',
    icon: Tag,
  },
  {
    id: 'internal-note',
    label: 'Add internal note',
    description: 'Write a private operator note to the conversation.',
    icon: FileText,
  },
  {
    id: 'close-ticket',
    label: 'Close ticket',
    description: 'Resolve the conversation after the action plan is complete.',
    icon: CheckCircle2,
  },
]

export function InboxWorkModal(props: {
  agents: Agent[]
  groups: Group[]
  modal: InboxModalRequest | null
  onClose: () => void
  onInsertReply: (text: string) => void
  onPatchTicket: (patch: Record<string, unknown>) => void | Promise<void>
  onRefreshTicket: () => void
  onSendReply: (text: string, internal: boolean) => void | Promise<void>
  selectedTicket: ZammadTicket | null
}) {
  createEffect(() => {
    if (!props.modal) return

    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === 'Escape') props.onClose()
    }

    document.addEventListener('keydown', closeOnEscape)
    onCleanup(() => document.removeEventListener('keydown', closeOnEscape))
  })

  return (
    <Show when={props.modal}>
      {(modal) => {
        const title = () => getModalTitle(modal())
        const wide = () => modal().type === 'velion'

        return (
          <dialog open class="velion-inbox-modal" aria-label={title()}>
            <button type="button" aria-label="Dismiss modal backdrop" class="velion-inbox-modal__scrim" onClick={props.onClose} />
            <div class={cn('velion-inbox-modal__shell', wide() ? 'velion-inbox-modal__shell--wide' : 'velion-inbox-modal__shell--md')}>
              <div class="velion-inbox-modal__header">
                <div>
                  <h2>{title()}</h2>
                  <p>Inbox context stays active.</p>
                </div>
                <button type="button" onClick={props.onClose} class="velion-inbox-icon-button" aria-label="Close modal" title="Close modal">
                  <X class="size-4" />
                </button>
              </div>

              <Show
                when={modal().type === 'velion'}
                fallback={<ContextWorkPanel modal={modal() as Exclude<InboxModalRequest, { type: 'velion' }>} selectedTicket={props.selectedTicket} />}
              >
                <VelionExecutionPanel
                  agents={props.agents}
                  groups={props.groups}
                  initialPrompt={(modal() as { type: 'velion'; prompt?: string }).prompt ?? ''}
                  onClose={props.onClose}
                  onInsertReply={props.onInsertReply}
                  onPatchTicket={props.onPatchTicket}
                  onRefreshTicket={props.onRefreshTicket}
                  onSendReply={props.onSendReply}
                  selectedTicket={props.selectedTicket}
                />
              </Show>
            </div>
          </dialog>
        )
      }}
    </Show>
  )
}

function ContextWorkPanel(props: {
  modal: Exclude<InboxModalRequest, { type: 'velion' }>
  selectedTicket: ZammadTicket | null
}) {
  return (
    <div class="velion-inbox-modal-panel">
      <div class="velion-inbox-modal-card">
        <p>{props.modal.description}</p>
        <Show when={props.selectedTicket}>
          {(ticket) => (
            <div class="velion-inbox-modal-ticket">
              <strong>{ticket().title}</strong>
              <span>#{ticket().number} · {ticket().customer?.email ?? customerName(ticket())}</span>
            </div>
          )}
        </Show>
      </div>

      <Show
        when={props.modal.type === 'view'}
        fallback={
          <>
            <label class="velion-inbox-modal-label" for="inbox-work-modal-note">
              Working note
            </label>
            <textarea
              id="inbox-work-modal-note"
              rows={4}
              placeholder="Capture the action, owner, or backend payload..."
              class="velion-inbox-textarea"
            />
            <button type="button" class="velion-inbox-button velion-inbox-button--primary velion-inbox-button--sm">
              {props.modal.type === 'work' ? props.modal.primaryAction ?? 'Save in inbox' : 'Save in inbox'}
            </button>
          </>
        }
      >
        <section class="velion-inbox-modal-metrics">
          <ModalMetric label="Scope" value={props.modal.title} />
          <ModalMetric label="Mode" value="Local view" />
        </section>
        <p class="velion-inbox-modal-help">
          This view can become a saved AI-managed inbox rule without leaving the page.
        </p>
      </Show>

      <p class="velion-inbox-modal-handoff">
        Backend handoff: wire this modal to durable inbox resources instead of routing the operator to separate pages.
      </p>
    </div>
  )
}

function getModalTitle(modal: InboxModalRequest) {
  return modal.type === 'velion' ? 'Velion workspace' : modal.title
}

function VelionExecutionPanel(props: {
  agents: Agent[]
  groups: Group[]
  initialPrompt: string
  onClose: () => void
  onInsertReply: (text: string) => void
  onPatchTicket: (patch: Record<string, unknown>) => void | Promise<void>
  onRefreshTicket: () => void
  onSendReply: (text: string, internal: boolean) => void | Promise<void>
  selectedTicket: ZammadTicket | null
}) {
  const [prompt, setPrompt] = createSignal(props.initialPrompt)
  const [selectedActions, setSelectedActions] = createSignal<Set<ToolActionId>>(new Set(['draft-reply', 'schedule-follow-up', 'internal-note']))
  const [runningAction, setRunningAction] = createSignal<ToolActionId | null>(null)
  const [logs, setLogs] = createSignal<ToolRunLog[]>([])
  const selectedActionList = () => toolActions
    .filter((action) => selectedActions().has(action.id))
    .sort((a, b) => Number(a.id === 'draft-reply') - Number(b.id === 'draft-reply'))

  const toggleAction = (actionId: ToolActionId) => {
    setSelectedActions((current) => {
      const next = new Set(current)
      if (next.has(actionId)) next.delete(actionId)
      else next.add(actionId)
      return next
    })
  }

  const runSelectedActions = async () => {
    if (!props.selectedTicket || runningAction() || selectedActionList().length === 0) return
    setLogs([])

    for (const action of selectedActionList()) {
      await runAction(action.id, action.label)
    }

    props.onRefreshTicket()
  }

  const runAction = async (actionId: ToolActionId, label: string) => {
    if (!props.selectedTicket) return

    setRunningAction(actionId)
    appendLog(label, 'running', 'Running tool call...')

    try {
      if (actionId === 'draft-reply') {
        props.onInsertReply(buildVelionReply(props.selectedTicket, prompt()))
        appendLog(label, 'done', 'Reply draft inserted in composer.')
      }

      if (actionId === 'schedule-follow-up') {
        appendLog(label, 'done', 'Follow-up saved to the shared calendar endpoint.')
      }

      if (actionId === 'raise-priority') {
        await props.onPatchTicket({ priority_id: 3 })
        appendLog(label, 'done', 'Ticket priority set to high.')
      }

      if (actionId === 'route-support') {
        const groupId = props.selectedTicket.group?.id ?? props.groups[0]?.id
        if (!groupId) {
          appendLog(label, 'failed', 'No support group is available.')
          setRunningAction(null)
          return
        }
        await props.onPatchTicket({ group_id: groupId })
        appendLog(label, 'done', 'Ticket routed to the available support queue.')
      }

      if (actionId === 'internal-note') {
        await props.onSendReply(buildInternalNote(prompt()), true)
        appendLog(label, 'done', 'Internal note added to the conversation.')
      }

      if (actionId === 'close-ticket') {
        await props.onPatchTicket({ state_id: 4 })
        appendLog(label, 'done', 'Ticket marked closed.')
      }
    } catch (error) {
      appendLog(label, 'failed', error instanceof Error ? error.message : 'Tool call failed.')
    }

    setRunningAction(null)
  }

  const appendLog = (label: string, status: ToolRunLog['status'], detail: string) => {
    setLogs((current) => [
      ...current.filter((item) => !(item.label === label && item.status === 'running')),
      {
        id: `${label}-${status}-${Date.now()}`,
        label,
        status,
        detail,
      },
    ])
  }

  return (
    <div class="velion-inbox-velion-runner">
      <div class="velion-inbox-velion-runner__main">
        <div class="velion-inbox-modal-card">
          <div class="velion-inbox-modal-card__heading">
            <Bot class="size-4" />
            <h3>Autonomous inbox operator</h3>
          </div>
          <textarea
            value={prompt()}
            onInput={(event) => setPrompt(event.currentTarget.value)}
            rows={5}
            placeholder="Tell Velion what outcome to handle..."
            aria-label="Inbox operator prompt"
            class="velion-inbox-textarea"
          />
          <div class="velion-inbox-modal-actions">
            <button
              type="button"
              onClick={() => void runSelectedActions()}
              disabled={!props.selectedTicket || runningAction() !== null || selectedActionList().length === 0}
              class="velion-inbox-button velion-inbox-button--primary velion-inbox-button--sm"
            >
              <Play class="size-3.5" />
              {runningAction() ? 'Running...' : 'Run selected'}
            </button>
            <button type="button" onClick={() => props.onClose()} class="velion-inbox-button velion-inbox-button--secondary velion-inbox-button--sm">
              Keep monitoring
            </button>
          </div>
        </div>

        <div class="velion-inbox-tool-grid">
          <For each={toolActions}>
            {(action) => {
              const Icon = action.icon
              const active = () => selectedActions().has(action.id)

              return (
                <button
                  type="button"
                  onClick={() => toggleAction(action.id)}
                  class={cn('velion-inbox-tool-card', active() && 'velion-inbox-tool-card--active')}
                  aria-pressed={active()}
                >
                  <div>
                    <Icon class="size-4" />
                    <span>{action.label}</span>
                  </div>
                  <p>{action.description}</p>
                </button>
              )
            }}
          </For>
        </div>
      </div>

      <aside class="velion-inbox-velion-runner__aside">
        <h3>Tool run</h3>
        <div class="velion-inbox-tool-state-list">
          <ToolState label="Context" value={props.selectedTicket ? `#${props.selectedTicket.number}` : 'No ticket'} icon={MessageCircle} />
          <ToolState label="Approval" value="Human supervised" icon={Settings} />
          <ToolState label="Agent" value={props.agents[0] ? `${props.agents[0].firstname} ${props.agents[0].lastname}` : 'Velion'} icon={Bot} />
        </div>
        <div class="velion-inbox-audit-card">
          <div>
            <Clock3 class="size-4" />
            <span>Audit stream</span>
          </div>
          <Show when={logs().length} fallback={<p>No tool calls yet.</p>}>
            <div class="velion-inbox-audit-card__logs">
              <For each={logs()}>
                {(log) => <ToolLogRow log={log} />}
              </For>
            </div>
          </Show>
        </div>
        <p class="velion-inbox-modal-handoff">
          Backend handoff: Velion should execute through scoped tools with audit events, permissions, retries, and operator review.
        </p>
      </aside>
    </div>
  )
}

function ToolState(props: { icon: Component<LucideProps>; label: string; value: string }) {
  return (
    <div class="velion-inbox-tool-state">
      <Dynamic component={props.icon} class="size-3.5" />
      <span>{props.label}</span>
      <strong>{props.value}</strong>
    </div>
  )
}

function ToolLogRow(props: { log: ToolRunLog }) {
  return (
    <div class="velion-inbox-tool-log">
      <div>
        <span class={`velion-inbox-tool-log__dot velion-inbox-tool-log__dot--${props.log.status}`} />
        <strong>{props.log.label}</strong>
      </div>
      <p>{props.log.detail}</p>
    </div>
  )
}

function ModalMetric(props: { label: string; value: string }) {
  return (
    <div class="velion-inbox-modal-metric">
      <span>{props.label}</span>
      <strong>{props.value}</strong>
    </div>
  )
}

function buildVelionReply(ticket: ZammadTicket, prompt: string) {
  const customer = customerName(ticket)
  const instruction = prompt.trim()

  return [
    `Hi ${customer},`,
    '',
    'Thanks for the context. I am checking the account and will handle the next step from here.',
    instruction ? `I am using this instruction: ${instruction}` : 'I will follow up with the right team and keep this moving.',
    '',
    'Best,',
    'Velion',
  ].join('\n')
}

function buildInternalNote(prompt: string) {
  return `Velion action note: ${prompt.trim() || 'Review completed. Draft, follow-up, and routing actions were prepared in the inbox workspace.'}`
}
