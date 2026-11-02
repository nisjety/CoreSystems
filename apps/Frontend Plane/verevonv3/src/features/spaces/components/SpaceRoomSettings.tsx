import { createEffect, createSignal, Show } from 'solid-js'

import {
  updateSpaceRoomDetails,
  updateSpaceRoomSettings,
  type RoomJoinRole,
  type RoomVisibility,
  type SpaceRoomDetails,
  type SpaceRoomSettingsResult,
} from '@/shared/api/spaces-client'
import { translateApiError, useI18n } from '@/shared/i18n'

import './space-room-settings.css'

// The form's length hints. The authoritative rules (NFC, trim, whitespace collapse, name 1-120,
// topic 0-250, no control or bidi characters) run in Application on every write; these only
// stop typing past the limit so the common mistake never reaches the server.
const NAME_MAX = 120
const TOPIC_MAX = 250

/** Owner and manager only. Mirrors the gateway's own gate; the server enforces it regardless. */
export function canEditRoomDetails(role: string): boolean {
  return role === 'owner' || role === 'manager'
}

export interface SpaceRoomSettingsProps {
  readonly spaceRef: string
  readonly name: string
  readonly topic: string | null | undefined
  /** The caller's own room role from Control's membership read. */
  readonly role: string
  /** The room's `kind` and `lifecycle`: only an active, named room has editable details. */
  readonly kind: string
  readonly lifecycle: string
  /** An archived room is read-only for everyone, its name and topic included. */
  readonly archived?: boolean
  /** The room's `features.room_details`. Absent or off, nothing renders. */
  readonly enabled: boolean
  /** Called with the stored values after a save, so the page can show them without a reload. */
  readonly onSaved?: (details: SpaceRoomDetails) => void
  /** WS04-S8: the room's `features.room_directory`. Absent or off, the visibility block does not render. */
  readonly directoryEnabled?: boolean
  /** The organization room follows the organization and is never opened or closed here. */
  readonly isOrganizationRoom?: boolean
  /** Control's current visibility and self-join role. Absent on an older Control: shown as unknown. */
  readonly visibility?: string | null
  readonly joinRole?: string | null
  /** Called after the visibility or join role was saved, so the page can reload the room context. */
  readonly onSettingsSaved?: (settings: SpaceRoomSettingsResult) => void
}

/**
 * The room's name and topic, editable by an owner or manager (WS04-S2). It lives in the room
 * details panel opened from the header, not in the header itself. Presentation only: the
 * gateway re-checks the role from Control on every write and Application normalizes the text, so
 * hiding this form for anyone else is courtesy, not security. The topic is a display label and
 * is never sent to a model.
 */
