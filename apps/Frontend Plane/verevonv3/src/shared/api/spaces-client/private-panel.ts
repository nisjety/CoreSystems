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

/**
 * The owner's private-panel thread ids this browser has been given. Session Core locks each one to conversation scope
 * (SD-22 v1), so the chat controller sends every turn on one with conversation scope. It is a hint only: the gateway
 * and Session Core decide every call, and a missing id fails closed (the send is refused), never open. A known id only
 * ever narrows a turn to conversation scope, so it is safe to share between tabs: the ids are kept in localStorage, so
 * "Continue in Chat" opened in a new tab, or a later visit, keeps the scope (W3-FIX-14, E-PRIO-01). Ids an older build
 * kept in this tab's sessionStorage are still read. Where storage is blocked they last for this page only.
 */
const PRIVATE_PANEL_THREADS_KEY = 'verevon.spaces.privatePanelThreads.v1'
/** A bound on the kept ids: one per room the person opened a panel in. The oldest go first. */
const MAX_PRIVATE_PANEL_THREADS = 500
let privatePanelThreadIds: Set<string> | null = null

function storedIds(storage: () => Storage, into: Set<string>): void {
  try {
    const stored: unknown = JSON.parse(storage().getItem(PRIVATE_PANEL_THREADS_KEY) ?? '[]')
    if (Array.isArray(stored)) {
      for (const id of stored) {
        if (typeof id === 'string' && id) into.add(id)
      }
    }
  } catch {
    // Storage is blocked or holds something else: nothing is read from it.
  }
}

function readStoredIds(into: Set<string>): void {
  storedIds(() => window.localStorage, into)
  storedIds(() => window.sessionStorage, into)
}

function privatePanelThreads(): Set<string> {
  if (privatePanelThreadIds) return privatePanelThreadIds
  const ids = new Set<string>()
  readStoredIds(ids)
  privatePanelThreadIds = ids
  return ids
}

function rememberPrivatePanelThread(threadId: string): void {
  const ids = privatePanelThreads()
  ids.add(threadId)
  // What localStorage holds now: another tab may have added ids since this page read them, and they are kept.
  const shared = new Set<string>()
  storedIds(() => window.localStorage, shared)
  if (shared.has(threadId)) return
  shared.add(threadId)
  for (const id of shared) ids.add(id)
  const kept = Array.from(shared).slice(-MAX_PRIVATE_PANEL_THREADS)
  try {
    window.localStorage.setItem(PRIVATE_PANEL_THREADS_KEY, JSON.stringify(kept))
  } catch {
    // Storage is full or blocked: the id stays in memory for this page.
  }
}

/**
 * Whether a thread is this owner's private panel for a room, as a pointer read named it (see the note above). An id
 * this page has not seen is looked up in storage again, so a panel another tab opened after this page loaded counts.
 */
export function isPrivatePanelThread(threadId: string | null | undefined): boolean {
  if (typeof threadId !== 'string' || !threadId) return false
  const ids = privatePanelThreads()
  if (ids.has(threadId)) return true
  readStoredIds(ids)
  return ids.has(threadId)
}

function panelFrom(wire: PrivatePanelWire | null | undefined, spaceRef: string): SpacePrivatePanelPointer {
  const threadId = typeof wire?.thread_id === 'string' && wire.thread_id.trim() ? wire.thread_id.trim() : null
  const agentSubjectId =
    typeof wire?.agent_subject_id === 'string' && wire.agent_subject_id.trim() ? wire.agent_subject_id.trim() : null
  if (threadId) rememberPrivatePanelThread(threadId)
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
