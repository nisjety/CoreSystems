import { Portal } from '@solidjs/web'
import { createSignal, onCleanup, Show } from 'solid-js'
import type { JSX } from '@solidjs/web'

import { useI18n } from '@/shared/i18n'
import { Bot } from '@/shared/icons'
import { formatWhen } from '../lib/space-thread-presentation'
import type { AgentLead, IdentityCardModel } from '../lib/agent-activity'
import { AgentAvatar } from './AgentAvatar'
import './space-identity-card.css'

/**
 * Identity hover card (WS07-S8): wraps the author name of a turn or the name on
 * an agent card and shows who or what it is, leading with the most important
 * state. It opens after a short hover delay and immediately on keyboard focus,
 * closes on Escape, blur or leaving, and stays open while the pointer is on it.
 *
 * Library decision (WS07-D9): the in-house popover, not Kobalte. The repo pins
 * `@kobalte/core` 2.0.0-alpha.0 and uses it only for a switch; its HoverCard has
 * not been exercised under Solid 2 rc and the plan allows this fallback. The
 * card is read-only: no controls, no approval detail, nothing that decides.
 */

const OPEN_DELAY_MS = 350
const CLOSE_DELAY_MS = 120
const CARD_WIDTH = 300
const VIEWPORT_MARGIN = 8
const GAP = 6
const FLIP_THRESHOLD = 170

let cardSequence = 0

const ROLE_LABELS: Record<string, { no: string; en: string }> = {
  viewer: { no: 'Leser', en: 'Viewer' },
  editor: { no: 'Redaktør', en: 'Editor' },
  manager: { no: 'Leder', en: 'Manager' },
  owner: { no: 'Eier', en: 'Owner' },
}

export interface SpaceIdentityHoverCardProps {
  readonly model: IdentityCardModel
  readonly children: JSX.Element
}

export function SpaceIdentityHoverCard(props: SpaceIdentityHoverCardProps) {
  const cardId = `verevon-identity-card-${(cardSequence += 1)}`
  const [open, setOpen] = createSignal(false)
  const [place, setPlace] = createSignal<{ left: number; top?: number; bottom?: number }>({ left: VIEWPORT_MARGIN })
  let trigger: HTMLSpanElement | undefined
  let timer: ReturnType<typeof setTimeout> | undefined

  const clearTimer = () => {
    if (timer !== undefined) clearTimeout(timer)
    timer = undefined
  }
  const onDocumentKey = (event: KeyboardEvent) => {
    if (event.key === 'Escape') hide()
  }
  const show = () => {
    clearTimer()
    if (open()) return
    const rect = trigger?.getBoundingClientRect()
    if (rect) {
      const width = Math.min(CARD_WIDTH, window.innerWidth - VIEWPORT_MARGIN * 2)
      const left = Math.max(VIEWPORT_MARGIN, Math.min(rect.left, window.innerWidth - width - VIEWPORT_MARGIN))
      const flip = window.innerHeight - rect.bottom < FLIP_THRESHOLD && rect.top > FLIP_THRESHOLD
      setPlace(flip ? { left, bottom: window.innerHeight - rect.top + GAP } : { left, top: rect.bottom + GAP })
    }
    setOpen(true)
    document.addEventListener('keydown', onDocumentKey)
  }
  function hide() {
    clearTimer()
    document.removeEventListener('keydown', onDocumentKey)
    setOpen(false)
  }
  const showSoon = () => {
    clearTimer()
    if (open()) return
    timer = setTimeout(show, OPEN_DELAY_MS)
  }
  const hideSoon = () => {
    clearTimer()
    timer = setTimeout(hide, CLOSE_DELAY_MS)
  }
  onCleanup(hide)

  return (
    <>
      <span
        ref={trigger}
        class="verevon-identity-trigger"
        tabindex="0"
        aria-describedby={open() ? cardId : undefined}
        onMouseEnter={showSoon}
        onMouseLeave={hideSoon}
        onFocus={show}
        onBlur={hide}
      >
        {props.children}
      </span>
      <Show when={open()}>
        <Portal>
          <div
            id={cardId}
            role="tooltip"
            class="verevon-identity-card"
            style={{
              left: `${place().left}px`,
              ...(place().top !== undefined ? { top: `${place().top}px` } : {}),
              ...(place().bottom !== undefined ? { bottom: `${place().bottom}px` } : {}),
              width: `min(${CARD_WIDTH}px, calc(100vw - ${VIEWPORT_MARGIN * 2}px))`,
            }}
            onMouseEnter={clearTimer}
            onMouseLeave={hideSoon}
          >
            <CardBody model={props.model} />
          </div>
        </Portal>
      </Show>
    </>
  )
}

