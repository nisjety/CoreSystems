import { describe, expect, it } from 'vitest'
import { streamFailurePresentation } from './chat-normalizers'

// A model-entitlement refusal in a turn: the reader gets the catalogue's words in
// their language (Norwegian first, never the server's English), the action the
// golden names, and the flags of a refusal rather than an outage. Nothing was
// saved, so anything that streamed is kept out of the answer.

const en = (_norwegian: string, english: string) => english
const no = (norwegian: string) => norwegian
const SERVER_TEXT = 'Server operator text in English'

describe('streamFailurePresentation for a model-entitlement refusal', () => {
  it('shows model_not_entitled in Norwegian with the next step, and never the server text', () => {
    const shown = streamFailurePresentation('model_not_entitled', SERVER_TEXT, no)
    expect(shown.message).toBe(
      'Denne modellen er ikke tilgjengelig for deg. Forespørselen ble ikke sendt til en modell. Velg en annen modell og send på nytt.',
    )
    expect(shown.message).not.toContain('Server operator')
  })

  it('shows model_not_entitled in English from the catalogue', () => {
    const shown = streamFailurePresentation('model_not_entitled', SERVER_TEXT, en)
    expect(shown.message).toBe(
      'This model is not available to you. The request was not sent to a model. Choose another model and send again.',
    )
  })

  it('offers no retry for a model the reader may not use: the same turn would be refused again', () => {
    expect(streamFailurePresentation('model_not_entitled', SERVER_TEXT, en)).toMatchObject({
      action: 'none',
      unsaved: true,
      unavailable: false,
    })
  })

  it('offers the retry button when the entitlement could not be confirmed', () => {
    const shown = streamFailurePresentation('model_entitlement_unavailable', SERVER_TEXT, no)
    expect(shown.message).toBe(
      'Det kan ikke bekreftes akkurat nå hvilke modeller du kan bruke. Forespørselen ble ikke sendt til en modell. Prøv igjen om litt.',
    )
    expect(shown).toMatchObject({ action: 'retry', unsaved: true, unavailable: false })
    expect(shown.serviceTitle).toBeUndefined()
  })

  it('leaves every other code to its own rendering', () => {
    const shown = streamFailurePresentation('model_not_entitled_somewhere_else', SERVER_TEXT, en)
    expect(shown.message).toBe(SERVER_TEXT)
  })
})
