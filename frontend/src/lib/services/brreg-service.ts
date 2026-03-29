/**
 * Norwegian Enhetsregisteret (Brreg) service.
 * All requests are proxied through /api/org → org-core → data.brreg.no
 */

export interface BrregAddress {
  land: string
  landkode: string
  postnummer: string
  poststed: string
  adresse: string[]
  kommune: string
  kommunenummer: string
}

export interface BrregOrgForm {
  kode: string
  beskrivelse: string
}

export interface BrregNaeringskode {
  kode: string
  beskrivelse: string
}

export interface BrregEnhet {
  organisasjonsnummer: string
  navn: string
  organisasjonsform?: BrregOrgForm
  forretningsadresse?: BrregAddress
  postadresse?: BrregAddress
  naeringskode1?: BrregNaeringskode
  antallAnsatte?: number
  stiftelsesdato?: string
  hjemmeside?: string
  epostadresse?: string
  telefon?: string
  konkurs: boolean
  underAvvikling: boolean
  registreringsdatoEnhetsregisteret?: string
}

export interface BrregSearchResponse {
  results: BrregEnhet[]
  count: number
}

const ORG_PROXY_BASE = '/api/org/api/v1'

export const brregService = {
  /**
   * Search Enhetsregisteret by organisation name.
   */
  async searchByName(name: string, size = 10): Promise<BrregEnhet[]> {
    const params = new URLSearchParams({ q: name, size: String(size) })
    const res = await fetch(`${ORG_PROXY_BASE}/brreg/search?${params}`)
    if (!res.ok) {
      const err = await res.json().catch(() => ({ error: 'Unknown error' }))
      throw new Error(err.error ?? `Brreg search failed (${res.status})`)
    }
    const data: BrregSearchResponse = await res.json()
    return data.results
  },

  /**
   * Look up a single organisation by 9-digit org number.
   */
  async lookupByOrgNr(orgnr: string): Promise<BrregEnhet | null> {
    const res = await fetch(`${ORG_PROXY_BASE}/brreg/${orgnr}`)
    if (res.status === 404) return null
    if (!res.ok) {
      const err = await res.json().catch(() => ({ error: 'Unknown error' }))
      throw new Error(err.error ?? `Brreg lookup failed (${res.status})`)
    }
    return res.json()
  },
}

/** Format a Brreg address into a single readable line. */
export function formatBrregAddress(addr?: BrregAddress): string {
  if (!addr) return ''
  const lines = addr.adresse?.filter(Boolean) ?? []
  const postLine = [addr.postnummer, addr.poststed].filter(Boolean).join(' ')
  return [...lines, postLine].filter(Boolean).join(', ')
}
