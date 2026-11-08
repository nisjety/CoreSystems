import type { BrowserProfileBrowsingDataCounts, BrowserProfileListResponse } from '@/shared/api/browser-client'

type Tr = (noText: string, enText: string) => string

/** Whether a clear would remove nothing. */
export function browsingDataIsEmpty(counts: BrowserProfileBrowsingDataCounts): boolean {
  return (
    counts.cookies === 0 &&
    counts.local_storage_entries === 0 &&
    counts.session_storage_entries === 0 &&
    counts.indexed_db_rows === 0 &&
    counts.origins === 0
  )
}

/** The clear control is shown only when the edge's profile list says it is served (F-t2-068). */
export function clearDataAvailable(list: Partial<BrowserProfileListResponse> | null | undefined): boolean {
  return list?.clear_data_enabled === true
}

/** What the confirm step says a clear would remove, in the page's language. */
export function browsingDataSummary(counts: BrowserProfileBrowsingDataCounts, tr: Tr): string {
  if (browsingDataIsEmpty(counts)) {
    return tr('Profilen har ingen nettleserdata å tømme.', 'This profile has no browsing data to clear.')
  }
  const parts: string[] = []
  if (counts.cookies > 0) {
    parts.push(countOf(counts.cookies, tr('informasjonskapsel', 'cookie'), tr('informasjonskapsler', 'cookies')))
  }
  const storage = counts.local_storage_entries + counts.session_storage_entries
  if (storage > 0) {
    parts.push(countOf(storage, tr('lagringspost', 'storage entry'), tr('lagringsposter', 'storage entries')))
  }
  if (counts.indexed_db_rows > 0) {
    parts.push(countOf(counts.indexed_db_rows, tr('IndexedDB-post', 'IndexedDB record'), tr('IndexedDB-poster', 'IndexedDB records')))
  }
  if (counts.origins > 0) {
    parts.push(countOf(counts.origins, tr('nettsted', 'site'), tr('nettsteder', 'sites')))
  }
  const list = parts.join(', ')
  return tr(`Fjerner ${list}.`, `Removes ${list}.`)
}

/** Said under the counts: the profile is kept, and an open session on it must close first. */
export function clearKeepsProfileNote(tr: Tr): string {
  return tr(
    'Profilen, navnet og innstillingene beholdes. Lukk åpne nettleserøkter på profilen først.',
    'The profile, its name and its settings are kept. Close any open browser session on this profile first.',
  )
}

function countOf(value: number, one: string, many: string): string {
  return `${value} ${value === 1 ? one : many}`
}
