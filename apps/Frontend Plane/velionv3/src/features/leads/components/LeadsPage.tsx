import { createResource, createSignal, For, Show } from 'solid-js'
import {
  createLeadList,
  deleteLeadList,
  getLeadList,
  leadExportHref,
  listLeadLists,
  searchLeads,
  type LeadCompany,
  type SavedLeadList,
} from '@/shared/api/leads-client'

/** Lead-builder: filtered Enhetsregisteret company search → save a named list →
 *  table → metered CSV export. Company data only; no person/role data is ever
 *  fetched or shown. */
export function LeadsPage() {
  // ── Search filters ──────────────────────────────────────────────────────
  const [naeringskode, setNaeringskode] = createSignal('')
  const [kommunenummer, setKommunenummer] = createSignal('')
  const [organisasjonsform, setOrganisasjonsform] = createSignal('')
  const [fraAnsatte, setFraAnsatte] = createSignal('')
  const [tilAnsatte, setTilAnsatte] = createSignal('')

  const [results, setResults] = createSignal<LeadCompany[]>([])
  const [selected, setSelected] = createSignal<Record<string, LeadCompany>>({})
  const [searching, setSearching] = createSignal(false)
  const [searchError, setSearchError] = createSignal<string | null>(null)
  const [searched, setSearched] = createSignal(false)

  const [listName, setListName] = createSignal('')
  const [saveMessage, setSaveMessage] = createSignal<string | null>(null)

  const [lists, { refetch: refetchLists }] = createResource(listLeadLists)
  const [openList, setOpenList] = createSignal<SavedLeadList | null>(null)

  function num(value: string): number | undefined {
    const n = Number.parseInt(value, 10)
    return Number.isFinite(n) ? n : undefined
  }

  async function runSearch(e: Event) {
    e.preventDefault()
    setSearchError(null)
    setSaveMessage(null)
    setSearching(true)
    setSearched(true)
    try {
      const page = await searchLeads({
        naeringskode: naeringskode().trim() || undefined,
        kommunenummer: kommunenummer().trim() || undefined,
        organisasjonsform: organisasjonsform().trim() || undefined,
        fra_antall_ansatte: num(fraAnsatte()),
        til_antall_ansatte: num(tilAnsatte()),
        size: 50,
      })
      setResults(page.companies ?? [])
      setSelected({})
    } catch (err) {
      // Enhetsregisteret rejects employee filters in the 1–4 band and deep paging
      // beyond 10k — the gateway surfaces these as a clear message (422).
      setSearchError(err instanceof Error ? err.message : 'Search failed. Please adjust the filters and retry.')
      setResults([])
    } finally {
      setSearching(false)
    }
  }

  function toggle(company: LeadCompany) {
    setSelected((prev) => {
      const next = { ...prev }
      if (next[company.organisasjonsnummer]) {
        delete next[company.organisasjonsnummer]
      } else {
        next[company.organisasjonsnummer] = company
      }
      return next
    })
  }

  const selectedCount = () => Object.keys(selected()).length

  async function saveList(e: Event) {
    e.preventDefault()
    setSaveMessage(null)
    const companies = Object.values(selected())
    if (!listName().trim() || companies.length === 0) {
      setSaveMessage('Name the list and select at least one company.')
      return
    }
    try {
      const list = await createLeadList(listName().trim(), companies)
      setSaveMessage(`Saved “${list.name}” (${list.company_count} companies).`)
      setListName('')
      setSelected({})
      await refetchLists()
    } catch (err) {
      setSaveMessage(err instanceof Error ? err.message : 'Could not save the list. Please retry.')
    }
  }

  async function open(list: SavedLeadList) {
    try {
      setOpenList(await getLeadList(list.id))
    } catch {
      setOpenList(null)
    }
  }

  async function remove(list: SavedLeadList) {
    await deleteLeadList(list.id)
    if (openList()?.id === list.id) setOpenList(null)
    await refetchLists()
  }

  return (
    <section class="velion-leads" aria-label="Lead builder">
      <header class="velion-leads__head">
        <h1>Leads</h1>
        <p class="velion-leads__sub">
          Find Norwegian companies in Enhetsregisteret by industry, location, and size. Company data only —
          no contacts or named persons.
        </p>
      </header>

      <form class="velion-leads__filters" onSubmit={runSearch}>
        <label>
          Næringskode<input value={naeringskode()} onInput={(e) => setNaeringskode(e.currentTarget.value)} placeholder="e.g. 10.209" />
        </label>
        <label>
          Kommunenummer<input value={kommunenummer()} onInput={(e) => setKommunenummer(e.currentTarget.value)} placeholder="e.g. 4601" />
        </label>
        <label>
          Organisasjonsform<input value={organisasjonsform()} onInput={(e) => setOrganisasjonsform(e.currentTarget.value)} placeholder="e.g. AS" />
        </label>
        <label>
          Min. ansatte<input type="number" min="0" value={fraAnsatte()} onInput={(e) => setFraAnsatte(e.currentTarget.value)} placeholder="0 or ≥5" />
        </label>
        <label>
          Maks. ansatte<input type="number" min="0" value={tilAnsatte()} onInput={(e) => setTilAnsatte(e.currentTarget.value)} />
        </label>
        <button type="submit" disabled={searching()}>{searching() ? 'Searching…' : 'Search'}</button>
      </form>
      <p class="velion-leads__hint">Enhetsregisteret cannot filter employee counts of 1–4; use 0 or 5+.</p>

      <Show when={searchError()}>
        <p class="velion-leads__error" role="alert">{searchError()}</p>
      </Show>

      <Show when={searched() && !searching() && !searchError()}>
        <Show
          when={results().length > 0}
          fallback={<p class="velion-leads__muted">No companies matched these filters.</p>}
        >
          <form class="velion-leads__save" onSubmit={saveList}>
            <input value={listName()} onInput={(e) => setListName(e.currentTarget.value)} placeholder="Name this list" aria-label="List name" />
            <button type="submit" disabled={selectedCount() === 0}>Save {selectedCount()} as list</button>
            <Show when={saveMessage()}><span class="velion-leads__note" role="status">{saveMessage()}</span></Show>
          </form>

          <table class="velion-leads__table">
            <thead>
              <tr>
                <th aria-label="Select" />
                <th>Navn</th><th>Org.nr</th><th>Form</th><th>Næring</th><th>Sted</th><th>Ansatte</th>
              </tr>
            </thead>
            <tbody>
              <For each={results()}>
                {(c) => (
                  <tr>
                    <td>
                      <input
                        type="checkbox"
                        checked={!!selected()[c.organisasjonsnummer]}
                        onChange={() => toggle(c)}
                        aria-label={`Select ${c.navn}`}
                      />
                    </td>
                    <td>{c.navn}</td>
                    <td>{c.organisasjonsnummer}</td>
                    <td>{c.organisasjonsform ?? ''}</td>
                    <td>{c.naering_beskrivelse ?? c.naeringskode ?? ''}</td>
                    <td>{c.poststed ?? ''}</td>
                    <td>{c.antall_ansatte ?? '—'}</td>
                  </tr>
                )}
              </For>
            </tbody>
          </table>
        </Show>
      </Show>

      <section class="velion-leads__lists" aria-label="Saved lists">
        <h2>Saved lists</h2>
        <Show when={!lists.loading} fallback={<p class="velion-leads__muted">Loading…</p>}>
          <Show when={(lists() ?? []).length > 0} fallback={<p class="velion-leads__muted">No saved lists yet.</p>}>
            <ul class="velion-leads__listrows">
              <For each={lists()}>
                {(list) => (
                  <li class="velion-leads__listrow">
                    <button type="button" class="velion-leads__listname" onClick={() => open(list)}>
                      {list.name} <span class="velion-leads__count">{list.company_count}</span>
                    </button>
                    <div class="velion-leads__listactions">
                      {/* Metered: the gateway gates this on the org's billing entitlement. */}
                      <a class="velion-leads__btn" href={leadExportHref(list.id)} download={`${list.name}.csv`}>Export CSV</a>
                      <button type="button" class="velion-leads__btn velion-leads__btn--danger" onClick={() => remove(list)}>Delete</button>
                    </div>
                  </li>
                )}
              </For>
            </ul>
          </Show>
        </Show>

        <Show when={openList()}>
          {(list) => (
            <div class="velion-leads__viewer">
              <h3>{list().name} — {list().company_count} companies</h3>
              <table class="velion-leads__table">
                <thead><tr><th>Navn</th><th>Org.nr</th><th>Sted</th><th>Ansatte</th></tr></thead>
                <tbody>
                  <For each={list().companies ?? []}>
                    {(c) => (
                      <tr>
                        <td>{c.navn}</td><td>{c.organisasjonsnummer}</td><td>{c.poststed ?? ''}</td><td>{c.antall_ansatte ?? '—'}</td>
                      </tr>
                    )}
                  </For>
                </tbody>
              </table>
            </div>
          )}
        </Show>
      </section>
    </section>
  )
}

export default LeadsPage
