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

/** One participant of a Space as shown to another participant. */
export type SpaceRosterMember = {
  subject_type: 'user' | 'service'
  subject_id: string
  role: 'viewer' | 'editor' | 'manager' | 'owner'
  revision: number
  /** May be empty: a membership can exist before its user projection does. */
  display_name: string
}

/**
 * Who is in this Space. Control gates it on your own membership and answers 404
 * when you are not in the room — that is "you cannot see this", not "the room
 * is empty", and the two must stay distinguishable in the UI.
 */
export async function getSpaceRoster(spaceRef: string): Promise<readonly SpaceRosterMember[]> {
  const response = await requestJson<{ members: readonly SpaceRosterMember[] }>(
    `/api/v1/spaces/${encodeURIComponent(spaceRef)}/roster`,
  )
  return response.members
}

export function getSpaceActions(spaceRef: string): Promise<SpaceActions> {
  return requestJson(`/api/v1/spaces/${encodeURIComponent(spaceRef)}/actions`)
}

/**
 * Provision the caller's own personal Space.
 *
 * Idempotent server-side — an owner has at most one personal Space, so calling
 * this twice returns the same room rather than creating a second.
 *
 * The room comes back as `pending_registration`, not active: Control registers
 * it afterwards, and Space actions stay refused until it does. Treat a
 * successful response as "the room now exists", not as "the room is ready".
 * Owner and organization are taken from the session at the gateway; nothing
 * here identifies the user.
 */
export async function createPersonalSpace(name?: string): Promise<SpaceSummary> {
  const response = await requestJson<{ space: SpaceSummary }>('/api/v1/spaces', {
    method: 'POST',
    body: JSON.stringify(name?.trim() ? { name: name.trim() } : {}),
  })
  return response.space
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
