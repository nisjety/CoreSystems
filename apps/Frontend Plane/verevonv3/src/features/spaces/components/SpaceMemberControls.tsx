import { Loader2, Users } from '@/shared/icons'
import { createMemo, createSignal, For, Show } from 'solid-js'

import { SpaceConfirmButton } from './SpaceConfirmButton'

import { listOrganizationMembers } from '@/shared/api/organization-client'
import {
  addSpaceMember,
  changeSpaceMemberRole,
  leaveSpaceRoom,
  removeSpaceMember,
  type SpaceAudienceState,
  type SpaceRosterMember,
} from '@/shared/api/spaces-client'
import { translateApiError, useI18n } from '@/shared/i18n'
import { createResource } from '@/shared/lib/create-resource-compat'

import {
  assignableRolesForAdd,
  assignableRolesForChange,
  canRemove,
  isRoomRole,
  type RoomRole,
} from '../lib/space-roles'

const ROLE_LABELS: Record<RoomRole, { no: string; en: string }> = {
  viewer: { no: 'Leser', en: 'Viewer' },
  editor: { no: 'Redaktør', en: 'Editor' },
  manager: { no: 'Leder', en: 'Manager' },
  owner: { no: 'Eier', en: 'Owner' },
}

function roleLabel(role: string, tr: (no: string, en: string) => string): string {
  return isRoomRole(role) ? tr(ROLE_LABELS[role].no, ROLE_LABELS[role].en) : role
}

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
  /**
   * The caller's own room role, and whether the gateway's Control member writer is on. With the
   * writer on, a person is added with a chosen role; without it the legacy add (no role) is
   * used. `undefined` means the server did not say, which is NOT the same as "on".
   */
  readonly actorRole?: string
  readonly writerApi?: boolean
}