function CardBody(props: { readonly model: IdentityCardModel }) {
  const i18n = useI18n()
  const model = () => props.model
  const roleLabel = (role: string) => {
    const label = ROLE_LABELS[role]
    return label ? i18n.tr(label.no, label.en) : role
  }
  return (
    <>
      <div class="verevon-identity-card__head">
        <AgentAvatar
          kind={model().kind}
          size={36}
          name={model().name}
          color={model().kind === 'agent' ? (model() as { color?: string }).color : undefined}
        />
        <div class="verevon-identity-card__who">
          <p class="verevon-identity-card__name">{model().name}</p>
          <Show
            when={model().kind === 'agent'}
            fallback={
              <Show when={(model() as { role?: string }).role}>
                {(role) => <p class="verevon-identity-card__sub">{roleLabel(role())}</p>}
              </Show>
            }
          >
            <p class="verevon-identity-card__sub">
              <span class="verevon-identity-card__chip">
                <Bot size={11} aria-hidden="true" />
                {i18n.tr('Agent', 'Agent')}
              </span>
              <Show when={(model() as { title?: string }).title}>
                {(title) => <span> {title()}</span>}
              </Show>
            </p>
          </Show>
        </div>
      </div>
      <Show
        when={model().kind === 'agent'}
        fallback={
          <Show when={(model() as { lastPostAt?: string }).lastPostAt}>
            {(at) => (
              <p class="verevon-identity-card__lead">
                {i18n.tr('Siste innlegg ', 'Last post ')}{formatWhen(at(), { locale: i18n.locale() })}
              </p>
            )}
          </Show>
        }
      >
        <LeadLine
          lead={(model() as { lead: AgentLead }).lead}
          ownerName={(model() as { ownerName?: string }).ownerName}
        />
      </Show>
    </>
  )
}

function LeadLine(props: { readonly lead: AgentLead; readonly ownerName?: string }) {
  const i18n = useI18n()
  return (
    <>
      <Show when={props.lead.kind === 'blocking'}>
        <p class="verevon-identity-card__lead verevon-identity-card__lead--blocking">
          {props.ownerName
            ? i18n.tr(`Venter på ${props.ownerName}`, `Waiting for ${props.ownerName}`)
            : i18n.tr('Venter på eieren av samtalen', 'Waiting for the conversation owner')}
        </p>
      </Show>
      <Show when={props.lead.kind === 'working'}>
        <p class="verevon-identity-card__lead">
          {(props.lead as { threadTitle?: string }).threadTitle
            ? i18n.tr(
                `Jobber i ${(props.lead as { threadTitle: string }).threadTitle}`,
                `Working in ${(props.lead as { threadTitle: string }).threadTitle}`,
              )
            : i18n.tr('Jobber nå', 'Working now')}
        </p>
      </Show>
      <Show when={props.lead.kind === 'last'}>
        <p class="verevon-identity-card__lead">
          <span class="verevon-identity-card__label">{i18n.tr('Siste svar', 'Last reply')}</span>
          {(props.lead as { text: string }).text}
          <Show when={(props.lead as { at?: string }).at}>
            {(at) => <span class="verevon-identity-card__when"> · {formatWhen(at(), { locale: i18n.locale() })}</span>}
          </Show>
        </p>
      </Show>
    </>
  )
}
