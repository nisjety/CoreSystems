import { Loader2, ShieldAlert } from 'lucide-solid'
import { createMemo, createSignal, Show } from 'solid-js'
import { useI18n } from '@/shared/i18n'
import { ApiError } from '@/shared/api/http'
import {
  acknowledgeDeletion,
  markExported,
  restoreOrg,
  type DeletionStatus,
} from '@/shared/api/org-deletion-client'
import { exportMyData, type DsarExport } from '@/shared/api/privacy-client'
import { getSession } from '@/shared/session/session-store'

function errorMessage(error: unknown): string {
  return error instanceof ApiError || error instanceof Error ? error.message : 'Unexpected error'
}

function isOrgOwner(role: string | null | undefined): boolean {
  return (role ?? '').trim().toLowerCase() === 'owner'
}

function daysUntil(deadline: string | undefined): number | null {
  if (!deadline) return null
  const ms = new Date(deadline).getTime() - Date.now()
  if (Number.isNaN(ms)) return null
  return Math.max(0, Math.ceil(ms / (1000 * 60 * 60 * 24)))
}

function downloadDsarExport(data: DsarExport): void {
  const blob = new Blob([JSON.stringify(data, null, 2)], { type: 'application/json' })
  const url = URL.createObjectURL(blob)
  const anchor = document.createElement('a')
  anchor.href = url
  anchor.download = `velion-data-export-${data.subject_id}.json`
  anchor.click()
  URL.revokeObjectURL(url)
}

/**
 * App-wide pending-organization-deletion notice — rendered once in CoreShell
 * (like FeedbackWidget) so it shows on every route for every member, not just
 * the owner. CoreShell owns the `getDeletionStatus` resource (it also needs
 * the pending flag to reflow `.core-main`'s top padding around the fixed
 * banner) and passes it down; this component renders nothing while no
 * deletion is pending.
 *
 * "Export my data" reuses the EXISTING user-core DSAR export
 * (`GET /api/v1/users/:id/gdpr/export`, proxied self-scoped via
 * `privacy-client.ts`'s `exportMyData`) rather than inventing a new export —
 * Flow C only needs a checkpoint that the member received it, which
 * `markExported` records after the download starts.
 */
export function OrgDeletionBanner(props: {
  status: DeletionStatus | null | undefined
  onRefetch: () => void
}) {
  const i18n = useI18n()
  const session = getSession()
  const orgId = createMemo(() => session.activeOrg?.id ?? null)
  const isOwner = createMemo(() => isOrgOwner(session.activeOrg?.role))
  const status = () => props.status

  const [exporting, setExporting] = createSignal(false)
  const [acknowledging, setAcknowledging] = createSignal(false)
  const [cancelling, setCancelling] = createSignal(false)
  const [actionError, setActionError] = createSignal<string | null>(null)

  const daysRemaining = createMemo(() => daysUntil(status()?.deadline))
  const alreadyExported = createMemo(() => Boolean(status()?.member_status?.exported_at))
  const alreadyAcknowledged = createMemo(() => Boolean(status()?.member_status?.acknowledged_at))

  async function handleExport() {
    const id = orgId()
    if (!id || exporting()) return
    setExporting(true)
    setActionError(null)
    try {
      downloadDsarExport(await exportMyData())
      await markExported(id)
      props.onRefetch()
    } catch (reason) {
      setActionError(errorMessage(reason))
    } finally {
      setExporting(false)
    }
  }

  async function handleAcknowledge() {
    const id = orgId()
    if (!id || acknowledging()) return
    setAcknowledging(true)
    setActionError(null)
    try {
      await acknowledgeDeletion(id)
      props.onRefetch()
    } catch (reason) {
      setActionError(errorMessage(reason))
    } finally {
      setAcknowledging(false)
    }
  }

  async function handleCancel() {
    const id = orgId()
    if (!id || cancelling()) return
    setCancelling(true)
    setActionError(null)
    try {
      await restoreOrg(id)
      props.onRefetch()
    } catch (reason) {
      setActionError(errorMessage(reason))
    } finally {
      setCancelling(false)
    }
  }

  return (
    <Show when={status()?.pending}>
      <div class="org-deletion-banner" role="alert">
        <div class="org-deletion-banner__message">
          <ShieldAlert size={16} aria-hidden="true" />
          <span>
            {i18n.tr(
              `${status()?.org_name || 'Denne organisasjonen'} skal slettes permanent om ${daysRemaining() ?? '—'} dag${daysRemaining() === 1 ? '' : 'er'}.`,
              `${status()?.org_name || 'This organization'} is scheduled for permanent deletion in ${daysRemaining() ?? '—'} day${daysRemaining() === 1 ? '' : 's'}.`,
            )}
          </span>
        </div>

        <div class="org-deletion-banner__actions">
          <button
            type="button"
            class="org-deletion-banner__action"
            onClick={() => void handleExport()}
            disabled={exporting()}
          >
            <Show when={exporting()}>
              <Loader2 size={13} class="velion-trust-spin" aria-hidden="true" />
            </Show>
            {alreadyExported()
              ? i18n.tr('Data eksportert ✓', 'Data exported ✓')
              : i18n.tr('Eksporter mine data', 'Export my data')}
          </button>

          <button
            type="button"
            class="org-deletion-banner__action"
            onClick={() => void handleAcknowledge()}
            disabled={acknowledging() || alreadyAcknowledged()}
          >
            <Show when={acknowledging()}>
              <Loader2 size={13} class="velion-trust-spin" aria-hidden="true" />
            </Show>
            {alreadyAcknowledged() ? i18n.tr('Bekreftet ✓', 'Acknowledged ✓') : i18n.tr('Bekreft', 'Acknowledge')}
          </button>

          <Show when={isOwner()}>
            <button
              type="button"
              class="org-deletion-banner__action org-deletion-banner__action--primary"
              onClick={() => void handleCancel()}
              disabled={cancelling()}
            >
              <Show when={cancelling()}>
                <Loader2 size={13} class="velion-trust-spin" aria-hidden="true" />
              </Show>
              {i18n.tr('Avbryt sletting', 'Cancel deletion')}
            </button>
          </Show>
        </div>

        <Show when={actionError()}>
          {(message) => <span class="org-deletion-banner__error">{message()}</span>}
        </Show>
      </div>
    </Show>
  )
}
