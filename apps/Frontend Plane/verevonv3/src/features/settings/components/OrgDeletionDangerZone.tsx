import { Loader2, ShieldAlert, Trash2 } from 'lucide-solid'
import { createSignal, Show } from 'solid-js'
import { SettingsButton } from '@/features/settings/components/settings-ui'
import { ApiError } from '@/shared/api/http'
import { triggerOrgSoftDelete } from '@/shared/api/org-deletion-client'
import { getSession } from '@/shared/session/session-store'

function errorMessage(error: unknown): string {
  return error instanceof ApiError || error instanceof Error ? error.message : 'Unexpected error'
}

function isOrgOwner(role: string | null | undefined): boolean {
  return (role ?? '').trim().toLowerCase() === 'owner'
}

/**
 * Danger Zone — organization deletion (Flow C). Owner-only: the org-core
 * `authorizeOrgErasure` gate is the authoritative check, but the control is
 * hidden entirely for non-owners here so admins/members never see a button
 * they cannot use.
 *
 * Destructive action is gated by typing the organization's exact name
 * (GitHub/Notion/Slack "type to confirm" pattern) rather than a checkbox —
 * mirrors PrivacyDataSection's typed-confirm erase flow for the same reason:
 * a single click must never be enough to trigger something irreversible.
 *
 * Unlike account erasure, this does not delete immediately: org-core opens a
 * 30-day recoverable grace window (`soft-delete`). The app-wide
 * OrgDeletionBanner picks up from here for every member (export/acknowledge)
 * and the owner (cancel).
 */
export function OrgDeletionDangerZone() {
  const session = getSession()
  const orgId = () => session.activeOrg?.id ?? null
  const orgName = () => session.activeOrg?.name ?? ''

  const [showConfirm, setShowConfirm] = createSignal(false)
  const [typedName, setTypedName] = createSignal('')
  const [deleting, setDeleting] = createSignal(false)
  const [error, setError] = createSignal<string | null>(null)
  const [scheduled, setScheduled] = createSignal(false)

  const nameMatches = () => orgName().trim().length > 0 && typedName().trim() === orgName().trim()
  const canDelete = () => nameMatches() && !deleting() && Boolean(orgId())

  function cancel() {
    setShowConfirm(false)
    setTypedName('')
    setError(null)
  }

  async function runDelete(event: Event) {
    event.preventDefault()
    const id = orgId()
    if (!canDelete() || !id) return
    setDeleting(true)
    setError(null)
    try {
      await triggerOrgSoftDelete(id, true, orgName())
      setScheduled(true)
      setShowConfirm(false)
      setTypedName('')
    } catch (reason) {
      setError(errorMessage(reason))
    } finally {
      setDeleting(false)
    }
  }

  return (
    <Show when={isOrgOwner(session.activeOrg?.role)}>
      <div id="org-danger-zone" class="verevon-privacy-danger">
        <div class="verevon-privacy-block__head">
          <h3>
            <ShieldAlert size={15} aria-hidden="true" /> Delete organization
          </h3>
          <Show when={!showConfirm() && !scheduled()}>
            <SettingsButton settingsSize="sm" danger onClick={() => setShowConfirm(true)}>
              <Trash2 size={14} aria-hidden="true" /> Delete organization…
            </SettingsButton>
          </Show>
        </div>

        <Show when={scheduled()}>
          <p class="verevon-settings-status-message verevon-settings-status-message--success" role="status">
            Deletion scheduled. Every member will see a countdown banner, and you can cancel it any time
            within the next 30 days.
          </p>
        </Show>

        <p class="verevon-privacy-danger__note">
          This schedules <strong>{orgName() || 'this organization'}</strong> for permanent deletion in 30
          days. Every member is notified and can export their data; you can cancel any time before the
          deadline. After 30 days the organization and all its data are permanently erased and cannot be
          recovered.
        </p>

        <Show when={showConfirm()}>
          <form class="verevon-privacy-erase-form" onSubmit={runDelete}>
            <label>
              Type <strong>{orgName()}</strong> to confirm
              <input
                type="text"
                autocomplete="off"
                value={typedName()}
                onInput={(event) => setTypedName(event.currentTarget.value)}
                placeholder={orgName()}
              />
            </label>
            <Show when={error()}>
              {(message) => (
                <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
                  {message()}
                </p>
              )}
            </Show>
            <div class="verevon-privacy-erase-actions">
              <SettingsButton type="button" settingsSize="sm" onClick={cancel} disabled={deleting()}>
                Cancel
              </SettingsButton>
              <SettingsButton type="submit" settingsSize="sm" danger disabled={!canDelete()}>
                <Show when={deleting()} fallback={<>Delete organization</>}>
                  <Loader2 size={14} class="verevon-trust-spin" aria-hidden="true" /> Scheduling…
                </Show>
              </SettingsButton>
            </div>
          </form>
        </Show>
      </div>
    </Show>
  )
}
