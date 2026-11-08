import { createSignal, For, Show } from 'solid-js'
import type { JSX } from '@solidjs/web'

import { hideSpaceDm, type SpaceRosterMember } from '@/shared/api/spaces-client'
import { translateApiError, useI18n } from '@/shared/i18n'

import { initials } from './sidepane/SpaceTurnList'
import { avatarTextColor, memberTint } from '../lib/agent-avatar'
import { headerPeople } from './SpaceRoomHeader'
import './space-room-header.css'
import './space-dm.css'

export interface SpaceDmHeaderProps {
  readonly spaceRef: string
  /** The other people, by name (see `spaceDisplayName`). */
  readonly name: string
  readonly kind: string
  /** The roster, or `undefined` while it loads; only drawn as avatars. */
  readonly roster: readonly SpaceRosterMember[] | undefined
  /** The quiet live line next to the name. */
  readonly status?: JSX.Element
  /** Called once the DM is hidden from the caller's sidebar. */
  readonly onHidden?: () => void
}

/**
 * The header of a direct message or group message: the other people's names, their avatars and
 * one "Hide" button. No topic, no rename, no archive, no member controls: the roster is fixed at
 * registration and a DM has no managers. Hide only removes it from this person's sidebar; the
 * conversation and the other people are untouched, and a new message brings it back.
 */
export function SpaceDmHeader(props: SpaceDmHeaderProps) {
  const i18n = useI18n()
  const [busy, setBusy] = createSignal(false)
  const [error, setError] = createSignal('')
  const people = () => headerPeople(props.roster ?? [])

  async function hide() {
    if (busy()) return
    setBusy(true)
    setError('')
    try {
      await hideSpaceDm(props.spaceRef)
      props.onHidden?.()
    } catch (err) {
      setError(
        translateApiError(err, i18n.tr, {
          no: 'Meldingen kunne ikke skjules. Ingenting er endret.',
          en: 'The message could not be hidden. Nothing was changed.',
        }),
      )
    } finally {
      setBusy(false)
    }
  }

  return (
    <header class="verevon-space-header">
      <div class="verevon-space-header__title">
        <h1 id="space-title">{props.name}</h1>
        <p class="space-dm-header__kind">
          {props.kind === 'group_dm'
            ? i18n.tr('Gruppesamtale', 'Group message')
            : i18n.tr('Direktemelding', 'Direct message')}
        </p>
        {props.status}
      </div>

      <div class="verevon-space-header__actions">
        <Show when={people().total > 0}>
          <span
            class="verevon-space-header__people"
            role="img"
            aria-label={i18n.tr(`${people().total} deltakere`, `${people().total} participants`)}
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
          </span>
        </Show>
        <button type="button" class="space-dm-header__hide" disabled={busy()} onClick={() => void hide()}>
          {i18n.tr('Skjul', 'Hide')}
        </button>
      </div>
      <Show when={error()}>
        <p role="alert">{error()}</p>
      </Show>
    </header>
  )
}