export function SpaceRoomSettings(props: SpaceRoomSettingsProps) {
  const i18n = useI18n()
  const [open, setOpen] = createSignal(false)
  const [name, setName] = createSignal('')
  const [topic, setTopic] = createSignal('')
  const [busy, setBusy] = createSignal(false)
  const [error, setError] = createSignal('')

  const visible = () =>
    props.enabled
    && props.archived !== true
    && props.kind === 'room'
    && props.lifecycle === 'active'
    && canEditRoomDetails(props.role)

  function begin() {
    setName(props.name)
    setTopic(props.topic ?? '')
    setError('')
    setOpen(true)
  }

  function invalidCopy(err: unknown): string | undefined {
    const code = (err as { code?: unknown } | null)?.code
    if (code !== 'room_details_invalid') return undefined
    const details = (err as { details?: Readonly<Record<string, unknown>> | null }).details
    const field = details?.field
    const reason = details?.reason
    if (field === 'name' && reason === 'name_required') {
      return i18n.tr('Rommet må ha et navn.', 'The room needs a name.')
    }
    if (field === 'name' && (reason === 'name_too_long' || reason === 'too_long')) {
      return i18n.tr(`Navnet kan ha opptil ${NAME_MAX} tegn.`, `The name can be up to ${NAME_MAX} characters.`)
    }
    if (field === 'topic' && (reason === 'topic_too_long' || reason === 'too_long')) {
      return i18n.tr(`Emnet kan ha opptil ${TOPIC_MAX} tegn.`, `The topic can be up to ${TOPIC_MAX} characters.`)
    }
    if (reason === 'control_character' || reason === 'bidi_override') {
      return i18n.tr(
        'Navnet og emnet kan ikke inneholde kontrolltegn.',
        'The name and topic cannot contain control characters.',
      )
    }
    return undefined
  }

  async function save(event: Event) {
    event.preventDefault()
    if (busy()) return
    // Send only what changed, so a topic-only edit never rewrites the name.
    const body: { name?: string; topic?: string } = {}
    if (name() !== props.name) body.name = name()
    if (topic() !== (props.topic ?? '')) body.topic = topic()
    if (body.name === undefined && body.topic === undefined) {
      setOpen(false)
      return
    }
    setBusy(true)
    setError('')
    try {
      const saved = await updateSpaceRoomDetails(props.spaceRef, body)
      props.onSaved?.(saved)
      setOpen(false)
    } catch (err) {
      setError(
        invalidCopy(err) ??
          translateApiError(err, i18n.tr, {
            no: 'Navnet og emnet kunne ikke lagres. Ingenting er endret.',
            en: 'The name and topic could not be saved. Nothing was changed.',
          }),
      )
    } finally {
      setBusy(false)
    }
  }

  return (
    <>
    <Show when={visible()}>
      <div class="verevon-space-room-settings">
        <Show
          when={open()}
          fallback={
            <button type="button" class="verevon-space-secondary-action" onClick={begin}>
              {i18n.tr('Rediger navn og emne', 'Edit name and topic')}
            </button>
          }
        >
          <form class="verevon-space-room-settings__form" onSubmit={save}>
            <label class="verevon-space-room-settings__field">
              <span>{i18n.tr('Navn', 'Name')}</span>
              <input
                type="text"
                value={name()}
                maxlength={NAME_MAX}
                required
                disabled={busy()}
                onInput={(event) => setName(event.currentTarget.value)}
              />
            </label>
            <label class="verevon-space-room-settings__field">
              <span>{i18n.tr('Emne', 'Topic')}</span>
              <input
                type="text"
                value={topic()}
                maxlength={TOPIC_MAX}
                disabled={busy()}
                placeholder={i18n.tr('Hva handler rommet om?', 'What is this room about?')}
                onInput={(event) => setTopic(event.currentTarget.value)}
              />
            </label>
            <Show when={error()}>
              {(message) => (
                <p class="verevon-space-projection-error" role="alert">{message()}</p>
              )}
            </Show>
            <div class="verevon-space-room-settings__actions">
              <button type="submit" class="verevon-space-secondary-action" disabled={busy()}>
                {busy() ? i18n.tr('Lagrer...', 'Saving...') : i18n.tr('Lagre', 'Save')}
              </button>
              <button
                type="button"
                class="verevon-space-secondary-action"
                disabled={busy()}
                onClick={() => setOpen(false)}
              >
                {i18n.tr('Avbryt', 'Cancel')}
              </button>
            </div>
          </form>
        </Show>
      </div>
    </Show>
    <SpaceRoomVisibility
      spaceRef={props.spaceRef}
      role={props.role}
      kind={props.kind}
      lifecycle={props.lifecycle}
      archived={props.archived}
      enabled={props.directoryEnabled === true}
      isOrganizationRoom={props.isOrganizationRoom}
      visibility={props.visibility}
      joinRole={props.joinRole}
      onSaved={props.onSettingsSaved}
    />
    </>
  )
}

interface SpaceRoomVisibilityProps {
  readonly spaceRef: string
  readonly role: string
  readonly kind: string
  readonly lifecycle: string
  readonly archived?: boolean
  readonly enabled: boolean
  readonly isOrganizationRoom?: boolean
  readonly visibility?: string | null
  readonly joinRole?: string | null
  readonly onSaved?: (settings: SpaceRoomSettingsResult) => void
}

const asVisibility = (value: string | null | undefined): RoomVisibility | undefined =>
  value === 'org_public' ? 'org_public' : value === 'private' ? 'private' : undefined
const asJoinRole = (value: string | null | undefined): RoomJoinRole | undefined =>
  value === 'editor' ? 'editor' : value === 'viewer' ? 'viewer' : undefined

/**
 * WS04-S8: who can find and enter the room. Owner and manager choose between private (the
 * default: invitation only) and open to the organization's room directory, and the role a
 * self-join receives (viewer or editor, never more). Presentation only: Control re-checks the
 * role, the organization's directory policy and the room kind on every change, and answers the
 * typed refusal this renders. The organization room, personal Spaces and archived rooms never
 * show it.
 */
