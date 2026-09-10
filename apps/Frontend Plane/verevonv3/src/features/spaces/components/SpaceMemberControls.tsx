import { Loader2, Users } from '@/shared/icons'
import { createMemo, createSignal, For, Show } from 'solid-js'

import { SpaceConfirmButton } from './SpaceConfirmButton'

import { listOrganizationMembers } from '@/shared/api/organization-client'
import { addSpaceMember, removeSpaceMember, type SpaceRosterMember } from '@/shared/api/spaces-client'
import { translateApiError, useI18n } from '@/shared/i18n'
import { createResource } from '@/shared/lib/create-resource-compat'

/**
 * Who is in a named room, and the controls to change it.
 *
 * Only for a room somebody made. The organization room is deliberately excluded
 * because its roster is derived — being in the organization is what puts you in
 * its channel — and a manual list beside a derived one would be overwritten on
 * the next sync while appearing to work. A personal Space is excluded because
 * Control refuses to replace its membership at all.
 *
 * The picker offers only people already in the organization. A room grant is
 * not a way into the tenant, and the server refuses otherwise; showing anyone
 * else would be offering a door that does not open.
 */
export interface SpaceMemberControlsProps {
  readonly spaceRef: string
  /** Control's current roster, so the picker can exclude who is already here. */
  readonly roster: () => readonly SpaceRosterMember[]
  /** The organization whose people may be added. */
  readonly orgId: string | undefined
  /** Called after a change settles, so the roster is re-read from Control. */
  readonly onChanged?: () => void
}

export function SpaceMemberControls(props: SpaceMemberControlsProps) {
  const i18n = useI18n()
  const [open, setOpen] = createSignal(false)
  const [busyId, setBusyId] = createSignal<string | undefined>(undefined)
  const [actionError, setActionError] = createSignal('')
  const [candidates] = createResource(
    () => (open() ? props.orgId ?? '' : ''),
    async (orgId) => (orgId ? listOrganizationMembers(orgId) : []),
  )

  // Control's roster is the source for "already here", not our own optimism:
  // a grant that Control has not accepted must keep the person in the picker.
  const alreadyHere = createMemo(
    () => new Set(props.roster().filter((m) => m.subject_type === 'user').map((m) => m.subject_id)),
  )
  const addable = createMemo(() =>
    (candidates.error ? [] : candidates() ?? []).filter((member) => !alreadyHere().has(member.userId)),
  )

  async function run(id: string, change: () => Promise<unknown>, failure: { no: string; en: string }) {
    if (busyId()) return
    setBusyId(id)
    setActionError('')
    try {
      await change()
      props.onChanged?.()
    } catch (err) {
      setActionError(translateApiError(err, i18n.tr, failure))
    } finally {
      setBusyId(undefined)
    }
  }

  return (
    <section class="verevon-space-member-controls">
      <div class="verevon-space-member-controls__actions">
        <button
          type="button"
          class="verevon-space-secondary-action"
          onClick={() => setOpen((value) => !value)}
        >
          <Users size={15} aria-hidden="true" />
          {open() ? i18n.tr('Lukk', 'Close') : i18n.tr('Legg til personer', 'Add people')}
        </button>
      </div>

      <Show when={actionError()}>
        {(message) => (
          <p class="verevon-space-projection-error" role="alert">{message()}</p>
        )}
      </Show>

      <Show when={open()}>
        <div class="verevon-space-member-picker">
          <Show when={candidates.loading}>
            <p class="verevon-space-inline-status" role="status">
              {i18n.tr('Henter personer …', 'Loading people…')}
            </p>
          </Show>

          {/* An unreadable org roster is not an empty organization. */}
          <Show when={candidates.error}>
            <p class="verevon-space-projection-error" role="alert">
              {i18n.tr(
                'Vi fikk ikke hentet hvem som er i organisasjonen.',
                'We could not load who is in the organization.',
              )}
            </p>
          </Show>

          <Show when={!candidates.loading && !candidates.error && addable().length === 0}>
            <p class="verevon-space-member-picker__empty">
              {i18n.tr(
                'Alle i organisasjonen er allerede med i dette rommet.',
                'Everyone in the organization is already in this room.',
              )}
            </p>
          </Show>

          <ul class="verevon-space-member-picker__list">
            <For each={addable()}>
              {(member) => (
                <li>
                  <span class="verevon-space-member-picker__who">
                    <strong>{member.name || member.email}</strong>
                    <Show when={member.name && member.email}>
                      <small>{member.email}</small>
                    </Show>
                  </span>
                  <button
                    type="button"
                    class="verevon-space-agent__control"
                    disabled={busyId() !== undefined}
                    onClick={() =>
                      void run(
                        member.userId,
                        () => addSpaceMember(props.spaceRef, member.userId),
                        {
                          no: 'Personen kunne ikke legges til. Ingenting er endret.',
                          en: 'That person could not be added. Nothing was changed.',
                        },
                      )
                    }
                  >
                    <Show when={busyId() === member.userId} fallback={i18n.tr('Legg til', 'Add')}>
                      <Loader2 size={14} aria-hidden="true" />
                      {i18n.tr('Legger til …', 'Adding…')}
                    </Show>
                  </button>
                </li>
              )}
            </For>
          </ul>
        </div>
      </Show>

    </section>
  )
}

/**
 * The removal control for one roster row.
 *
 * Deliberately rendered into the roster the Members tab already draws rather
 * than beside a second copy of it — one list of who is here, with the control
 * on the row it acts on.
 */
export function SpaceMemberRemoveButton(props: {
  readonly spaceRef: string
  readonly member: SpaceRosterMember
  readonly onChanged?: () => void
}) {
  const i18n = useI18n()
  const [busy, setBusy] = createSignal(false)
  const [failed, setFailed] = createSignal('')

  const remove = async () => {
    if (busy()) return
    setBusy(true)
    setFailed('')
    try {
      await removeSpaceMember(props.spaceRef, props.member.subject_id)
      props.onChanged?.()
    } catch (err) {
      setFailed(translateApiError(err, i18n.tr, {
        no: 'Personen kunne ikke fjernes. De er fortsatt medlem.',
        en: 'That person could not be removed. They are still a member.',
      }))
    } finally {
      setBusy(false)
    }
  }

  return (
    <>
      <SpaceConfirmButton
        class="verevon-space-agent__control verevon-space-agent__control--remove"
        label={i18n.tr('Fjern', 'Remove')}
        confirmLabel={i18n.tr(
          `Bekreft at ${props.member.display_name?.trim() || props.member.subject_id} fjernes`,
          `Confirm removing ${props.member.display_name?.trim() || props.member.subject_id}`,
        )}
        consequence={i18n.tr(
          'De mister tilgang til det som er sagt i rommet.',
          'They lose access to what has been said in the room.',
        )}
        disabled={busy()}
        onConfirm={() => void remove()}
      />
      <Show when={failed()}>
        {(message) => (
          <span class="verevon-space-agent__error" role="alert">{message()}</span>
        )}
      </Show>
    </>
  )
}
