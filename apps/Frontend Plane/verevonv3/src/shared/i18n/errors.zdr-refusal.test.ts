import { describe, expect, it } from 'vitest'
import { translateApiError } from './errors'

// Every JSON surface that reports a zero-retention refusal (a schedule, a
// browser run, a draft, the speech route) reads its words from this seam, so the
// codes of the Model Plane catalogue must come out in the reader's language, with
// the feature named and the next step given, and never as the generic fallback.

const no = (norwegian: string) => norwegian
const en = (_norwegian: string, english: string) => english

describe('translateApiError for the zero-retention refusals', () => {
  it('gives a catalogue code its Norwegian and English text, with the next step', () => {
    expect(translateApiError({ code: 'zdr_unavailable' }, no)).toBe(
      'Zero Data Retention gjelder for denne forespørselen, men ingen modellinstans er godkjent til å levere det. Forespørselen ble ikke sendt til en modell. Velg en annen modell og send på nytt.',
    )
    expect(translateApiError({ code: 'zdr_unavailable' }, en)).toBe(
      'Zero data retention applies to this request, but no model deployment is attested to provide it. The request was not sent to a model. Choose another model and send again.',
    )
  })

  it('names the feature the error carries, by its key', () => {
    expect(translateApiError({ code: 'zdr_feature_not_supported', feature: 'browser_run' }, en)).toBe(
      'This is not supported with zero data retention yet: browser runs. The request was refused before any model call. Turn the feature off and send again.',
    )
  })

  it('says "this feature" when the error names no feature the SPA knows', () => {
    expect(translateApiError({ code: 'zdr_feature_not_supported' }, en)).toContain('this feature')
    expect(translateApiError({ code: 'zdr_feature_not_supported', feature: 'a_future_feature' }, no)).toContain(
      'denne funksjonen',
    )
  })

  it('keeps the existing copy of the organisation switch codes, which are not catalogue codes', () => {
    expect(translateApiError({ code: 'zdr_no_attested_provider' }, en)).toMatch(/no AI provider has a recorded/)
  })

  it('still uses the caller fallback for a zero-retention code the SPA does not know', () => {
    const fallback = { no: 'Reservekopi', en: 'Fallback copy' }
    expect(translateApiError({ code: 'zdr_a_future_code' }, en, fallback)).toBe('Fallback copy')
  })
})
