import { createSignal, Show } from 'solid-js'

import { Lock } from '@/shared/icons'
import { archiveSpaceRoom, unarchiveSpaceRoom } from '@/shared/api/spaces-client'
import { translateApiError, useI18n } from '@/shared/i18n'

import { SpaceConfirmButton } from './SpaceConfirmButton'

/**
 * Who may archive or unarchive a room: an owner or manager of a named room that somebody made.
 * Mirrors the gateway's own gate (`room_role_forbidden`, `room_not_archivable`,
 * `room_personal_space`); the server enforces it regardless, so hiding the door is courtesy.
 *
 * `isOrganizationRoom` is `undefined` when an older gateway did not say, which is NOT "no": the
 * organization's own room cannot be archived, so silence keeps the control away.
 */
export function canArchiveRoom(input: {
  readonly role: string
  readonly kind: string
  readonly isOrganizationRoom: boolean | undefined
  readonly featureOn: boolean
}): boolean {
  return (
    input.featureOn
    && (input.role === 'owner' || input.role === 'manager')
    && input.kind === 'room'
    && input.isOrganizationRoom === false
  )
}

/** The copy for a refused archive call. Codes without their own words fall to the shared translator. */
export function archiveErrorCopy(
  err: unknown,
  tr: (no: string, en: string) => string,
  action: 'archive' | 'unarchive',
): string {
  const code = (err as { code?: unknown } | null)?.code
  const raw = (err as { details?: unknown } | null)?.details
  const details = raw && typeof raw === 'object' && !Array.isArray(raw) ? (raw as Readonly<Record<string, unknown>>) : null
  if (code === 'rate_limited') {
    const seconds = details?.retry_after_seconds
    return typeof seconds === 'number' && Number.isFinite(seconds) && seconds > 0
      ? tr(
          `For mange endringer på kort tid. Prøv igjen om ${Math.ceil(seconds)} sekunder.`,
          `Too many changes in a short time. Try again in ${Math.ceil(seconds)} seconds.`,
        )
      : tr('For mange endringer på kort tid. Prøv igjen om litt.', 'Too many changes in a short time. Try again shortly.')
  }
  if (code === 'room_admin_unavailable') {
    return tr(
      'Rommet kunne ikke endres akkurat nå. Ingenting er endret. Prøv igjen om litt.',
      'The room could not be changed right now. Nothing was changed. Try again shortly.',
    )
  }
  if (code === 'room_not_archivable') {
    return tr(
      'Organisasjonens eget rom kan ikke arkiveres.',
      "The organization's own room cannot be archived.",
    )
  }
  if (code === 'room_state_conflict') {
    return tr(
      'Rommet ble endret av noen andre. Last inn på nytt og prøv igjen.',
      'The room was changed by someone else. Reload and try again.',
    )
  }
  return translateApiError(err, tr, action === 'archive'
    ? { no: 'Rommet kunne ikke arkiveres. Ingenting er endret.', en: 'The room could not be archived. Nothing was changed.' }
    : { no: 'Arkiveringen kunne ikke fjernes. Ingenting er endret.', en: 'The room could not be unarchived. Nothing was changed.' })
}

/** One archive or unarchive call with its busy and error state, shared by the details panel and the banner. */
function createArchiveAction(props: { readonly spaceRef: string; readonly onChanged?: () => void }) {
  const i18n = useI18n()
  const [busy, setBusy] = createSignal(false)
  const [error, setError] = createSignal('')
  // A plain flag beside the signal: a signal write is applied at the next flush, so two clicks in
  // the same tick would both read `busy()` as false.
  let inFlight = false

  async function run(action: 'archive' | 'unarchive') {
    if (inFlight) return
    inFlight = true
    setBusy(true)
    setError('')
    try {
      // A fresh key per click: a retry after a failure is a new intent, and the key only has to
      // make one click safe against a duplicated request.
      const options = { idempotencyKey: `space-${action}:${crypto.randomUUID()}` }
      if (action === 'archive') await archiveSpaceRoom(props.spaceRef, options)
      else await unarchiveSpaceRoom(props.spaceRef, options)
      props.onChanged?.()
    } catch (err) {
      setError(archiveErrorCopy(err, i18n.tr, action))
    } finally {
      inFlight = false
      setBusy(false)
    }
  }

  return { busy, error, run }
}

