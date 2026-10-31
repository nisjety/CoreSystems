import { describe, expect, it } from 'vitest'
import { translateApiError } from './errors'

// The refusal copy of the org model policy: a member refused a model reads the
// catalogue's own sentence and next step, and an admin whose save is refused reads
// why, never the generic fallback.

const no = (norwegian: string) => norwegian
const en = (_norwegian: string, english: string) => english

describe('translateApiError for the model-entitlement refusals', () => {
  it('gives a refused model its Norwegian text and the next step', () => {
    expect(translateApiError({ code: 'model_not_entitled' }, no)).toBe(
      'Denne modellen er ikke tilgjengelig for deg. Forespørselen ble ikke sendt til en modell. Velg en annen modell og send på nytt.',
    )
    expect(translateApiError({ code: 'model_not_entitled' }, en)).toBe(
      'This model is not available to you. The request was not sent to a model. Choose another model and send again.',
    )
  })

  it('says the policy could not be confirmed, without a next step beyond the button', () => {
    expect(translateApiError({ code: 'model_entitlement_unavailable' }, en)).toBe(
      'Which models you may use cannot be confirmed right now. The request was not sent to a model. Try again shortly.',
    )
  })

  it('explains a refused admin save', () => {
    expect(translateApiError({ code: 'revision_conflict' }, en)).toMatch(/changed by someone else/)
    expect(translateApiError({ code: 'model_policy_leaves_no_zdr_model' }, no)).toMatch(/nullbevaringsgodkjenning/)
    expect(translateApiError({ code: 'zdr_models_not_entitled' }, en)).toMatch(/Zero Data Retention cannot be turned on/)
    expect(translateApiError({ code: 'model_policy_guard_unavailable' }, en)).toMatch(/nothing was saved/)
    expect(translateApiError({ code: 'invalid_model_policy' }, no)).toMatch(/ikke gyldig/)
  })

  it('still uses the caller fallback for a code the SPA does not know', () => {
    expect(translateApiError({ code: 'model_future_refusal' }, en, { no: 'x', en: 'Fallback copy' })).toBe('Fallback copy')
  })
})
