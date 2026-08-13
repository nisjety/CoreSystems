import { requestJson } from './http'

export type SpaceMembership = {
  space_ref: string
  org_id: string
  subject_id: string
  kind: 'personal' | 'shared' | string
  role: string
  revisions: {
    authority: number
    membership: number
    privacy: number
    recipient_audience: number
    entitlement: number
  }
}

export type SpaceSummary = {
  space_ref: string
  name: string
  kind: string
  lifecycle: string
}

/** A server-composed lifecycle + current-membership read. It contains no
 * signed decision, recipient audience, or owner-resource authority. */
export type SpaceContext = {
  space: SpaceSummary
  membership: SpaceMembership
}

/** Owner-bound conversation index for the selected Space. The BFF has already
 * rechecked lifecycle and membership; this remains a read model, not a grant. */
export type SpaceThread = {
  thread_id: string
  space_id: string
  title?: string
  preview?: string
  updated_at?: string
  latest_run_id?: string
  latest_run_status?: string
  latest_run_updated_at?: string
}

export type SpaceThreads = {
  space: SpaceSummary
  membership: SpaceMembership
  threads: readonly SpaceThread[]
}

/** Actor-filtered owner contracts available in a Space. This is an availability
 * view only: each owner plane reauthorizes the exact resource at execution. */
export type SpaceActions = {
  space_ref: string
  actor_type: 'human'
  catalog: {
    catalogVersion: string
    actorType: 'human'
    actions: readonly unknown[]
  }
}

export type SpaceDeletionRequest = {
  requestId: string
  idempotencyKey: string
  spaceRef: string
  externalOrgId: string
  ownerExternalAuthId: string
  state: 'pending_authorization' | 'authorized' | 'blocked_legal_hold' | 'rejected'
  createdAt: number
  updatedAt: number
}

export type SpaceDeletionOwnerReceipt = {
  ownerPlane: 'application' | 'control' | 'data' | 'ingestion' | 'model' | 'infra'
  status: 'pending' | 'blocked_legal_hold' | 'succeeded' | 'partial' | 'failed' | 'unknown'
  receiptRef?: string
  detail?: string
  updatedAt: number
}

/** Authorization and purge progress stay separate. `authorized` does not mean
 * deleted; `purgeStatus` is successful only after every owner reports it. */
export type SpaceDeletionReceipt = {
  request: SpaceDeletionRequest
  receipts: readonly SpaceDeletionOwnerReceipt[]
  purgeStatus: SpaceDeletionOwnerReceipt['status']
}

/** Current Control membership only; it deliberately contains no bearer,
 * recipient audience, or resource decision that a browser could replay. */
export function getSpaceMembership(spaceRef: string): Promise<SpaceMembership> {
  return requestJson(`/api/v1/spaces/${encodeURIComponent(spaceRef)}/membership`)
}

export async function listSpaces(): Promise<readonly SpaceSummary[]> {
  const response = await requestJson<{ spaces: readonly SpaceSummary[] }>('/api/v1/spaces')
  return response.spaces
}

export function getSpaceContext(spaceRef: string): Promise<SpaceContext> {
  return requestJson(`/api/v1/spaces/${encodeURIComponent(spaceRef)}/context`)
}

export function getSpaceThreads(spaceRef: string): Promise<SpaceThreads> {
  return requestJson(`/api/v1/spaces/${encodeURIComponent(spaceRef)}/threads`)
}

export function getSpaceActions(spaceRef: string): Promise<SpaceActions> {
  return requestJson(`/api/v1/spaces/${encodeURIComponent(spaceRef)}/actions`)
}

export function requestPersonalSpaceDeletion(spaceRef: string, idempotencyKey: string): Promise<SpaceDeletionRequest> {
  return requestJson(`/api/v1/spaces/${encodeURIComponent(spaceRef)}/deletion-requests`, {
    method: 'POST',
    body: JSON.stringify({ idempotency_key: idempotencyKey }),
  })
}

export function getPersonalSpaceDeletionReceipt(requestId: string): Promise<SpaceDeletionReceipt> {
  return requestJson(`/api/v1/spaces/deletion-requests/${encodeURIComponent(requestId)}`)
}
