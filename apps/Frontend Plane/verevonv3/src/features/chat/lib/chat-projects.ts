import { canStartChatProject, chatProjectSchema, isEligibleChatProject, type ChatProject } from '@/shared/actions/chat-project'
import {
  getSpaceContext,
  getSpaceMembership,
  getSpaceRoster,
  listSpaces,
  type SpaceSummary,
} from '@/shared/api/spaces-client'
import { readClientJson, readSessionValue, removeSessionValue, writeClientJson, writeSessionValue } from '@/shared/session/client-storage'

export const CHAT_PROJECT_START_EVENT = 'verevon:chat-project-start'

const PENDING_PROJECT_KEY = 'verevon.chat-project.pending.v1'
const PROJECT_THREAD_HINTS_KEY = 'verevon.chat-project-threads.v1'
const AUDIENCE_CONFIRMED_KEY = 'verevon.chat-project-audience.v1'
const MAX_PROJECT_THREAD_HINTS = 200
const MAX_AUDIENCE_CONFIRMATIONS = 200

/**
 * Whether this build offers project chats (WS18-S4). The gateway's switch,
 * GATEWAY_CHAT_PROJECTS_ENABLED, is not visible to the SPA, so the build carries
 * the same decision and the two are flipped together, as `planExecuteEnabled`
 * does. Only the literal "true" enables.
 *
 * Keep it unset until Dreaming excludes space threads (plan K3-5): until then a
 * project message can be mined into its author's personal memory.
 */
export const chatProjectsEnabled = (): boolean => import.meta.env.VITE_CHAT_PROJECTS_ENABLED === 'true'

/**
 * The Spaces the caller can start a project chat in. A Space whose membership
 * cannot be read is left out: a role the client cannot see is not one it may use.
 * Control and the gateway decide again on every turn.
 */
export async function listChatProjects(): Promise<ChatProject[]> {
  const candidates = (await listSpaces()).filter((space) => isEligibleChatProject(space))
  const memberships = await Promise.allSettled(candidates.map((space) => getSpaceMembership(space.space_ref)))
  const projects: ChatProject[] = []
  candidates.forEach((space, index) => {
    const result = memberships[index]
    if (result?.status !== 'fulfilled' || !canStartChatProject(result.value.role)) return
    const parsed = chatProjectSchema.safeParse({
      spaceRef: space.space_ref,
      name: space.name,
      kind: space.kind,
      role: result.value.role,
    })
    if (parsed.success) projects.push(parsed.data)
  })
  // A personal Space first, then the rooms in the order Control lists them.
  return projects.sort((a, b) => (a.kind === b.kind ? 0 : a.kind === 'personal' ? -1 : 1))
}

/** The Space a project chat is about and the caller's role in it; null when the caller cannot read it. */
export async function readProjectContext(spaceRef: string): Promise<{ space: SpaceSummary; role: string } | null> {
  try {
    const context = await getSpaceContext(spaceRef)
    return { space: context.space, role: context.membership.role }
  } catch {
    return null
  }
}

/** How many people can read the project, counted from the roster the caller may see; null when it cannot be read. */
export async function readProjectAudience(spaceRef: string): Promise<number | null> {
  try {
    const members = await getSpaceRoster(spaceRef)
    return members.filter((member) => member.subject_type === 'user').length
  } catch {
    return null
  }
}

/**
 * Asks the chat page to start a new chat in a project. The request travels on the
 * event for a mounted page and in tab storage for one that is not mounted yet (the
 * person is on another route). Returns whether the tab storage kept it, so a caller
 * that must navigate can refuse when neither channel can reach the page.
 */
export function queueChatProjectStart(spaceRef: string): boolean {
  const stored = writeSessionValue(PENDING_PROJECT_KEY, spaceRef)
  if (typeof window !== 'undefined') {
    window.dispatchEvent(new CustomEvent(CHAT_PROJECT_START_EVENT, { detail: { spaceRef } }))
  }
  return stored
}

export function takeChatProjectStart(): string | null {
  const spaceRef = readSessionValue(PENDING_PROJECT_KEY)?.trim() || null
  removeSessionValue(PENDING_PROJECT_KEY)
  return spaceRef
}

const isRecordOfStrings = (value: unknown): value is Record<string, string> => (
  typeof value === 'object'
  && value !== null
  && !Array.isArray(value)
  && Object.values(value).every((entry) => typeof entry === 'string')
)

const isStringList = (value: unknown): value is string[] => (
  Array.isArray(value) && value.every((entry) => typeof entry === 'string')
)

function readProjectThreadHints(): Record<string, string> {
  return readClientJson(PROJECT_THREAD_HINTS_KEY, isRecordOfStrings) ?? {}
}

/**
 * Remembers which project a thread belongs to, so a reload can reopen it. This is a
 * routing hint for this viewer only, never authority: Control decides every read and
 * write. Losing it only means the thread is reopened from its project list.
 */
export function rememberProjectThread(threadId: string, spaceRef: string): void {
  if (!threadId || !spaceRef) return
  const others = Object.entries(readProjectThreadHints()).filter(([id]) => id !== threadId)
  const kept = [...others, [threadId, spaceRef] as const].slice(-MAX_PROJECT_THREAD_HINTS)
  writeClientJson(PROJECT_THREAD_HINTS_KEY, Object.fromEntries(kept))
}

/** The project a thread was opened from in this browser, if the switch is on and the hint is known. */
export function projectThreadRef(threadId: string | null | undefined): string | undefined {
  if (!threadId || !chatProjectsEnabled()) return undefined
  const hints = readProjectThreadHints()
  return Object.prototype.hasOwnProperty.call(hints, threadId) ? hints[threadId] : undefined
}

function readAudienceConfirmations(): string[] {
  return readClientJson(AUDIENCE_CONFIRMED_KEY, isStringList) ?? []
}

/** Whether this browser has already confirmed who can see a shared project's messages. */
export function audienceConfirmed(spaceRef: string): boolean {
  return readAudienceConfirmations().includes(spaceRef)
}

export function rememberAudienceConfirmed(spaceRef: string): void {
  const next = [spaceRef, ...readAudienceConfirmations().filter((ref) => ref !== spaceRef)]
  writeClientJson(AUDIENCE_CONFIRMED_KEY, next.slice(0, MAX_AUDIENCE_CONFIRMATIONS))
}
