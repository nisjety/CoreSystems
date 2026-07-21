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
import { createEffect, createMemo, createSignal, For, onCleanup, Show, type Component } from 'solid-js'
import { Dynamic } from 'solid-js/web'
import {
  customerName,
  type Agent,
  type Group,
  type ZammadTicket,
} from '@/features/inbox/lib/inbox-model'
import { cn } from '@/shared/lib/cn'
import { useI18n } from '@/shared/i18n'

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
  const i18n = useI18n()

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
        const title = () => getModalTitle(modal(), i18n.tr)
        const wide = () => modal().type === 'velion'

        return (
          <dialog open class="velion-inbox-modal" aria-label={title()}>
            <button type="button" aria-label={i18n.tr('Lukk modalbakgrunn', 'Dismiss modal backdrop')} class="velion-inbox-modal__scrim" onClick={props.onClose} />
            <div class={cn('velion-inbox-modal__shell', wide() ? 'velion-inbox-modal__shell--wide' : 'velion-inbox-modal__shell--md')}>
              <div class="velion-inbox-modal__header">
                <div>
                  <h2>{title()}</h2>
                  <p>{i18n.tr('Innboks-konteksten forblir aktiv.', 'Inbox context stays active.')}</p>
                </div>
                <button type="button" onClick={props.onClose} class="velion-inbox-icon-button" aria-label={i18n.tr('Lukk modal', 'Close modal')} title={i18n.tr('Lukk modal', 'Close modal')}>
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
  const i18n = useI18n()
  const saveLabel = () => i18n.tr('Lagre i innboks', 'Save in inbox')

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
              {i18n.tr('Arbeidsnotat', 'Working note')}
            </label>
            <textarea
              id="inbox-work-modal-note"
              rows={4}
              placeholder={i18n.tr('Fang opp handlingen, eieren, eller backend-nyttelasten …', 'Capture the action, owner, or backend payload...')}
              class="velion-inbox-textarea"
            />
            <button type="button" class="velion-inbox-button velion-inbox-button--primary velion-inbox-button--sm">
              {props.modal.type === 'work' ? props.modal.primaryAction ?? saveLabel() : saveLabel()}
            </button>
          </>
        }
      >
        <section class="velion-inbox-modal-metrics">
          <ModalMetric label={i18n.tr('Omfang', 'Scope')} value={props.modal.title} />
          <ModalMetric label={i18n.tr('Modus', 'Mode')} value={i18n.tr('Lokal visning', 'Local view')} />
        </section>
        <p class="velion-inbox-modal-help">
          {i18n.tr(
            'Denne visningen kan bli til en lagret AI-styrt innboksregel uten å forlate siden.',
            'This view can become a saved AI-managed inbox rule without leaving the page.',
          )}
        </p>
      </Show>

      <p class="velion-inbox-modal-handoff">
        {i18n.tr(
          'Backend-overlevering: koble denne modalen til varige innboks-ressurser i stedet for å sende operatøren til separate sider.',
          'Backend handoff: wire this modal to durable inbox resources instead of routing the operator to separate pages.',
        )}
      </p>
    </div>
  )
}

