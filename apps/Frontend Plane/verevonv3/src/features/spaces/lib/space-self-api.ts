/**
 * WS18-S6: the room's own-thread agent switch as the SPA runs it. Sequencing and presentation only: the one write
 * path is the action registry (`spaces.set_agent_self_api`), the gateway checks the caller's live role
 * (apps/gateway/src/domains/spaces/self_api.rs), and Control decides every action the agent takes at run time.
 * The codes are the gateway's; the words for them live in shared/i18n/errors.ts.
 */
import { executeAction } from '@/shared/actions/action-client'
import type { ActionActor } from '@/shared/actions/types'
import { hasErrorCode } from '@/shared/api/http'
import { spaceSelfApiSwitchFrom, type SpaceSelfApiSwitch, type SpaceThread } from '@/shared/api/spaces-client'
import { translateApiError } from '@/shared/i18n'

type Copy = { readonly no: string; readonly en: string }

/**
 * Codes this switch answers with its own sentence. The shared copy for `space_role_cannot_manage_agent` is the wake
 * policy's ("who can wake the agent"), which is the wrong thing to tell someone turning this switch on or off.
 */
const SELF_API_OWN_COPY: Readonly<Record<string, Copy>> = {
  space_role_cannot_manage_agent: {
    no: 'Bare en eier eller leder kan endre om agenten kan handle i sin egen tråd. Ingenting er endret.',
    en: 'Only an owner or manager can change whether the agent may act on its own thread. Nothing was changed.',
  },
}

const SELF_API_FALLBACK: Copy = {
  no: 'Innstillingen for agenten kunne ikke endres. Ingenting er endret.',
  en: 'The agent setting could not be changed. Nothing was changed.',
}

/** The sentence for a failed switch call: its own words for the codes above, otherwise the shared words or the fallback. */
export function selfApiErrorMessage(err: unknown, tr: (noText: string, enText: string) => string): string {
  for (const [code, copy] of Object.entries(SELF_API_OWN_COPY)) {
    if (hasErrorCode(err, code)) return tr(copy.no, copy.en)
  }
  return translateApiError(err, tr, SELF_API_FALLBACK)
}

/**
 * Sets one bound agent's own-thread switch in this room through the action registry. A refusal arrives as the
 * ApiError the gateway sent, with its typed code; a reply that does not match the contract throws and writes nothing.
 */
export async function setSpaceAgentSelfApi(
  spaceRef: string,
  actor: ActionActor,
  bindingRef: string,
  enabled: boolean,
): Promise<SpaceSelfApiSwitch> {
  const execution = await executeAction('spaces.set_agent_self_api', actor, { spaceRef, bindingRef, enabled })
  return spaceSelfApiSwitchFrom(execution.result)
}

/** A room agent renamed this post (its title's last writer). Absent is never an agent. */
export function threadRenamedByAgent(thread: Pick<SpaceThread, 'title_source'>): boolean {
  return thread.title_source === 'agent'
}

/** A room agent pinned this post (the pin's last writer). Only a pinned post can say so. */
export function threadPinnedByAgent(thread: Pick<SpaceThread, 'pinned' | 'pinned_source'>): boolean {
  return thread.pinned === true && thread.pinned_source === 'agent'
}
