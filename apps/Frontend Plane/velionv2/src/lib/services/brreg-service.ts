/**
 * Norwegian Enhetsregisteret (Brreg) service.
 * All requests are proxied through /api/org -> org-core -> data.brreg.no.
 * Ported from velion v1 (src/lib/services/brreg-service.ts) for velionv2 parity.
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

interface BrregSearchResponse {
  results: BrregEnhet[]
  count: number
}

const ORG_PROXY_BASE = "/api/org/api/v1"

export const brregService = {
  /**
   * Search Enhetsregisteret by organisation name.
   */
  async searchByName(name: string, size = 10, signal?: AbortSignal): Promise<BrregEnhet[]> {
    const params = new URLSearchParams({ q: name, size: String(size) })
    const res = await fetch(`${ORG_PROXY_BASE}/brreg/search?${params}`, { signal })
    if (!res.ok) {
      const err = await res.json().catch(() => ({ error: "Unknown error" }))
      throw new Error(err.error ?? `Brreg search failed (${res.status})`)
    }
    const data: BrregSearchResponse = await res.json()
    return data.results
  },

  /**
   * Look up a single organisation by 9-digit org number.
   */
  async lookupByOrgNr(orgnr: string, signal?: AbortSignal): Promise<BrregEnhet | null> {
    const res = await fetch(`${ORG_PROXY_BASE}/brreg/${orgnr}`, { signal })
    if (res.status === 404) return null
    if (!res.ok) {
      const err = await res.json().catch(() => ({ error: "Unknown error" }))
      throw new Error(err.error ?? `Brreg lookup failed (${res.status})`)
    }
    return res.json()
  },
}

/** Format a Brreg address into a single readable line. */
export function formatBrregAddress(addr?: Partial<BrregAddress>): string {
  if (!addr) return ""
  const lines = addr.adresse?.filter(Boolean) ?? []
  const postLine = [addr.postnummer, addr.poststed].filter(Boolean).join(" ")
  return [...lines, postLine].filter(Boolean).join(", ")
}

/** Derive an org "size" bucket from BRREG employee count. */
export function sizeFromEmployeeCount(count?: number): string {
  if (!count || count <= 1) return "1"
  if (count <= 10) return "2-10"
  if (count <= 50) return "11-50"
  if (count <= 200) return "51-200"
  if (count <= 1000) return "201-1000"
  return "1000+"
}

/** Slugify an organisation name into a URL-safe slug. */
export function slugify(value: string): string {
  return value
    .toLowerCase()
    .normalize("NFKD")
    .replace(/[̀-ͯ]/g, "")
    .replace(/[æ]/g, "ae")
    .replace(/[ø]/g, "o")
    .replace(/[å]/g, "a")
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 48)
}
