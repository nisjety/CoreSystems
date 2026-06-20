import { createResource, createSignal, For, Show } from 'solid-js'
import { no } from '@/shared/i18n/no'
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

const t = no.leads

/** Lead-builder: filtered Enhetsregisteret company search → save a named list →
 *  table → metered CSV export. Company data only; no person/role data is ever
 *  fetched or shown. Norwegian copy is centralized in shared/i18n/no.ts. */
export function LeadsPage() {
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
      setSearchError(err instanceof Error ? err.message : t.searchError)
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
      setSaveMessage(t.saveValidation)
      return
    }
    try {
      const list = await createLeadList(listName().trim(), companies)
      setSaveMessage(t.saved(list.name, list.company_count))
      setListName('')
      setSelected({})
      await refetchLists()
    } catch (err) {
      setSaveMessage(err instanceof Error ? err.message : t.saveError)
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
    <section class="velion-leads" aria-label={t.title}>
      <header class="velion-leads__head">
        <h1>{t.title}</h1>
        <p class="velion-leads__sub">{t.subtitle}</p>
      </header>

      <form class="velion-leads__filters" onSubmit={runSearch}>
        <label>
          {t.naeringskode}<input value={naeringskode()} onInput={(e) => setNaeringskode(e.currentTarget.value)} placeholder="f.eks. 10.209" />
        </label>
        <label>
          {t.kommunenummer}<input value={kommunenummer()} onInput={(e) => setKommunenummer(e.currentTarget.value)} placeholder="f.eks. 4601" />
        </label>
        <label>
          {t.organisasjonsform}<input value={organisasjonsform()} onInput={(e) => setOrganisasjonsform(e.currentTarget.value)} placeholder="f.eks. AS" />
        </label>
        <label>
          {t.minEmployees}<input type="number" min="0" value={fraAnsatte()} onInput={(e) => setFraAnsatte(e.currentTarget.value)} placeholder="0 eller ≥5" />
        </label>
        <label>
          {t.maxEmployees}<input type="number" min="0" value={tilAnsatte()} onInput={(e) => setTilAnsatte(e.currentTarget.value)} />
        </label>
        <button type="submit" disabled={searching()}>{searching() ? t.searching : t.search}</button>
      </form>
      <p class="velion-leads__hint">{t.employeeBandHint}</p>

      <Show when={searchError()}>
        <p class="velion-leads__error" role="alert">{searchError()}</p>
      </Show>

      <Show when={searched() && !searching() && !searchError()}>
        <Show
          when={results().length > 0}
          fallback={<p class="velion-leads__muted">{t.noResults}</p>}
        >
          <form class="velion-leads__save" onSubmit={saveList}>
            <input value={listName()} onInput={(e) => setListName(e.currentTarget.value)} placeholder={t.nameList} aria-label={t.nameList} />
            <button type="submit" disabled={selectedCount() === 0}>{t.saveSelected(selectedCount())}</button>
            <Show when={saveMessage()}><span class="velion-leads__note" role="status">{saveMessage()}</span></Show>
          </form>

          <table class="velion-leads__table">
            <thead>
              <tr>
                <th aria-label="Velg" />
                <th>{t.columns.navn}</th><th>{t.columns.orgnr}</th><th>{t.columns.form}</th><th>{t.columns.naering}</th><th>{t.columns.sted}</th><th>{t.columns.ansatte}</th>
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
                        aria-label={`Velg ${c.navn}`}
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

      <section class="velion-leads__lists" aria-label={t.savedLists}>
        <h2>{t.savedLists}</h2>
        <Show when={!lists.loading} fallback={<p class="velion-leads__muted">{t.loading}</p>}>
          <Show when={(lists() ?? []).length > 0} fallback={<p class="velion-leads__muted">{t.noSavedLists}</p>}>
            <ul class="velion-leads__listrows">
              <For each={lists()}>
                {(list) => (
                  <li class="velion-leads__listrow">
                    <button type="button" class="velion-leads__listname" onClick={() => open(list)}>
                      {list.name} <span class="velion-leads__count">{list.company_count}</span>
                    </button>
                    <div class="velion-leads__listactions">
                      {/* Metered: the gateway gates this on the org's billing entitlement. */}
                      <a class="velion-leads__btn" href={leadExportHref(list.id)} download={`${list.name}.csv`}>{t.exportCsv}</a>
                      <button type="button" class="velion-leads__btn velion-leads__btn--danger" onClick={() => remove(list)}>{t.delete}</button>
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
              <h3>{list().name} — {t.companies(list().company_count)}</h3>
              <table class="velion-leads__table">
                <thead><tr><th>{t.columns.navn}</th><th>{t.columns.orgnr}</th><th>{t.columns.sted}</th><th>{t.columns.ansatte}</th></tr></thead>
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