function SpaceRoomVisibility(props: SpaceRoomVisibilityProps) {
  const i18n = useI18n()
  const [visibility, setVisibility] = createSignal<RoomVisibility>(asVisibility(props.visibility) ?? 'private')
  const [joinRole, setJoinRole] = createSignal<RoomJoinRole>(asJoinRole(props.joinRole) ?? 'viewer')
  const [busy, setBusy] = createSignal(false)
  const [error, setError] = createSignal('')
  const [saved, setSaved] = createSignal(false)

  const visible = () =>
    props.enabled
    && props.archived !== true
    && props.kind === 'room'
    && props.lifecycle === 'active'
    && props.isOrganizationRoom !== true
    && canEditRoomDetails(props.role)

  // Follow the server's value whenever the context reloads.
  createEffect(
    () => [props.visibility, props.joinRole] as const,
    ([nextVisibility, nextJoinRole]) => {
      setVisibility(asVisibility(nextVisibility) ?? 'private')
      setJoinRole(asJoinRole(nextJoinRole) ?? 'viewer')
    },
  )

  async function save(event: Event) {
    event.preventDefault()
    if (busy()) return
    // Send only what changed, so a role-only edit never rewrites the visibility.
    const body: { visibility?: RoomVisibility; joinRole?: RoomJoinRole } = {}
    if (visibility() !== (asVisibility(props.visibility) ?? 'private')) body.visibility = visibility()
    if (joinRole() !== (asJoinRole(props.joinRole) ?? 'viewer')) body.joinRole = joinRole()
    if (body.visibility === undefined && body.joinRole === undefined) return
    setBusy(true)
    setError('')
    setSaved(false)
    try {
      const result = await updateSpaceRoomSettings(props.spaceRef, body)
      setSaved(true)
      props.onSaved?.(result)
    } catch (err) {
      setError(
        translateApiError(err, i18n.tr, {
          no: 'Innstillingene kunne ikke lagres. Ingenting er endret.',
          en: 'The settings could not be saved. Nothing was changed.',
        }),
      )
    } finally {
      setBusy(false)
    }
  }

  return (
    <Show when={visible()}>
      <form class="verevon-space-room-settings verevon-space-room-visibility" onSubmit={save}>
        <fieldset class="verevon-space-room-settings__form" disabled={busy()}>
          <legend>{i18n.tr('Hvem kan finne rommet', 'Who can find this room')}</legend>
          <label class="verevon-space-room-visibility__choice">
            <input
              type="radio"
              name={`visibility-${props.spaceRef}`}
              checked={visibility() === 'private'}
              onChange={() => {
                setSaved(false)
                setVisibility('private')
              }}
            />
            <span>{i18n.tr('Privat: bare inviterte medlemmer', 'Private: invited members only')}</span>
          </label>
          <label class="verevon-space-room-visibility__choice">
            <input
              type="radio"
              name={`visibility-${props.spaceRef}`}
              checked={visibility() === 'org_public'}
              onChange={() => {
                setSaved(false)
                setVisibility('org_public')
              }}
            />
            <span>
              {i18n.tr(
                'Åpent for organisasjonen: alle kan finne og bli med i romkatalogen',
                'Open to the organization: anyone can find and join it in the room directory',
              )}
            </span>
          </label>
          <Show when={visibility() === 'org_public'}>
            <label class="verevon-space-room-settings__field">
              <span>{i18n.tr('Rolle når noen blir med selv', 'Role someone gets when they join')}</span>
              <select
                value={joinRole()}
                onChange={(event) => {
                  setSaved(false)
                  setJoinRole(event.currentTarget.value === 'editor' ? 'editor' : 'viewer')
                }}
              >
                <option value="viewer">{i18n.tr('Leser', 'Viewer')}</option>
                <option value="editor">{i18n.tr('Redaktør', 'Editor')}</option>
              </select>
            </label>
          </Show>
          <Show when={error()}>
            {(message) => (
              <p class="verevon-space-projection-error" role="alert">{message()}</p>
            )}
          </Show>
          <Show when={saved()}>
            <p class="verevon-space-room-visibility__saved" role="status">
              {i18n.tr('Lagret.', 'Saved.')}
            </p>
          </Show>
          <div class="verevon-space-room-settings__actions">
            <button type="submit" class="verevon-space-secondary-action" disabled={busy()}>
              {busy() ? i18n.tr('Lagrer...', 'Saving...') : i18n.tr('Lagre', 'Save')}
            </button>
          </div>
        </fieldset>
      </form>
    </Show>
  )
}
