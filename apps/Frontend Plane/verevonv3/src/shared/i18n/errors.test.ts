import { describe, expect, it } from 'vitest'
import { translateApiError } from './errors'

describe('translateApiError', () => {
  it('keeps an unknown provider-send outcome distinct from a normal retryable failure', () => {
    const tr = (no: string) => no

    expect(translateApiError({ code: 'delivery_unknown' }, tr)).toMatch(/ikke send på nytt automatisk/i)
    expect(translateApiError({ code: 'send_failed' }, tr)).toMatch(/ble ikke sendt/i)
    expect(translateApiError({ code: 'delivery_unavailable' }, tr)).toMatch(/ikke sendt eller lagret/i)
  })

  // Owner ruling (5): a personal Microsoft/Google connection's two ownership
  // failures must surface as their own specific copy, not the generic
  // fallback — this is the seam the connector-attach picker relies on.
  it('gives the personal-connection ownership codes their own specific copy', () => {
    const tr = (no: string) => no

    expect(translateApiError({ code: 'personal_connection_forbidden' }, tr)).toMatch(/tilhører en annen bruker/i)
    expect(translateApiError({ code: 'personal_connection_no_delegated_user' }, tr)).toMatch(/logget inn som eieren/i)
  })

  // The org Zero Data Retention switch is refused with these two codes, from the
  // settings page and from organization creation alike. Each needs its own
  // sentence: the generic fallback would tell an admin "something went wrong"
  // about a refusal that is deliberate and has a way forward.
  it('gives the org ZDR switch refusals their own copy, in both languages', () => {
    const no = (norwegian: string) => norwegian
    const en = (_norwegian: string, english: string) => english

    expect(translateApiError({ code: 'zdr_no_attested_provider' }, no)).toMatch(/ingen AI-leverandør har en registrert nullbevaringsgodkjenning/i)
    expect(translateApiError({ code: 'zdr_no_attested_provider' }, en)).toMatch(/no AI provider has a recorded zero-retention attestation/i)
    expect(translateApiError({ code: 'zdr_gate_unavailable' }, no)).toMatch(/Kunne ikke kontrollere/i)
    expect(translateApiError({ code: 'zdr_gate_unavailable' }, en)).toMatch(/could not check which providers/i)
    expect(translateApiError({ code: 'zdr_no_attested_provider' }, no)).not.toMatch(/Noe gikk galt/)
  })
})