export function SpaceMemberControls(props: SpaceMemberControlsProps) {
  const i18n = useI18n()
  const [open, setOpen] = createSignal(false)
  const [busyId, setBusyId] = createSignal<string | undefined>(undefined)
  const [actionError, setActionError] = createSignal('')
  const [audience, setAudience] = createSignal<SpaceAudienceState | undefined>(undefined)
  const [addRole, setAddRole] = createSignal<RoomRole>('editor')
  const addRoles = createMemo(() => (props.writerApi ? assignableRolesForAdd(props.actorRole ?? '') : []))
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

  async function run(
    id: string,
    change: () => Promise<{ audience?: SpaceAudienceState } | unknown>,
    failure: { no: string; en: string },
  ) {
    if (busyId()) return
    setBusyId(id)
    setActionError('')
    setAudience(undefined)
    try {
      const result = await change()
      setAudience((result as { audience?: SpaceAudienceState } | undefined)?.audience)
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

      {/* The Control write already stands; this only says whether the new person can read yet.
          `pending` is honest ("being prepared"), `unconfirmed` is a warning that heals itself. */}
      <Show when={audience() === 'pending'}>
        <p class="verevon-space-inline-status" role="status">
          {i18n.tr(
            'Tilgangen forberedes. Personen kan lese rommet om et øyeblikk.',
            'Access is being prepared. The person can read the room in a moment.',
          )}
        </p>
      </Show>
      <Show when={audience() === 'unconfirmed'}>
        <p class="verevon-space-projection-error" role="status">
          {i18n.tr(
            'Endringen er lagret, men tilgangen er ikke bekreftet ennå. Den ordner seg automatisk.',
            'The change is saved, but access is not confirmed yet. It will settle automatically.',
          )}
        </p>
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

          <Show when={addRoles().length > 0}>
            <label class="verevon-space-member-picker__role">
              <span>{i18n.tr('Rolle for nye medlemmer', 'Role for new members')}</span>
              <select
                class="verevon-space-agent__control"
                value={addRole()}
                onChange={(event) => {
                  const next = event.currentTarget.value
                  if (isRoomRole(next)) setAddRole(next)
                }}
              >
                <For each={addRoles()}>
                  {(role) => <option value={role}>{roleLabel(role, i18n.tr)}</option>}
                </For>
              </select>
            </label>
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
                        () =>
                          addSpaceMember(
                            props.spaceRef,
                            member.userId,
                            // Only with the writer on, and only a role this caller may grant:
                            // otherwise the legacy add (no role) is unchanged.
                            addRoles().includes(addRole()) ? addRole() : undefined,
                          ),
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

/**
 * The role picker for one roster row (WS04-S6, Control writer on).
 *
 * Offers only the roles the caller may set on this member (the Control role matrix, mirrored in
 * `space-roles.ts`); Control re-decides every call, so this only hides doors that would not
 * open. The change carries the row revision the roster showed: a stale one answers
 * `room_member_changed` and the roster is re-read.
 */
export function SpaceMemberRoleSelect(props: {
  readonly spaceRef: string
  readonly member: SpaceRosterMember
  readonly actorId: string
  readonly actorRole: string
  readonly activeOwners: number
  readonly onChanged?: () => void
}) {
  const i18n = useI18n()
  const [busy, setBusy] = createSignal(false)
  const [failed, setFailed] = createSignal('')
  const isSelf = () => props.member.subject_id === props.actorId
  const roles = createMemo(() =>
    assignableRolesForChange(props.actorRole, props.member.role, isSelf(), props.activeOwners),
  )

  const change = async (role: RoomRole) => {
    if (busy() || role === props.member.role) return
    setBusy(true)
    setFailed('')
    try {
      await changeSpaceMemberRole(props.spaceRef, props.member.subject_id, role, props.member.revision)
      props.onChanged?.()
    } catch (err) {
      setFailed(translateApiError(err, i18n.tr, {
        no: 'Rollen kunne ikke endres. Ingenting er endret.',
        en: 'The role could not be changed. Nothing was changed.',
      }))
      // A stale revision or a refused change both mean the roster on screen is not the truth.
      props.onChanged?.()
    } finally {
      setBusy(false)
    }
  }

  return (
    <>
      <select
        class="verevon-space-agent__control"
        aria-label={i18n.tr(
          `Rolle for ${props.member.display_name?.trim() || props.member.subject_id}`,
          `Role for ${props.member.display_name?.trim() || props.member.subject_id}`,
        )}
        disabled={busy() || roles().length <= 1}
        value={props.member.role}
        onChange={(event) => {
          const next = event.currentTarget.value
          if (isRoomRole(next)) void change(next)
        }}
      >
        <For each={roles()}>
          {(role) => <option value={role}>{roleLabel(role, i18n.tr)}</option>}
        </For>
      </select>
      <Show when={failed()}>
        {(message) => (
          <span class="verevon-space-agent__error" role="alert">{message()}</span>
        )}
      </Show>
    </>
  )
}

/**
 * Leave this room (WS04-S6, Control writer on). Control refuses the last owner
 * (`room_sole_owner`) and the organization room; the button is not offered when the matrix would
 * refuse it here, and `onLeft` runs only after Control confirmed.
 */
export function SpaceLeaveRoomButton(props: {
  readonly spaceRef: string
  readonly actorRole: string
  readonly activeOwners: number
  readonly onLeft: () => void
}) {
  const i18n = useI18n()
  const [busy, setBusy] = createSignal(false)
  const [failed, setFailed] = createSignal('')
  const allowed = () => canRemove(props.actorRole, props.actorRole, true, props.activeOwners) === null

  const leave = async () => {
    if (busy()) return
    setBusy(true)
    setFailed('')
    try {
      const result = await leaveSpaceRoom(props.spaceRef)
      if (result.left) props.onLeft()
      else setFailed(i18n.tr('Du er fortsatt med i rommet.', 'You are still in the room.'))
    } catch (err) {
      setFailed(translateApiError(err, i18n.tr, {
        no: 'Du kunne ikke forlate rommet. Du er fortsatt medlem.',
        en: 'You could not leave the room. You are still a member.',
      }))
    } finally {
      setBusy(false)
    }
  }

  return (
    <Show when={allowed()}>
      <div class="verevon-space-member-controls__actions">
        <SpaceConfirmButton
          class="verevon-space-agent__control verevon-space-agent__control--remove"
          label={i18n.tr('Forlat rommet', 'Leave room')}
          confirmLabel={i18n.tr('Bekreft at du forlater rommet', 'Confirm leaving the room')}
          consequence={i18n.tr(
            'Du mister tilgang til det som er sagt i rommet.',
            'You lose access to what has been said in the room.',
          )}
          disabled={busy()}
          onConfirm={() => void leave()}
        />
        <Show when={failed()}>
          {(message) => (
            <span class="verevon-space-agent__error" role="alert">{message()}</span>
          )}
        </Show>
      </div>
    </Show>
  )
}
