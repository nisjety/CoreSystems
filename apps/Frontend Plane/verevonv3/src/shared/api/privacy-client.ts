import { requestJson } from './http'

/**
 * Privacy / GDPR self-service client.
 *
 * Talks to the gateway `privacy.rs` domain, which self-scopes every call to the
 * authenticated session's user — the SPA never passes a user id, and a forged
 * one would be ignored upstream. Export is GDPR Art. 15; erase is Art. 17.
 */

/** GDPR Art. 15 export as returned by user-core (`DSARExport`). */
export interface DsarExport {
  subject: string
  subject_id: string
  generated_at: string
  profile: Record<string, unknown>
  org_memberships: Array<Record<string, unknown>>
  api_keys: Array<Record<string, unknown>>
  notes: string[]
}

/**
 * The Control-Plane-only scope disclosure, copied VERBATIM from user-core's
 * `gdpr.go` `BuildDSARExport` notes. Rendered so a user understands, before
 * exporting or erasing, that this surface covers Control-Plane data only and
 * that Model/Data-plane data is handled by a separate erasure fan-out. The
 * accompanying test pins this to the backend text so the two cannot drift.
 */
export const CONTROL_PLANE_DSAR_DISCLOSURE: readonly string[] = [
  'Control Plane export: profile + org memberships + API key metadata.',
  'Audit events for this subject are retained by audit-core (verevon.audit.v1.control.*).',
  'Model Plane run history / conversations and Data Plane documents are purged/exported via the verevon.gdpr.erasure.requested fan-out (follow-up subscribers).',
]

/** Fetch the authenticated user's Control-Plane data export (Art. 15). */
export async function exportMyData(signal?: AbortSignal): Promise<DsarExport> {
  return requestJson<DsarExport>('/api/v1/privacy/export', { signal })
}

/**
 * Irreversibly erase the authenticated user's account (Art. 17). The gateway
 * rejects with 422 unless `confirm: true` is sent, so we always send it; the
 * caller is responsible for the typed-confirm + step-up re-auth gate in the UI.
 * Resolves only on a 2xx — any non-2xx throws, so the caller must NOT sign the
 * user out unless this resolves.
 */
export async function eraseMyAccount(): Promise<void> {
  await requestJson('/api/v1/privacy/erase', {
    method: 'DELETE',
    body: JSON.stringify({ confirm: true }),
  })
}
