import { describe, expect, it } from 'vitest'
import type { BrowserProfileBrowsingDataCounts } from '@/shared/api/browser-client'
import { browsingDataIsEmpty, browsingDataSummary, clearDataAvailable } from './browser-profile-clear'

const none: BrowserProfileBrowsingDataCounts = {
  cookies: 0,
  local_storage_entries: 0,
  session_storage_entries: 0,
  indexed_db_rows: 0,
  origins: 0,
}

const english = (_noText: string, enText: string) => enText
const norsk = (noText: string, _enText: string) => noText

describe('clear browsing data helpers', () => {
  it('shows the clear control only when the list says the edge serves it', () => {
    expect(clearDataAvailable({ profiles: [] })).toBe(false)
    expect(clearDataAvailable({ profiles: [], clear_data_enabled: false })).toBe(false)
    expect(clearDataAvailable({ profiles: [], clear_data_enabled: true })).toBe(true)
    expect(clearDataAvailable(null)).toBe(false)
  })

  it('knows that a profile with no browsing data has nothing to clear', () => {
    expect(browsingDataIsEmpty(none)).toBe(true)
    expect(browsingDataSummary(none, english)).toBe('This profile has no browsing data to clear.')
    expect(browsingDataSummary(none, norsk)).toBe('Profilen har ingen nettleserdata å tømme.')
  })

  it('counts in the page language, with the singular and the plural forms', () => {
    const counts = { ...none, cookies: 1, local_storage_entries: 2, origins: 1 }
    expect(browsingDataIsEmpty(counts)).toBe(false)
    expect(browsingDataSummary(counts, english)).toBe('Removes 1 cookie, 2 storage entries, 1 site.')
    expect(browsingDataSummary(counts, norsk)).toBe('Fjerner 1 informasjonskapsel, 2 lagringsposter, 1 nettsted.')
  })

  it('counts IndexedDB records on their own', () => {
    const counts = { ...none, cookies: 2, indexed_db_rows: 5, origins: 2 }
    expect(browsingDataSummary(counts, english)).toBe('Removes 2 cookies, 5 IndexedDB records, 2 sites.')
  })
})
