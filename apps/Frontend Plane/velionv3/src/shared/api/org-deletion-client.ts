import { requestJson } from './http'

/**
 * Org deletion (Flow C) self-service client.
 *
 * Talks to the gateway `orgs.rs` GDPR-deletion routes, which proxy to
 * org-core's `/orgs/:id/gdpr/*` surface (`internal/http/gdpr_handlers.go`).
 * Soft-delete opens a 30-day grace window during which the org is inert but
 * recoverable; restore reverses it; the remaining three endpoints back the
 * per-member self-service checkpoints (export received / notice acknowledged)
 * shown by the app-wide pending-deletion banner.
 *
 * org-core makes the authoritative authorization decision on every call
 * (owner-only for soft-delete/restore, self-or-platform-admin for the
 * checkpoint routes) — the gateway's broad membership gate runs first.
 */

/** One member's export/acknowledge checkpoint row (`org_deletion_members`). */
export interface DeletionLedgerEntry {
  user_id: string
  notified_at?: string
  exported_at?: string
  acknowledged_at?: string
}

/** Shape returned by `GET /orgs/:id/gdpr/deletion/status`. */
export interface DeletionStatus {
  pending: boolean
  /** RFC3339 purge deadline. Present only when `pending` is true. */
  deadline?: string
  org_name: string
  /** The calling member's own checkpoint, when the org is pending deletion. */
  member_status?: DeletionLedgerEntry
  /** Every member's ledger row — populated only for an owner/admin caller. */
  members?: DeletionLedgerEntry[]
}

/**
 * Irreversibly-scheduled (but reversible within 30 days) organization
 * deletion. `orgName` must exactly match the organization's real name — the
 * gateway/org-core never trust a client-supplied name for anything beyond
 * this "type the name to confirm" check; the caller is responsible for
 * surfacing the resulting 400 (mismatch) message from `ApiError`.
 */
export async function triggerOrgSoftDelete(
  orgId: string,
  confirm: boolean,
  orgName: string,
): Promise<void> {
  await requestJson(`/api/v1/orgs/${encodeURIComponent(orgId)}/gdpr/soft-delete`, {
    method: 'DELETE',
    body: JSON.stringify({ confirm, org_name: orgName }),
  })
}

/**
 * Reverses a pending soft-delete: clears the org back to active and wipes its
 * deletion ledger. Owner-gated. Throws (409 via `ApiError`) if the org is not
 * currently pending deletion.
 */
export async function restoreOrg(orgId: string): Promise<void> {
  await requestJson(`/api/v1/orgs/${encodeURIComponent(orgId)}/gdpr/restore`, {
    method: 'POST',
  })
}

/**
 * Records that the calling member has received their personal-data export
 * ahead of the organization's scheduled purge. Any active member may call
 * this for themselves.
 */
export async function markExported(orgId: string): Promise<void> {
  await requestJson(`/api/v1/orgs/${encodeURIComponent(orgId)}/gdpr/deletion/mark-exported`, {
    method: 'POST',
  })
}

/**
 * Records that the calling member has acknowledged the organization's
 * pending-deletion notice.
 */
export async function acknowledgeDeletion(orgId: string): Promise<void> {
  await requestJson(`/api/v1/orgs/${encodeURIComponent(orgId)}/gdpr/deletion/acknowledge`, {
    method: 'POST',
  })
}

/**
 * Reads the organization's pending-deletion window (if any) and the calling
 * member's own export/acknowledge checkpoint. An owner/admin caller
 * additionally receives every member's ledger row in `members`.
 */
export async function getDeletionStatus(orgId: string): Promise<DeletionStatus> {
  return requestJson<DeletionStatus>(
    `/api/v1/orgs/${encodeURIComponent(orgId)}/gdpr/deletion/status`,
  )
}
