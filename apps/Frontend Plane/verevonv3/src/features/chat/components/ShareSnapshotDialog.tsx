import { createEffect, createMemo, createSignal, createUniqueId, For, onCleanup, Show } from 'solid-js'
import { Portal } from '@solidjs/web'
import { createResource } from '@/shared/lib/create-resource-compat'
import {
  chatSnapshotsEnabled,
  createChatSnapshot,
  grantChatSnapshot,
  listChatSnapshotGrants,
  listChatSnapshots,
  removeChatSnapshotGrant,
  revokeChatSnapshot,
  type ChatSnapshot,
} from '@/shared/api/snapshot-client'
import { listOrganizationMembers } from '@/shared/api/organization-client'
import { ApiError } from '@/shared/api/http'
import { isZdrCode, zdrRefusalPresentation, type Translate } from '@/shared/api/zdr-refusals'
import { translateApiError, useI18n } from '@/shared/i18n'
import { getSession } from '@/shared/session/session-store'
import { isLocalRetentionAllowed, subscribeLocalRetention } from '@/features/chat/lib/chat-retention'
import { Copy, Trash2, Users, X } from '@/shared/icons'
import './ShareSnapshotDialog.css'

/** Whether this build offers conversation snapshots (`VITE_CHAT_SNAPSHOTS`, read with the strict literal `true`). */
export const shareSnapshotEnabled = () => chatSnapshotsEnabled()

/**
 * "Share a snapshot" beside the export on the latest answer (W4-SH-6, F-t3-046). The person takes an immutable,
 * redacted copy of the conversation, picks members of their own organisation to see it, sees who can, and revokes
 * a member or the whole snapshot. There is no public link: the address it shows opens only for the owner and for
 * the members picked here. The control stays out of sight for a temporary chat, a Space thread and a zero-retention
 * caller, and goes away once the deployment answers that snapshots are off, as the export does. The gateway and
 * Session Core decide every one of these; this only asks.
 */