export interface SpaceRoomArchiveControlProps {
  readonly spaceRef: string
  readonly archived: boolean
  /** Called after the server accepted the change, so the page can re-read the room. */
  readonly onChanged?: () => void
}

/**
 * Archive or unarchive, inside the room details. Archiving asks in place first (the room stops
 * taking new messages for everyone); bringing a room back is harmless and takes one click.
 * Rendered only when {@link canArchiveRoom} says so.
 */
export function SpaceRoomArchiveControl(props: SpaceRoomArchiveControlProps) {
  const i18n = useI18n()
  const action = createArchiveAction(props)

  return (
    <div class="verevon-space-room-archive">
      <Show
        when={props.archived}
        fallback={
          <SpaceConfirmButton
            class="verevon-space-secondary-action verevon-space-room-archive__button verevon-space-room-archive__button--danger"
            label={i18n.tr('Arkiver rommet', 'Archive room')}
            confirmLabel={i18n.tr('Bekreft arkivering av rommet', 'Confirm archiving the room')}
            consequence={i18n.tr(
              'Rommet blir skrivebeskyttet for alle. Du kan fjerne arkiveringen senere.',
              'The room becomes read-only for everyone. You can unarchive it later.',
            )}
            disabled={action.busy()}
            onConfirm={() => void action.run('archive')}
          />
        }
      >
        <button
          type="button"
          class="verevon-space-secondary-action verevon-space-room-archive__button"
          disabled={action.busy()}
          onClick={() => void action.run('unarchive')}
        >
          {action.busy() ? i18n.tr('Fjerner arkivering …', 'Unarchiving…') : i18n.tr('Fjern arkivering', 'Unarchive room')}
        </button>
      </Show>
      <Show when={action.error()}>
        {(message) => <p class="verevon-space-projection-error" role="alert">{message()}</p>}
      </Show>
    </div>
  )
}

export interface SpaceArchivedBannerProps {
  readonly spaceRef: string
  /** Owner or manager of a room that can be archived: only they are offered the way back. */
  readonly canUnarchive: boolean
  readonly onChanged?: () => void
}

/**
 * Says plainly that the room is archived. Reads still work, so the banner sits above the
 * timeline rather than replacing it. The composer is replaced separately, by
 * {@link SpaceArchivedNotice}.
 */
export function SpaceArchivedBanner(props: SpaceArchivedBannerProps) {
  const i18n = useI18n()
  const action = createArchiveAction(props)

  return (
    <div class="verevon-space-archived-banner" role="status">
      <Lock size={15} aria-hidden="true" />
      <p class="verevon-space-archived-banner__text">
        <strong>{i18n.tr('Rommet er arkivert', 'This room is archived')}</strong>
        <span>{i18n.tr(
          'Du kan lese alt, men ingen kan skrive nye meldinger før arkiveringen fjernes.',
          'You can read everything, but nobody can write new messages until it is unarchived.',
        )}</span>
      </p>
      <Show when={props.canUnarchive}>
        <button
          type="button"
          class="verevon-space-secondary-action"
          disabled={action.busy()}
          onClick={() => void action.run('unarchive')}
        >
          {action.busy() ? i18n.tr('Fjerner arkivering …', 'Unarchiving…') : i18n.tr('Fjern arkivering', 'Unarchive room')}
        </button>
      </Show>
      <Show when={action.error()}>
        {(message) => <p class="verevon-space-archived-banner__error" role="alert">{message()}</p>}
      </Show>
    </div>
  )
}

/** Stands where the composer would be: an archived room takes no new messages. */
export function SpaceArchivedNotice() {
  const i18n = useI18n()
  return (
    <p class="verevon-space-archived-notice">
      <Lock size={14} aria-hidden="true" />
      {i18n.tr(
        'Rommet er arkivert og skrivebeskyttet. Meldinger kan ikke sendes.',
        'This room is archived and read-only. Messages cannot be sent.',
      )}
    </p>
  )
}
