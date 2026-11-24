import { createSignal, For, onCleanup, Show } from 'solid-js'

import { Link, Pencil } from '@/shared/icons'
import { useI18n } from '@/shared/i18n'
import { SPACE_MODERATION_REASONS, SPACE_QUICK_REACTIONS } from '@/shared/api/spaces-client'
import type { SpaceModerationReason } from '@/shared/api/spaces-client'
import { SpaceConfirmButton } from './SpaceConfirmButton'
import './space-message-lifecycle.css'

/** How many quick reactions the toolbar offers; the full picker stays on the reaction bar. */
export const SPACE_TOOLBAR_QUICK_REACTIONS = 3

const COPIED_NOTICE_MS = 2_000

/**
 * The hover toolbar of one message (WS12-S7, DEPTH-WS12-S7-1): quick reactions, Copy link, Edit and
 * Delete, each shown only when the viewer may use it. The page decides what to offer from the room
 * role and the organisation's policy hints; the server still decides every change. Delete asks in
 * place (SpaceConfirmButton) and says plainly that the text is no longer available afterwards; a
 * moderator removing someone else's message picks the reason from the closed set first.
 *
 * Keyboard: one Tab stop (roving tabindex). Arrow keys, Home and End move between the controls.
 */
export function SpaceMessageToolbar(props: {
  readonly canEdit: boolean
  readonly canDelete: boolean
  /** The message belongs to someone else (a moderator removal): the confirmation says so. */
  readonly moderating?: boolean
  readonly busy?: boolean
  readonly onEdit: () => void
  /** `reason` is set only for a moderator removal. */
  readonly onDelete: (reason?: SpaceModerationReason) => void
  /** The message's canonical link (spaceEntityHref). Absent hides Copy link. */
  readonly linkHref?: string
  /** A quick reaction is offered (a hint; the server decides). */
  readonly canReact?: boolean
  /** Emoji the viewer already used on this message: pressing one again removes it. */
  readonly reacted?: readonly string[]
  readonly onReact?: (emoji: string, present: boolean) => void
}) {
  const i18n = useI18n()
  const [copied, setCopied] = createSignal<'ok' | 'failed' | undefined>(undefined)
  const [reason, setReason] = createSignal<SpaceModerationReason>('other')
  let copiedTimer: number | undefined
  let container: HTMLDivElement | undefined
  onCleanup(() => {
    if (copiedTimer !== undefined) window.clearTimeout(copiedTimer)
  })

  const quick = () => SPACE_QUICK_REACTIONS.slice(0, SPACE_TOOLBAR_QUICK_REACTIONS)
  const isReacted = (emoji: string) => props.reacted?.includes(emoji) === true
  const offered = () => props.canEdit || props.canDelete || Boolean(props.linkHref) || props.canReact === true

  const reasonLabel = (value: SpaceModerationReason): string => {
    switch (value) {
      case 'policy_violation':
        return i18n.tr('Brudd på retningslinjer', 'Policy violation')
      case 'spam':
        return i18n.tr('Søppelpost', 'Spam')
      case 'harassment':
        return i18n.tr('Trakassering', 'Harassment')
      case 'privacy':
        return i18n.tr('Personvern', 'Privacy')
      case 'other':
        return i18n.tr('Annet', 'Other')
    }
  }

  // Roving tabindex: exactly one control is in the Tab order.
  const items = (): HTMLElement[] =>
    container
      ? Array.from(container.querySelectorAll<HTMLElement>('button:not([disabled]), select:not([disabled])'))
      : []
  const rove = (active: HTMLElement | undefined) => {
    const all = items()
    const current = active && all.includes(active) ? active : all[0]
    for (const item of all) item.tabIndex = item === current ? 0 : -1
  }
  const onKeyDown = (event: KeyboardEvent) => {
    const all = items()
    if (all.length === 0) return
    const target = event.target as HTMLElement
    // A select keeps its own arrow keys.
    if (target.tagName === 'SELECT' && (event.key === 'ArrowUp' || event.key === 'ArrowDown')) return
    const at = Math.max(all.indexOf(target), 0)
    let next: number | undefined
    if (event.key === 'ArrowRight' || event.key === 'ArrowDown') next = (at + 1) % all.length
    else if (event.key === 'ArrowLeft' || event.key === 'ArrowUp') next = (at - 1 + all.length) % all.length
    else if (event.key === 'Home') next = 0
    else if (event.key === 'End') next = all.length - 1
    if (next === undefined) return
    event.preventDefault()
    const item = all[next]
    rove(item)
    item?.focus()
  }

  const copyLink = async () => {
    const href = props.linkHref
    if (!href) return
    if (copiedTimer !== undefined) window.clearTimeout(copiedTimer)
    try {
      const absolute = new URL(href, window.location.origin).href
      await navigator.clipboard.writeText(absolute)
      setCopied('ok')
    } catch {
      setCopied('failed')
    }
    copiedTimer = window.setTimeout(() => setCopied(undefined), COPIED_NOTICE_MS)
  }

  return (
    <Show when={offered()}>
      <div
        ref={(element: HTMLDivElement) => {
          container = element
          // The controls render with the container; set the single Tab stop once they are in place.
          queueMicrotask(() => rove(undefined))
        }}
        class="verevon-message-toolbar"
        role="toolbar"
        aria-label={i18n.tr('Meldingshandlinger', 'Message actions')}
        onKeyDown={onKeyDown}
        onFocusIn={(event) => rove(event.target as HTMLElement)}
      >
        <Show when={props.canReact && props.onReact}>
          <For each={quick()}>
            {(emoji) => (
              <button
                type="button"
                class="verevon-message-toolbar__button verevon-message-toolbar__button--emoji"
                aria-pressed={isReacted(emoji) ? 'true' : 'false'}
                aria-label={
                  isReacted(emoji)
                    ? i18n.tr(`Fjern reaksjonen ${emoji}`, `Remove the ${emoji} reaction`)
                    : i18n.tr(`Reager med ${emoji}`, `React with ${emoji}`)
                }
                disabled={props.busy}
                onClick={() => props.onReact?.(emoji, !isReacted(emoji))}
              >
                <span aria-hidden="true">{emoji}</span>
              </button>
            )}
          </For>
        </Show>
        <Show when={props.linkHref}>
          <button
            type="button"
            class="verevon-message-toolbar__button"
            onClick={() => { void copyLink() }}
          >
            <Link size={14} aria-hidden="true" />
            <span>{i18n.tr('Kopier lenke', 'Copy link')}</span>
          </button>
        </Show>
        <Show when={props.canEdit}>
          <button
            type="button"
            class="verevon-message-toolbar__button"
            disabled={props.busy}
            onClick={() => props.onEdit()}
          >
            <Pencil size={14} aria-hidden="true" />
            <span>{i18n.tr('Rediger', 'Edit')}</span>
          </button>
        </Show>
        <Show when={props.canDelete && props.moderating}>
          <select
            class="verevon-message-toolbar__reason"
            aria-label={i18n.tr('Årsak til fjerning', 'Reason for removal')}
            value={reason()}
            disabled={props.busy}
            onChange={(event) => setReason(event.currentTarget.value as SpaceModerationReason)}
          >
            <For each={SPACE_MODERATION_REASONS}>
              {(value) => <option value={value}>{reasonLabel(value)}</option>}
            </For>
          </select>
        </Show>
        <Show when={props.canDelete}>
          <SpaceConfirmButton
            class="verevon-message-toolbar__button verevon-message-toolbar__button--danger"
            label={i18n.tr('Slett', 'Delete')}
            confirmLabel={i18n.tr('Bekreft sletting av meldingen', 'Confirm deleting the message')}
            consequence={
              props.moderating
                ? i18n.tr(
                    'Alle ser at en leder fjernet meldingen. Teksten er ikke tilgjengelig lenger.',
                    'Everyone sees that a manager removed the message. The text is no longer available.',
                  )
                : i18n.tr(
                    'Alle ser at meldingen er slettet. Teksten er ikke tilgjengelig lenger.',
                    'Everyone sees that the message was deleted. The text is no longer available.',
                  )
            }
            disabled={props.busy}
            onConfirm={() => props.onDelete(props.moderating ? reason() : undefined)}
          />
        </Show>
        <Show when={copied()}>
          {(state) => (
            <span class="verevon-message-toolbar__status" role="status">
              {state() === 'ok'
                ? i18n.tr('Lenken er kopiert.', 'Link copied.')
                : i18n.tr('Kunne ikke kopiere lenken.', 'Could not copy the link.')}
            </span>
          )}
        </Show>
      </div>
    </Show>
  )
}