export function ShareSnapshotDialog(props: {
  threadId: string
  temporaryChat?: boolean
  spaceScoped?: boolean
}) {
  const i18n = useI18n()
  const titleId = createUniqueId()
  const session = getSession()
  const orgId = createMemo(() => session.activeOrg?.id?.trim() ?? '')
  const ownUserId = createMemo(() => session.user?.id?.trim() ?? '')
  // The zero-retention posture of this caller can change while the page is open, so the control follows it.
  const [retentionAllowed, setRetentionAllowed] = createSignal(isLocalRetentionAllowed())
  onCleanup(subscribeLocalRetention((allowed) => setRetentionAllowed(allowed)))
  const [off, setOff] = createSignal(false)
  const [open, setOpen] = createSignal(false)
  const [selectedId, setSelectedId] = createSignal<string | null>(null)
  const [confirmRevokeId, setConfirmRevokeId] = createSignal<string | null>(null)
  const [pickedMember, setPickedMember] = createSignal('')
  const [busy, setBusy] = createSignal(false)
  const [refusal, setRefusal] = createSignal<string | null>(null)
  const [copiedId, setCopiedId] = createSignal<string | null>(null)

  const hidden = () =>
    off() || !props.threadId || Boolean(props.temporaryChat) || Boolean(props.spaceScoped) || !retentionAllowed()

  const refused = (error: unknown, fallback: { no: string; en: string }) => {
    setRefusal(refusalText(error, i18n.tr, fallback))
    if (codeOf(error) === 'chat_snapshots_disabled') setOff(true)
  }

  // The snapshots, the members and the grants load only while the dialog is open.
  const [snapshots, { refetch: refetchSnapshots }] = createResource(
    () => (open() && !hidden() ? props.threadId : undefined),
    async (threadId) => {
      try {
        return await listChatSnapshots(threadId)
      } catch (error) {
        if (codeOf(error) === 'chat_snapshots_disabled') setOff(true)
        throw error
      }
    },
  )
  const [members] = createResource(
    () => (open() && !hidden() && orgId() ? orgId() : undefined),
    (organizationId) => listOrganizationMembers(organizationId),
  )
  const [grants, { refetch: refetchGrants }] = createResource(
    () => (open() && !hidden() ? selectedId() ?? undefined : undefined),
    (snapshotId) => listChatSnapshotGrants(snapshotId),
  )

  const liveSnapshots = (): ChatSnapshot[] => (snapshots() ?? []).filter((snapshot) => snapshot.revokedAt === null)
  const selected = () => liveSnapshots().find((snapshot) => snapshot.id === selectedId())
  const grantedIds = () => new Set((grants() ?? []).map((grant) => grant.granteeUserId))
  // Everyone offered is an active member of this organisation other than the owner and those already shared with.
  const pickable = () =>
    (members() ?? []).filter((member) => member.userId !== ownUserId() && !grantedIds().has(member.userId))
  const labelOf = (userId: string) => {
    const member = (members() ?? []).find((entry) => entry.userId === userId)
    return member ? member.name || member.email || userId : userId
  }
  const locale = () => (i18n.locale() === 'no' ? 'nb-NO' : 'en-US')
  const stampOf = (iso: string) => {
    const date = new Date(iso)
    return Number.isNaN(date.getTime()) ? '' : date.toLocaleString(locale(), { dateStyle: 'medium', timeStyle: 'short' })
  }

  // Escape closes the dialog while it is open.
  createEffect(
    () => open(),
    (isOpen) => {
      if (!isOpen) return
      const onKey = (event: KeyboardEvent) => {
        if (event.key === 'Escape') close()
      }
      document.addEventListener('keydown', onKey)
      return () => document.removeEventListener('keydown', onKey)
    },
  )

  // With one live snapshot there is nothing to choose, so it is the one the member list works on.
  createEffect(
    () => liveSnapshots().map((snapshot) => snapshot.id).join('|'),
    () => {
      const live = liveSnapshots()
      if (live.length === 0) setSelectedId(null)
      else if (!live.some((snapshot) => snapshot.id === selectedId())) setSelectedId(live[0]!.id)
    },
  )

  function close() {
    setOpen(false)
    setConfirmRevokeId(null)
    setRefusal(null)
    setPickedMember('')
  }

  async function takeSnapshot() {
    if (busy()) return
    setBusy(true)
    setRefusal(null)
    try {
      const created = await createChatSnapshot(props.threadId)
      await refetchSnapshots()
      setSelectedId(created.id)
    } catch (error) {
      refused(error, { no: 'Øyeblikksbildet kunne ikke lages.', en: 'The snapshot could not be taken.' })
    } finally {
      setBusy(false)
    }
  }

  async function shareWithPicked() {
    const snapshotId = selectedId()
    const member = pickedMember()
    if (!snapshotId || !member || busy()) return
    setBusy(true)
    setRefusal(null)
    try {
      await grantChatSnapshot(snapshotId, member)
      setPickedMember('')
      await refetchGrants()
    } catch (error) {
      refused(error, { no: 'Øyeblikksbildet kunne ikke deles.', en: 'The snapshot could not be shared.' })
    } finally {
      setBusy(false)
    }
  }

  async function stopSharing(granteeUserId: string) {
    const snapshotId = selectedId()
    if (!snapshotId || busy()) return
    setBusy(true)
    setRefusal(null)
    try {
      await removeChatSnapshotGrant(snapshotId, granteeUserId)
      await refetchGrants()
    } catch (error) {
      refused(error, { no: 'Delingen kunne ikke fjernes.', en: 'The share could not be removed.' })
    } finally {
      setBusy(false)
    }
  }

  async function revoke(snapshotId: string) {
    if (busy()) return
    setBusy(true)
    setRefusal(null)
    try {
      await revokeChatSnapshot(snapshotId)
      setConfirmRevokeId(null)
      if (selectedId() === snapshotId) setSelectedId(null)
      await refetchSnapshots()
    } catch (error) {
      // A revoke that deleted the copy but could not remove every share leaves the snapshot unreadable; the list
      // is read again so the row goes, and the message says to revoke again to finish the clean-up.
      refused(error, { no: 'Øyeblikksbildet kunne ikke trekkes tilbake.', en: 'The snapshot could not be revoked.' })
      setConfirmRevokeId(null)
      await refetchSnapshots().catch(() => undefined)
    } finally {
      setBusy(false)
    }
  }

  async function copyAddress(snapshotId: string) {
    try {
      await navigator.clipboard.writeText(`${window.location.origin}/chat/shared/${encodeURIComponent(snapshotId)}`)
      setCopiedId(snapshotId)
    } catch {
      setCopiedId(null)
    }
  }

  return (
    <Show when={!hidden()}>
      <div class="verevon-chat-snapshot">
        <button
          type="button"
          class="verevon-chat-snapshot__trigger"
          aria-haspopup="dialog"
          onClick={() => setOpen(true)}
        >
          <Users size={14} />
          {i18n.tr('Del som øyeblikksbilde', 'Share a snapshot')}
        </button>
        <Show when={open()}>
          <Portal>
            <div class="verevon-chat-snapshot-backdrop">
              <section
                class="verevon-chat-snapshot-dialog"
                role="dialog"
                aria-modal="true"
                aria-labelledby={titleId}
                tabindex={-1}
              >
                <header class="verevon-chat-snapshot-dialog__header">
                  <div>
                    <h2 id={titleId}>{i18n.tr('Del et øyeblikksbilde', 'Share a snapshot')}</h2>
                    <p>
                      {i18n.tr(
                        'Et øyeblikksbilde er en redigert kopi av samtalen slik den er nå. Bare du og medlemmene du velger i organisasjonen din kan åpne den. Det finnes ingen offentlig lenke.',
                        'A snapshot is a redacted copy of the conversation as it is now. Only you and the members of your organization you pick can open it. There is no public link.',
                      )}
                    </p>
                  </div>
                  <button
                    type="button"
                    class="verevon-chat-snapshot-dialog__close"
                    aria-label={i18n.tr('Lukk', 'Close')}
                    onClick={() => close()}
                  >
                    <X size={16} />
                  </button>
                </header>

                <section class="verevon-chat-snapshot-dialog__card">
                  <button
                    type="button"
                    class="verevon-chat-snapshot-dialog__action"
                    disabled={busy()}
                    onClick={() => void takeSnapshot()}
                  >
                    {busy() ? i18n.tr('Jobber …', 'Working …') : i18n.tr('Lag øyeblikksbilde', 'Take a snapshot')}
                  </button>
                  <Show when={snapshots.loading && snapshots() === undefined}>
                    <p class="verevon-chat-snapshot-dialog__muted">{i18n.tr('Henter …', 'Loading …')}</p>
                  </Show>
                  <Show when={snapshots.error}>
                    <p class="verevon-chat-snapshot-dialog__refusal" role="alert">
                      {refusalText(snapshots.error, i18n.tr, {
                        no: 'Øyeblikksbildene kunne ikke hentes.',
                        en: 'The snapshots could not be loaded.',
                      })}
                    </p>
                  </Show>
                  <Show
                    when={liveSnapshots().length > 0}
                    fallback={
                      <Show when={!snapshots.loading && !snapshots.error}>
                        <p class="verevon-chat-snapshot-dialog__muted">
                          {i18n.tr('Ingen øyeblikksbilder av denne samtalen ennå.', 'No snapshots of this conversation yet.')}
                        </p>
                      </Show>
                    }
                  >
                    <ul class="verevon-chat-snapshot-dialog__list" aria-label={i18n.tr('Øyeblikksbilder', 'Snapshots')}>
                      <For each={liveSnapshots()}>
                        {(snapshot) => (
                          <li class="verevon-chat-snapshot-dialog__row">
                            <label class="verevon-chat-snapshot-dialog__pick">
                              <input
                                type="radio"
                                name={`${titleId}-snapshot`}
                                checked={selectedId() === snapshot.id}
                                onChange={() => {
                                  setSelectedId(snapshot.id)
                                  setConfirmRevokeId(null)
                                }}
                              />
                              <span>{stampOf(snapshot.createdAt) || snapshot.id}</span>
                            </label>
                            <button
                              type="button"
                              class="verevon-chat-snapshot-dialog__icon"
                              aria-label={i18n.tr('Kopier adressen', 'Copy the address')}
                              title={
                                copiedId() === snapshot.id
                                  ? i18n.tr('Kopiert', 'Copied')
                                  : i18n.tr('Adressen åpnes bare av eieren og de du deler med', 'The address opens only for you and those you share with')
                              }
                              onClick={() => void copyAddress(snapshot.id)}
                            >
                              <Copy size={14} />
                            </button>
                            <Show
                              when={confirmRevokeId() === snapshot.id}
                              fallback={
                                <button
                                  type="button"
                                  class="verevon-chat-snapshot-dialog__icon"
                                  aria-label={i18n.tr('Trekk tilbake øyeblikksbildet', 'Revoke the snapshot')}
                                  disabled={busy()}
                                  onClick={() => setConfirmRevokeId(snapshot.id)}
                                >
                                  <Trash2 size={14} />
                                </button>
                              }
                            >
                              <span class="verevon-chat-snapshot-dialog__confirm">
                                <span>
                                  {i18n.tr(
                                    'Kopien og alle delinger slettes.',
                                    'The copy and every share are deleted.',
                                  )}
                                </span>
                                <button
                                  type="button"
                                  class="verevon-chat-snapshot-dialog__action"
                                  disabled={busy()}
                                  onClick={() => void revoke(snapshot.id)}
                                >
                                  {i18n.tr('Trekk tilbake', 'Revoke')}
                                </button>
                                <button
                                  type="button"
                                  class="verevon-chat-snapshot-dialog__action"
                                  disabled={busy()}
                                  onClick={() => setConfirmRevokeId(null)}
                                >
                                  {i18n.tr('Avbryt', 'Cancel')}
                                </button>
                              </span>
                            </Show>
                          </li>
                        )}
                      </For>
                    </ul>
                  </Show>
                </section>

                <Show when={selected()}>
                  <section class="verevon-chat-snapshot-dialog__card">
                    <h3>{i18n.tr('Delt med', 'Shared with')}</h3>
                    <Show when={grants.error}>
                      <p class="verevon-chat-snapshot-dialog__refusal" role="alert">
                        {refusalText(grants.error, i18n.tr, {
                          no: 'Delingene kunne ikke hentes.',
                          en: 'The shares could not be loaded.',
                        })}
                      </p>
                    </Show>
                    <Show
                      when={(grants() ?? []).length > 0}
                      fallback={
                        <p class="verevon-chat-snapshot-dialog__muted">
                          {i18n.tr('Ikke delt med noen ennå.', 'Not shared with anyone yet.')}
                        </p>
                      }
                    >
                      <ul class="verevon-chat-snapshot-dialog__list" aria-label={i18n.tr('Medlemmer med tilgang', 'Members with access')}>
                        <For each={grants() ?? []}>
                          {(grant) => (
                            <li class="verevon-chat-snapshot-dialog__row">
                              <span class="verevon-chat-snapshot-dialog__who">{labelOf(grant.granteeUserId)}</span>
                              <span class="verevon-chat-snapshot-dialog__muted">{i18n.tr('Kan se', 'Can view')}</span>
                              <button
                                type="button"
                                class="verevon-chat-snapshot-dialog__action"
                                disabled={busy()}
                                onClick={() => void stopSharing(grant.granteeUserId)}
                              >
                                {i18n.tr('Fjern', 'Remove')}
                              </button>
                            </li>
                          )}
                        </For>
                      </ul>
                    </Show>
                    <div class="verevon-chat-snapshot-dialog__add">
                      <label class="verevon-chat-snapshot-dialog__field">
                        <span>{i18n.tr('Del med medlem', 'Share with a member')}</span>
                        <select
                          value={pickedMember()}
                          disabled={busy() || members.loading}
                          onChange={(event) => setPickedMember(event.currentTarget.value)}
                        >
                          <option value="">{i18n.tr('Velg et medlem', 'Choose a member')}</option>
                          <For each={pickable()}>
                            {(member) => (
                              <option value={member.userId}>{member.name || member.email || member.userId}</option>
                            )}
                          </For>
                        </select>
                      </label>
                      <button
                        type="button"
                        class="verevon-chat-snapshot-dialog__action"
                        disabled={busy() || !pickedMember()}
                        onClick={() => void shareWithPicked()}
                      >
                        {i18n.tr('Del', 'Share')}
                      </button>
                    </div>
                    <Show when={members.error}>
                      <p class="verevon-chat-snapshot-dialog__refusal" role="alert">
                        {i18n.tr('Medlemslisten kunne ikke hentes.', 'The member list could not be loaded.')}
                      </p>
                    </Show>
                  </section>
                </Show>

                <Show when={refusal()}>
                  {(message) => (
                    <p class="verevon-chat-snapshot-dialog__refusal" role="alert">
                      {message()}
                    </p>
                  )}
                </Show>
              </section>
            </div>
          </Portal>
        </Show>
      </div>
    </Show>
  )
}

