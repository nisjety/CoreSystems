import { requestJson } from './http'
import { gatewayBaseUrl } from './config'

// Gateway-facing only. The browser never calls leads-core directly, never sends
// an org header (the gateway resolves the org from the session), and only ever
// receives COMPANY data — leads-core has no person/role/birth-number field.

export type LeadCompany = {
  organisasjonsnummer: string
  navn: string
  organisasjonsform?: string
  naeringskode?: string
  naering_beskrivelse?: string
  kommunenummer?: string
  poststed?: string
  antall_ansatte?: number
  registreringsdato?: string
  hjemmeside?: string
  konkurs?: boolean
  under_avvikling?: boolean
}

export type LeadSearchFilter = {
  naeringskode?: string
  kommunenummer?: string
  organisasjonsform?: string
  fra_antall_ansatte?: number
  til_antall_ansatte?: number
  fra_registreringsdato?: string
  til_registreringsdato?: string
  page?: number
  size?: number
}

export type LeadSearchPage = {
  companies: LeadCompany[]
  page: number
  size: number
  total_elements: number
  total_pages: number
}

export type SavedLeadList = {
  id: string
  org_id: string
  name: string
  company_count: number
  companies?: LeadCompany[]
  created_at: string
  updated_at: string
}

export function searchLeads(filter: LeadSearchFilter): Promise<LeadSearchPage> {
  return requestJson<LeadSearchPage>('/api/v1/leads/search', {
    method: 'POST',
    body: JSON.stringify(filter),
  })
}

export function listLeadLists(): Promise<SavedLeadList[]> {
  return requestJson<SavedLeadList[]>('/api/v1/leads/lists')
}

export function createLeadList(name: string, companies: LeadCompany[]): Promise<SavedLeadList> {
  return requestJson<SavedLeadList>('/api/v1/leads/lists', {
    method: 'POST',
    body: JSON.stringify({ name, companies }),
  })
}

export function getLeadList(id: string): Promise<SavedLeadList> {
  return requestJson<SavedLeadList>(`/api/v1/leads/lists/${encodeURIComponent(id)}`)
}

export function deleteLeadList(id: string): Promise<unknown> {
  return requestJson(`/api/v1/leads/lists/${encodeURIComponent(id)}`, { method: 'DELETE' })
}

/** Same-origin URL for the metered CSV export (gated server-side on the org's
 *  billing entitlement). Used as a session-cookie-authenticated download link. */
export function leadExportHref(id: string): string {
  return `${gatewayBaseUrl()}/api/v1/leads/lists/${encodeURIComponent(id)}/export.csv`
}
