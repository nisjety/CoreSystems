// WS12-S3: direct messages. `POST /api/v1/spaces/dm` answers 202 (created, the Space is
// `pending_registration` until Control registers the roster) or 200 (the DM for exactly these
// people already existed, and is shown again for the caller); 400 `dm_participants_invalid`
// (`details.reason`), 403 `dm_not_entitled` (`details.missing[]`), 404 `dm_disabled`, 409
// `dm_registration_failed`, 412 `zdr_durable_mutation_forbidden`, 503 `zdr_decision_unavailable` /
// `space_provisioning_unavailable`. `POST` and `DELETE /api/v1/spaces/{space_ref}/hide` answer 409
// `space_not_hideable` for anything that is not a DM and 503 `dm_hide_unavailable`. They arrive as
// an ApiError carrying the code, rendered by `translateApiError`.
import { createSignal } from 'solid-js'
import { requestJson } from '../http'

export type SpaceDmParticipant = {
  readonly subject_id: string
  /** Empty on the open reply (the caller already knows the names it picked). */
  readonly display_name?: string
}

export type SpaceDmOpenResult = {
  readonly spaceRef: string
  readonly kind: 'dm' | 'group_dm'
  readonly lifecycle: string
  /** False when the DM for exactly this set of people already existed. */
  readonly created: boolean
  readonly participants: readonly SpaceDmParticipant[]
}

/**
 * Open (or return) the DM for exactly these people. `participants` are the OTHER people's subject
 * ids (1 to 8; the caller is added from the session). `clientRequestId` only identifies the click.
 */
export async function openSpaceDm(
  participants: readonly string[],
  clientRequestId: string = newClientRequestId(),
): Promise<SpaceDmOpenResult> {
  const response = await requestJson<{
    space?: { space_ref?: string; kind?: string; lifecycle?: string; participants?: readonly SpaceDmParticipant[] }
    created?: boolean
  }>('/api/v1/spaces/dm', {
    method: 'POST',
    body: JSON.stringify({ participants, client_request_id: clientRequestId }),
  })
  const space = response.space
  if (!space || typeof space.space_ref !== 'string' || space.space_ref === '') {
    throw new Error('dm_open_unreadable')
  }
  return {
    spaceRef: space.space_ref,
    kind: space.kind === 'group_dm' ? 'group_dm' : 'dm',
    lifecycle: typeof space.lifecycle === 'string' ? space.lifecycle : 'pending_registration',
    created: response.created === true,
    participants: Array.isArray(space.participants) ? space.participants : [],
  }
}

export type SpaceHideResult = {
  readonly spaceRef: string
  readonly hidden: boolean
  /** Epoch milliseconds; `null` once shown again. */
  readonly hiddenAt: number | null
}

async function setHidden(spaceRef: string, method: 'POST' | 'DELETE'): Promise<SpaceHideResult> {
  const response = await requestJson<{ space_ref?: string; hidden?: boolean; hidden_at?: number | null }>(
    `/api/v1/spaces/${encodeURIComponent(spaceRef)}/hide`,
    { method },
  )
  return {
    spaceRef: response.space_ref ?? spaceRef,
    hidden: response.hidden === true,
    hiddenAt: typeof response.hidden_at === 'number' ? response.hidden_at : null,
  }
}

/** Hide one DM from the caller's own sidebar. Nothing is deleted; a new message brings it back. */
export function hideSpaceDm(spaceRef: string) {
  return setHidden(spaceRef, 'POST')
}

/** Show a hidden DM again. */
export function unhideSpaceDm(spaceRef: string) {
  return setHidden(spaceRef, 'DELETE')
}

function newClientRequestId(): string {
  const random = typeof crypto !== 'undefined' && 'randomUUID' in crypto ? crypto.randomUUID() : `${Date.now()}-${Math.random()}`
  return `dm-${random}`.slice(0, 64)
}

// Whether the gateway offers DMs, as the last `GET /api/v1/spaces` said (`features.dm`). A rollout
// answer only; the open route re-checks it. Off until a listing says otherwise.
const [dmListingEnabled, setDmListingEnabledSignal] = createSignal(false)

export function setDmListingEnabled(enabled: boolean): void {
  setDmListingEnabledSignal(enabled)
}

/** Reactive: true once the Spaces listing reported `features.dm`. */
export function spaceDmEnabled(): boolean {
  return dmListingEnabled()
}