/** The typed code of a refusal, or null for anything that is not one. */
function codeOf(error: unknown): string | null {
  return error instanceof ApiError ? error.code : null
}

/** The closed reason word a refusal names in `details`, when it has one. */
function reasonOf(error: unknown): string | null {
  if (!(error instanceof ApiError)) return null
  const details = error.details
  if (typeof details !== 'object' || details === null || Array.isArray(details)) return null
  const reason = (details as Record<string, unknown>).reason
  return typeof reason === 'string' ? reason : null
}

/**
 * What the person is told when a snapshot call is refused. A zero-retention refusal and the refusals of a
 * conversation or a grant are said in the reader's language; anything else gets the caller's fallback by code or
 * status class, never the server's English sentence.
 */
function refusalText(error: unknown, tr: Translate, fallback: { no: string; en: string }): string {
  const code = codeOf(error)
  if (code && isZdrCode(code)) {
    const presented = zdrRefusalPresentation(code, error instanceof ApiError ? (error.feature ?? undefined) : undefined, tr)
    return presented.next ? `${presented.message} ${presented.next}` : presented.message
  }
  switch (code) {
    case 'thread_not_exportable': {
      const reason = reasonOf(error)
      if (reason === 'space_thread') {
        return tr('En Space-samtale kan ikke deles som øyeblikksbilde.', 'A Space conversation cannot be shared as a snapshot.')
      }
      if (reason === 'non_persistent_run') {
        return tr(
          'En kjøring i samtalen ble ikke lagret, så samtalen kan ikke deles i sin helhet.',
          'A run in this conversation was not kept, so it cannot be shared in full.',
        )
      }
      return tr('Samtalen kan ikke deles i sin helhet.', 'This conversation cannot be shared in full.')
    }
    case 'snapshot_limit_reached':
      return tr(
        'Samtalen har allerede det høyeste antallet øyeblikksbilder. Trekk tilbake ett først.',
        'This conversation already has the most snapshots. Revoke one first.',
      )
    case 'snapshot_too_large':
      return tr('Samtalen er for stor til å deles som ett øyeblikksbilde.', 'The conversation is too large to share as one snapshot.')
    case 'grantee_not_in_organization':
      return tr('Du kan bare dele med medlemmer av organisasjonen din.', 'You can only share with members of your organization.')
    case 'snapshot_not_found':
    case 'thread_not_found':
      return tr('Fant ikke øyeblikksbildet.', 'The snapshot was not found.')
    case 'snapshot_grants_disabled':
      return tr('Deling med medlemmer er ikke slått på her.', 'Sharing with members is not turned on here.')
    case 'chat_snapshots_disabled':
    case 'thread_shares_disabled':
      return tr('Øyeblikksbilder er ikke slått på her.', 'Snapshots are not turned on here.')
    case 'snapshot_grants_cleanup_failed':
      return tr(
        'Øyeblikksbildet er trukket tilbake, men ikke alle delinger kunne fjernes. Trekk det tilbake på nytt for å fullføre.',
        'The snapshot was revoked, but not every share could be removed. Revoke it again to finish.',
      )
    case 'snapshot_grants_unavailable':
      return tr('Deling er utilgjengelig akkurat nå. Prøv igjen.', 'Sharing is unavailable right now. Try again.')
    case 'delegated_credential_required':
      return tr('Logg inn på nytt for å dele.', 'Sign in again to share.')
    default:
      return translateApiError(error, tr, fallback)
  }
}