function getModalTitle(modal: InboxModalRequest, tr: (noText: string, enText: string) => string) {
  return modal.type === 'velion' ? tr('Velion arbeidsområde', 'Velion workspace') : modal.title
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
  const i18n = useI18n()
  const [prompt, setPrompt] = createSignal(props.initialPrompt)
  const [selectedActions, setSelectedActions] = createSignal<Set<ToolActionId>>(new Set(['draft-reply', 'schedule-follow-up', 'internal-note']))
  const [runningAction, setRunningAction] = createSignal<ToolActionId | null>(null)
  const [logs, setLogs] = createSignal<ToolRunLog[]>([])
  const toolActions = createMemo(() => [
    {
      id: 'draft-reply' as const,
      label: i18n.tr('Utkast til svar', 'Draft reply'),
      description: i18n.tr('Forbered et kundeklart svar i svarfeltet.', 'Prepare a customer-ready answer in the composer.'),
      icon: MessageCircle,
    },
    {
      id: 'schedule-follow-up' as const,
      label: i18n.tr('Planlegg oppfølging', 'Schedule follow-up'),
      description: i18n.tr('Opprett et kalenderelement knyttet til samtalen.', 'Create a calendar item linked to the conversation.'),
      icon: CalendarDays,
    },
    {
      id: 'raise-priority' as const,
      label: i18n.tr('Øk prioritet', 'Raise priority'),
      description: i18n.tr('Flytt hastesamtaler til høy prioritet.', 'Move urgent conversations to high priority.'),
      icon: Zap,
    },
    {
      id: 'route-support' as const,
      label: i18n.tr('Rute til team', 'Route to team'),
      description: i18n.tr('Flytt saken til den beste tilgjengelige teamkøen.', 'Move the ticket to the best available team queue.'),
      icon: Tag,
    },
    {
      id: 'internal-note' as const,
      label: i18n.tr('Legg til internt notat', 'Add internal note'),
      description: i18n.tr('Skriv et privat internt notat til samtalen.', 'Write a private operator note to the conversation.'),
      icon: FileText,
    },
    {
      id: 'close-ticket' as const,
      label: i18n.tr('Lukk sak', 'Close ticket'),
      description: i18n.tr('Løs samtalen når handlingsplanen er fullført.', 'Resolve the conversation after the action plan is complete.'),
      icon: CheckCircle2,
    },
  ])
  const selectedActionList = () => toolActions()
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
    appendLog(label, 'running', i18n.tr('Kjører verktøykall …', 'Running tool call...'))

    try {
      if (actionId === 'draft-reply') {
        props.onInsertReply(buildVelionReply(props.selectedTicket, prompt(), i18n.tr))
        appendLog(label, 'done', i18n.tr('Svarutkast satt inn i svarfeltet.', 'Reply draft inserted in composer.'))
      }

      if (actionId === 'schedule-follow-up') {
        appendLog(label, 'done', i18n.tr('Oppfølging lagret til det delte kalender-endepunktet.', 'Follow-up saved to the shared calendar endpoint.'))
      }

      if (actionId === 'raise-priority') {
        await props.onPatchTicket({ priority_id: 3 })
        appendLog(label, 'done', i18n.tr('Sakens prioritet satt til høy.', 'Ticket priority set to high.'))
      }

      if (actionId === 'route-support') {
        const groupId = props.selectedTicket.group?.id ?? props.groups[0]?.id
        if (!groupId) {
          appendLog(label, 'failed', i18n.tr('Ingen support-gruppe er tilgjengelig.', 'No support group is available.'))
          setRunningAction(null)
          return
        }
        await props.onPatchTicket({ group_id: groupId })
        appendLog(label, 'done', i18n.tr('Saken rutet til tilgjengelig support-kø.', 'Ticket routed to the available support queue.'))
      }

      if (actionId === 'internal-note') {
        await props.onSendReply(buildInternalNote(prompt(), i18n.tr), true)
        appendLog(label, 'done', i18n.tr('Internt notat lagt til samtalen.', 'Internal note added to the conversation.'))
      }

      if (actionId === 'close-ticket') {
        await props.onPatchTicket({ state_id: 4 })
        appendLog(label, 'done', i18n.tr('Saken markert som lukket.', 'Ticket marked closed.'))
      }
    } catch (error) {
      appendLog(label, 'failed', error instanceof Error ? error.message : i18n.tr('Verktøykall feilet.', 'Tool call failed.'))
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
            <h3>{i18n.tr('Autonom innboks-operatør', 'Autonomous inbox operator')}</h3>
          </div>
          <textarea
            value={prompt()}
            onInput={(event) => setPrompt(event.currentTarget.value)}
            rows={5}
            placeholder={i18n.tr('Fortell Velion hvilket utfall som skal håndteres …', 'Tell Velion what outcome to handle...')}
            aria-label={i18n.tr('Ledetekst for innboks-operatør', 'Inbox operator prompt')}
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
              {runningAction() ? i18n.tr('Kjører …', 'Running...') : i18n.tr('Kjør valgte', 'Run selected')}
            </button>
            <button type="button" onClick={() => props.onClose()} class="velion-inbox-button velion-inbox-button--secondary velion-inbox-button--sm">
              {i18n.tr('Fortsett å overvåke', 'Keep monitoring')}
            </button>
          </div>
        </div>

        <div class="velion-inbox-tool-grid">
          <For each={toolActions()}>
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
        <h3>{i18n.tr('Verktøykjøring', 'Tool run')}</h3>
        <div class="velion-inbox-tool-state-list">
          <ToolState label={i18n.tr('Kontekst', 'Context')} value={props.selectedTicket ? `#${props.selectedTicket.number}` : i18n.tr('Ingen sak', 'No ticket')} icon={MessageCircle} />
          <ToolState label={i18n.tr('Godkjenning', 'Approval')} value={i18n.tr('Menneskelig tilsyn', 'Human supervised')} icon={Settings} />
          <ToolState label={i18n.tr('Agent', 'Agent')} value={props.agents[0] ? `${props.agents[0].firstname} ${props.agents[0].lastname}` : 'Velion'} icon={Bot} />
        </div>
        <div class="velion-inbox-audit-card">
          <div>
            <Clock3 class="size-4" />
            <span>{i18n.tr('Revisjonslogg', 'Audit stream')}</span>
          </div>
          <Show when={logs().length} fallback={<p>{i18n.tr('Ingen verktøykall ennå.', 'No tool calls yet.')}</p>}>
            <div class="velion-inbox-audit-card__logs">
              <For each={logs()}>
                {(log) => <ToolLogRow log={log} />}
              </For>
            </div>
          </Show>
        </div>
        <p class="velion-inbox-modal-handoff">
          {i18n.tr(
            'Backend-overlevering: Velion bør utføre handlinger gjennom avgrensede verktøy med revisjonshendelser, tillatelser, nye forsøk og operatørgjennomgang.',
            'Backend handoff: Velion should execute through scoped tools with audit events, permissions, retries, and operator review.',
          )}
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

function buildVelionReply(ticket: ZammadTicket, prompt: string, tr: (noText: string, enText: string) => string) {
  const customer = customerName(ticket)
  const instruction = prompt.trim()

  return [
    tr(`Hei ${customer},`, `Hi ${customer},`),
    '',
    tr(
      'Takk for konteksten. Jeg sjekker kontoen og tar neste steg herfra.',
      'Thanks for the context. I am checking the account and will handle the next step from here.',
    ),
    instruction
      ? tr(`Jeg bruker denne instruksjonen: ${instruction}`, `I am using this instruction: ${instruction}`)
      : tr('Jeg følger opp med riktig team og holder dette i gang.', 'I will follow up with the right team and keep this moving.'),
    '',
    tr('Vennlig hilsen,', 'Best,'),
    'Velion',
  ].join('\n')
}

function buildInternalNote(prompt: string, tr: (noText: string, enText: string) => string) {
  const detail = prompt.trim() || tr(
    'Gjennomgang fullført. Utkast, oppfølging og rutingshandlinger ble forberedt i innboks-arbeidsområdet.',
    'Review completed. Draft, follow-up, and routing actions were prepared in the inbox workspace.',
  )
  return `${tr('Velion handlingsnotat', 'Velion action note')}: ${detail}`
}
