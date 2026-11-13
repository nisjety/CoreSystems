import { describe, expect, it } from 'vitest'
import { streamFailurePresentation } from './chat-normalizers'

// `org_policy_unavailable` (C-14-mg-rel1): a Do turn refused before any model ran because the organisation's agent
// policy cannot be read. It clears when the policy can be read again, so the reader is offered a retry. Nothing is
// saved. It is a refusal of the policy, not an outage of the model service, so it is not `unavailable`. The server's
// English text is never shown.

const no = (noText: string) => noText
const en = (_noText: string, enText: string) => enText

const SERVER_TEXT =
  "The organisation's agent policy cannot be read right now, so this agent run cannot start. Try again shortly."

describe('org_policy_unavailable stream presentation', () => {
  it('saves nothing, offers a retry, and is not a model-service outage', () => {
    expect(streamFailurePresentation('org_policy_unavailable', SERVER_TEXT, no)).toEqual({
      message: 'Organisasjonens agentpolicy kan ikke leses akkurat nå, så agenten startet ikke. Ingenting ble kjørt.',
      unsaved: true,
      unavailable: false,
      action: 'retry',
    })
  })

  it('words it in English for an English reader, and never shows the server text', () => {
    const shown = streamFailurePresentation('org_policy_unavailable', SERVER_TEXT, en)
    expect(shown.message).toBe(
      'The agent policy for this organisation cannot be read right now, so the agent did not start. Nothing was run.',
    )
    expect(shown.message).not.toContain(SERVER_TEXT)
  })
})
