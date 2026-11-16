// WS17-S6: the owner's private panel from the SPA. The gateway keeps one pointer per person and room
// (apps/gateway/src/domains/spaces/private_panel.rs, owed: see the WS17-S6 depth file) and relays the
// Application table `spacePrivatePanels`:
//   GET  /api/v1/spaces/{ref}/private-panel   200 {data:{space_ref, thread_id|null, agent_subject_id|null}}
//   POST /api/v1/spaces/{ref}/private-panel   200 {data:{space_ref, thread_id, agent_subject_id|null}}  (get or create)
// The pointer carries ids only. The panel's thread is read through the chat transcript route, which
// enforces its owner. Sharing a reply to the room is a human post (`createSpacePost`), never a call here.
// Errors arrive as an ApiError with the typed code: 403 `active_membership_required`, 404 `feature_disabled`
// (gateway flag off), 412 `zdr_durable_mutation_forbidden` (refused before any Control call), 503 (retry).
// The UI gate is `features.private_panel` on the room context. Presentation only: the gateway decides every call.
import { requestJson } from '../http'

/** One person's private panel for one room. `threadId` is null until the gateway has a thread to show. */
export type SpacePrivatePanelPointer = {
  readonly spaceRef: string
  readonly threadId: string | null
  readonly agentSubjectId: string | null
}

type PrivatePanelWire = {
  space_ref?: unknown
  thread_id?: unknown
  agent_subject_id?: unknown
}

function panelFrom(wire: PrivatePanelWire | null | undefined, spaceRef: string): SpacePrivatePanelPointer {
  const threadId = typeof wire?.thread_id === 'string' && wire.thread_id.trim() ? wire.thread_id.trim() : null
  const agentSubjectId =
    typeof wire?.agent_subject_id === 'string' && wire.agent_subject_id.trim() ? wire.agent_subject_id.trim() : null
  return { spaceRef, threadId, agentSubjectId }
}

/** The caller's panel pointer for one room. Reads the pointer only, never a message. */
export async function getSpacePrivatePanel(spaceRef: string): Promise<SpacePrivatePanelPointer> {
  const wire = await requestJson<PrivatePanelWire>(`/api/v1/spaces/${encodeURIComponent(spaceRef)}/private-panel`)
  return panelFrom(wire, spaceRef)
}

/** Get or create the caller's panel for one room. Idempotent: a repeat returns the same thread. */
export async function openSpacePrivatePanel(spaceRef: string): Promise<SpacePrivatePanelPointer> {
  const wire = await requestJson<PrivatePanelWire>(`/api/v1/spaces/${encodeURIComponent(spaceRef)}/private-panel`, {
    method: 'POST',
    body: JSON.stringify({}),
  })
  return panelFrom(wire, spaceRef)
}
