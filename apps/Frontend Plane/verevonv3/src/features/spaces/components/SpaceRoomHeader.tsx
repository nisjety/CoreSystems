import { createEffect, createSignal, For, Show } from 'solid-js'
import type { JSX } from '@solidjs/web'

import { Info, X } from '@/shared/icons'
import type { SpaceRoomDetails, SpaceRosterMember } from '@/shared/api/spaces-client'
import { useI18n } from '@/shared/i18n'

import { initials } from './sidepane/SpaceTurnList'
import { avatarTextColor, memberTint } from '../lib/agent-avatar'
import { canArchiveRoom, SpaceRoomArchiveControl } from './SpaceRoomArchive'
import { SpaceRoomSettings } from './SpaceRoomSettings'
import './space-room-header.css'

/** How many avatars the stack draws before the count carries the rest. */
export const HEADER_AVATAR_LIMIT = 4

// The tint lives with the other avatar helpers so the timeline draws the same person in the same colour.
export { memberTint }

/**
 * The people the stack draws and the count beside it. Agents are not people and are shown in
 * the Agent tab, so they are neither drawn nor counted. Roster order is kept.
 */
export function headerPeople(roster: readonly SpaceRosterMember[]): {
  readonly shown: readonly SpaceRosterMember[]
  readonly total: number
} {
  const people = roster.filter((member) => member.subject_type === 'user')
  return { shown: people.slice(0, HEADER_AVATAR_LIMIT), total: people.length }
}

export interface SpaceRoomHeaderProps {
  readonly spaceRef: string
  readonly name: string
  readonly topic: string | null | undefined
  readonly kind: string
  readonly lifecycle: string
  readonly role: string
  readonly isOrganizationRoom: boolean | undefined
  readonly archived: boolean
  /** `features.room_details` and `features.room_archive`. */
  readonly roomDetailsEnabled: boolean
  readonly roomArchiveEnabled: boolean
  /** The roster, or `undefined` while it loads or when it could not be read (nothing is drawn then). */
  readonly roster: readonly SpaceRosterMember[] | undefined
  /** Label and value pairs for the details panel (type, status, your role). */
  readonly facts: readonly { readonly label: string; readonly value: string }[]
  /** The quiet live line next to the name: who is here, and whether updates are live. */
  readonly status?: JSX.Element
  readonly onOpenMembers: () => void
  readonly onSaved?: (details: SpaceRoomDetails) => void
  readonly onArchiveChanged?: () => void
}

/**
 * The room's header, the way Slack and Teams draw it: the name, then the topic in muted text,
 * and on the right the people in the room and one details button. Everything that used to
 * crowd the header (kind, lifecycle, role, the rename form, archive) lives in the details panel
 * that button opens.
 */
export function SpaceRoomHeader(props: SpaceRoomHeaderProps) {
  const i18n = useI18n()
  const [open, setOpen] = createSignal(false)
  let root: HTMLDivElement | undefined
  let trigger: HTMLButtonElement | undefined

  const people = () => headerPeople(props.roster ?? [])
  const topic = () => props.topic?.trim() ?? ''
  const canArchive = () =>
    canArchiveRoom({
      role: props.role,
      kind: props.kind,
      isOrganizationRoom: props.isOrganizationRoom,
      featureOn: props.roomArchiveEnabled,
    })

  // Closed by Escape (focus goes back to the button) and by a press anywhere outside the panel.
  createEffect(open, (isOpen) => {
    if (!isOpen) return undefined
    const onPointer = (event: MouseEvent) => {
      if (root && event.target instanceof Node && !root.contains(event.target)) setOpen(false)
    }
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return
      setOpen(false)
      trigger?.focus()
    }
    document.addEventListener('mousedown', onPointer)
    document.addEventListener('keydown', onKey)
    return () => {
      document.removeEventListener('mousedown', onPointer)
      document.removeEventListener('keydown', onKey)
    }
  })

  return (
    <header class="verevon-space-header">
      <div class="verevon-space-header__title">
        <h1 id="space-title">{props.name}</h1>
        <Show when={topic()}>
          {(text) => <p class="verevon-space-header__topic" title={text()}>{text()}</p>}
        </Show>
        {props.status}
      </div>

      <div class="verevon-space-header__actions">
        <Show when={people().total > 0}>
          <button
            type="button"
            class="verevon-space-header__people"
            aria-label={i18n.tr(`Vis medlemmer (${people().total})`, `Show members (${people().total})`)}
            onClick={() => props.onOpenMembers()}
          >
            <span class="verevon-space-header__stack" aria-hidden="true">
              <For each={people().shown}>
                {(member) => (
                  <span
                    class="verevon-space-header__avatar"
                    style={{ background: memberTint(member.subject_id), color: avatarTextColor(memberTint(member.subject_id)) }}
                  >
                    {initials(member.display_name || member.subject_id)}
                  </span>
                )}
              </For>
            </span>
            <span class="verevon-space-header__count" aria-hidden="true">{people().total}</span>
          </button>
        </Show>

        <div class="verevon-space-details" ref={(element) => { root = element }}>
          <button
            type="button"
            class="verevon-space-header__details"
            aria-label={i18n.tr('Romdetaljer', 'Room details')}
            aria-haspopup="dialog"
            aria-expanded={open() ? 'true' : 'false'}
            aria-controls="space-room-details"
            ref={(element) => { trigger = element }}
            onClick={() => setOpen(!open())}
          >
            <Info size={17} aria-hidden="true" />
          </button>
          <Show when={open()}>
            <div
              id="space-room-details"
              class="verevon-space-details__panel"
              role="dialog"
              aria-label={i18n.tr('Romdetaljer', 'Room details')}
              tabindex="-1"
              ref={(element) => { queueMicrotask(() => element.focus()) }}
            >
              <div class="verevon-space-details__head">
                <h2>{props.name}</h2>
                <button
                  type="button"
                  class="verevon-space-details__close"
                  aria-label={i18n.tr('Lukk romdetaljer', 'Close room details')}
                  onClick={() => {
                    setOpen(false)
                    trigger?.focus()
                  }}
                >
                  <X size={16} aria-hidden="true" />
                </button>
              </div>

              <Show when={topic()}>
                {(text) => <p class="verevon-space-details__topic">{text()}</p>}
              </Show>

              <dl class="verevon-space-details__facts">
                <For each={props.facts}>
                  {(fact) => (
                    <div>
                      <dt>{fact.label}</dt>
                      <dd>{fact.value}</dd>
                    </div>
                  )}
                </For>
                <Show when={props.roster !== undefined}>
                  <div>
                    <dt>{i18n.tr('Medlemmer', 'Members')}</dt>
                    <dd>
                      {people().total}
                      <button
                        type="button"
                        class="verevon-space-details__link"
                        onClick={() => {
                          setOpen(false)
                          props.onOpenMembers()
                        }}
                      >
                        {i18n.tr('Se medlemmer', 'View members')}
                      </button>
                    </dd>
                  </div>
                </Show>
              </dl>

              <SpaceRoomSettings
                spaceRef={props.spaceRef}
                name={props.name}
                topic={props.topic}
                role={props.role}
                kind={props.kind}
                lifecycle={props.lifecycle}
                archived={props.archived}
                enabled={props.roomDetailsEnabled}
                onSaved={props.onSaved}
              />

              <Show when={canArchive()}>
                <SpaceRoomArchiveControl
                  spaceRef={props.spaceRef}
                  archived={props.archived}
                  onChanged={props.onArchiveChanged}
                />
              </Show>
            </div>
          </Show>
        </div>
      </div>
    </header>
  )
}
