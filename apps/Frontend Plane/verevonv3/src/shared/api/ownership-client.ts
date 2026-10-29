// Ownership / sharing client (PR-6). Talks to the BFF gateway's ownership +
// shares domains, which proxy user-core's resource_grants facade. All write
// surfaces are server-scoped: the gateway sets org_id + granted_by from the
// validated session, so the client only supplies the subject to share with.

import { requestJson } from '@/shared/api/http'

/**
 * The honesty-gate signal. `gate_open` is computed SERVER-SIDE
 * (enforcement === 'strict' && identity_live) — the client only obeys it; it
 * must never re-derive or override the privacy guarantee.
 */
export type OwnershipStatus = {
  enforcement: 'off' | 'permissive' | 'strict'
  identity_live: boolean
  gate_open: boolean
}

export async function getOwnershipStatus(): Promise<OwnershipStatus> {
  return requestJson<OwnershipStatus>('/api/v1/ownership/status')
}

/** One explicit grant on a document (the ShareDialog "shared with" rows). */
export type DocumentGrant = {
  grant_id: string
  subject_id: string
  role: string
  granted_by: string
  granted_at: string
}

export async function listDocumentShares(docId: string): Promise<{ grants: DocumentGrant[] }> {
  return requestJson<{ grants: DocumentGrant[] }>(
    `/api/v1/documents/${encodeURIComponent(docId)}/shares`,
  )
}

export async function shareDocument(docId: string, subjectId: string): Promise<DocumentGrant> {
  return requestJson<DocumentGrant>(`/api/v1/documents/${encodeURIComponent(docId)}/shares`, {
    method: 'POST',
    body: JSON.stringify({ subject_id: subjectId }),
  })
}

export async function revokeDocumentShare(docId: string, subjectId: string): Promise<void> {
  await requestJson<unknown>(
    `/api/v1/documents/${encodeURIComponent(docId)}/shares/${encodeURIComponent(subjectId)}`,
    { method: 'DELETE' },
  )
}

/** Document ids explicitly shared with the viewer (off ListVisible). */
export async function listSharedWithMe(): Promise<{ ids: string[]; all_org: boolean }> {
  return requestJson<{ ids: string[]; all_org: boolean }>('/api/v1/shares/shared-with-me')
}
