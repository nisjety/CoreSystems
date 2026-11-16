/**
 * W3-DEC-CANVAS (D2): the room agent's canvas-tools switch as the SPA runs it. Sequencing and presentation only: the one
 * write path is the action registry (`spaces.set_agent_canvas_tools`), the gateway checks the caller's live role
 * (apps/gateway/src/domains/spaces/agent_canvas_tools.rs), and Control decides every canvas action at run time. The codes
 * are the gateway's; the shared words for them live in shared/i18n/errors.ts.
 */
import { executeAction } from '@/shared/actions/action-client'
import type { ActionActor } from '@/shared/actions/types'
import { hasErrorCode } from '@/shared/api/http'
import { spaceCanvasToolsSwitchFrom, type SpaceCanvasToolsSwitch } from '@/shared/api/spaces-client'
import { translateApiError } from '@/shared/i18n'

type Copy = { readonly no: string; readonly en: string }

/**
 * Codes this switch answers with its own sentence. The shared copy for `space_role_cannot_manage_agent` is the wake
 * policy's ("who can wake the agent"), which is the wrong thing to tell someone turning the canvas switch on or off.
 */
const CANVAS_TOOLS_OWN_COPY: Readonly<Record<string, Copy>> = {
  space_role_cannot_manage_agent: {
    no: 'Bare en eier eller leder kan endre om agenten kan bruke rommets lerret. Ingenting er endret.',
    en: 'Only an owner or manager can change whether the agent may use the room canvas. Nothing was changed.',
  },
}

const CANVAS_TOOLS_FALLBACK: Copy = {
  no: 'Innstillingen for lerretet for agenten kunne ikke endres. Ingenting er endret.',
  en: 'The agent canvas setting could not be changed. Nothing was changed.',
}

/** The sentence for a failed switch call: its own words for the codes above, otherwise the shared words or the fallback. */
export function canvasToolsErrorMessage(err: unknown, tr: (noText: string, enText: string) => string): string {
  for (const [code, copy] of Object.entries(CANVAS_TOOLS_OWN_COPY)) {
    if (hasErrorCode(err, code)) return tr(copy.no, copy.en)
  }
  return translateApiError(err, tr, CANVAS_TOOLS_FALLBACK)
}

/**
 * Sets one bound agent's room canvas tools switch in this room through the action registry. A refusal arrives as the
 * ApiError the gateway sent, with its typed code; a reply that does not match the contract throws and writes nothing.
 */
export async function setSpaceAgentCanvasTools(
  spaceRef: string,
  actor: ActionActor,
  bindingRef: string,
  enabled: boolean,
): Promise<SpaceCanvasToolsSwitch> {
  const execution = await executeAction('spaces.set_agent_canvas_tools', actor, { spaceRef, bindingRef, enabled })
  return spaceCanvasToolsSwitchFrom(execution.result)
}
