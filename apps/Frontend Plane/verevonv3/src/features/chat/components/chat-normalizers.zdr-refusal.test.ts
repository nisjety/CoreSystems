import { describe, expect, it } from 'vitest'
import { streamFailurePresentation } from './chat-normalizers'

// A zero-retention refusal in a turn: the reader gets the catalogue's words in
// their language (never the server's English), the action the golden names, and
// the flags of a refusal rather than an outage. Nothing was saved, so anything
// that streamed is kept out of the answer.

const en = (_norwegian: string, english: string) => english
const no = (norwegian: string) => norwegian
const SERVER_TEXT = 'Server operator text in English'

describe('streamFailurePresentation for a zero-retention refusal', () => {
  it('shows the catalogue text in English, with the next step, and never the server text', () => {
    const shown = streamFailurePresentation('zdr_connection_not_acknowledged', SERVER_TEXT, en)
    expect(shown.message).toBe(
      'Zero data retention is on for this organisation, and the connection selected for this chat has not been confirmed for it. The request was not sent to any model. Go to the ChatGPT connection, review it, and send again.',
    )
    expect(shown.message).not.toContain('Server operator')
  })

  it('shows the Norwegian text to a Norwegian reader', () => {
    const shown = streamFailurePresentation('zdr_connection_not_acknowledged', SERVER_TEXT, no)
    expect(shown.message).toBe(
      'Zero Data Retention er slått på for denne organisasjonen, og tilkoblingen som er valgt for denne samtalen er ikke bekreftet for Zero Data Retention. Forespørselen ble ikke sendt til en modell. Gå til ChatGPT-tilkoblingen, se over den, og send på nytt.',
    )
  })

  it('maps the golden action to the control the reader has', () => {
    const actionFor = (code: string) => streamFailurePresentation(code, SERVER_TEXT, en).action
    // Open the connection, then retry.
    expect(actionFor('zdr_connection_not_acknowledged')).toBe('open_integrations')
    // Retry is the button.
    expect(actionFor('zdr_inference_failed')).toBe('retry')
    expect(actionFor('zdr_decision_unavailable')).toBe('retry')
    // Choose another model, or turn a feature off: the same turn cannot help, so no retry.
    expect(actionFor('zdr_unavailable')).toBe('none')
    expect(actionFor('zdr_connection_not_supported')).toBe('none')
    expect(actionFor('zdr_feature_not_supported')).toBe('none')
    // Nothing the reader can do here.
    expect(actionFor('zdr_posture_mismatch')).toBe('none')
  })

  it('is a refusal, not an outage, and keeps what streamed out of the answer', () => {
    const shown = streamFailurePresentation('zdr_decision_unavailable', SERVER_TEXT, en)
    expect(shown).toMatchObject({ unsaved: true, unavailable: false, action: 'retry' })
    expect(shown.serviceTitle).toBeUndefined()
    expect(streamFailurePresentation('zdr_unavailable', SERVER_TEXT, en)).toMatchObject({ unsaved: true, unavailable: false })
  })

  it('names the feature the stream details carry', () => {
    const shown = streamFailurePresentation('zdr_feature_not_supported', SERVER_TEXT, en, { feature: 'web_search' })
    expect(shown.message).toBe(
      'This is not supported with zero data retention yet: web search and browsing. The request was refused before any model call. Turn the feature off and send again.',
    )
  })

  it('names an unknown feature, or none, as the unnamed fallback', () => {
    const unknown = streamFailurePresentation('zdr_feature_not_supported', SERVER_TEXT, en, { feature: 'hologram_export' })
    const missing = streamFailurePresentation('zdr_feature_not_supported', SERVER_TEXT, en)
    expect(unknown.message).toContain('zero data retention yet: this feature.')
    expect(unknown.message).not.toContain('hologram')
    expect(missing.message).toBe(unknown.message)
    expect(missing.action).toBe('none')
  })

  it('gives a zero-retention code the SPA does not know the generic text, with no action', () => {
    const shown = streamFailurePresentation('zdr_from_a_newer_server', SERVER_TEXT, en)
    expect(shown).toMatchObject({ action: 'none', unsaved: true, unavailable: false })
    expect(shown.message).toBe(
      'The request could not be completed with zero data retention. Contact support if the problem persists.',
    )
    expect(shown.message).not.toContain('Server operator')
  })
})
