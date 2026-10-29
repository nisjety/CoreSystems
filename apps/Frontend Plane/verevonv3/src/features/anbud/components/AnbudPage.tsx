import { createEffect, createMemo, createSignal, For, on, onCleanup, onMount, Show } from 'solid-js'
import {
  fetchRetenders,
  searchTenderCatalogue,
  type TenderCatalogue,
  type TenderCatalogueRequest,
  searchTenderMatches,
  type Retenders,
  type TenderMatches,
  type TenderMatchRequest,
  type TenderStrictness,
} from '@/shared/api/anbud-client'
import { ApiError } from '@/shared/api/http'
import { getSession } from '@/shared/session/session-store'
import { translateApiError, useI18n } from '@/shared/i18n'
import { anbudState, anbudWorkspace, anbudFilters, anbudDetail } from '@/features/anbud/lib/anbud-state'
import { createLatestRequest } from '@/features/anbud/lib/latest'
import { parseProfile, type ProfileProblem } from '@/features/anbud/lib/profile'
import { RetenderRadar } from '@/features/anbud/components/RetenderRadar'
import { ArrowUpRight, Building2, CalendarDays, Columns3, Download, Inbox, LayoutGrid, List, Search, Settings2, SlidersHorizontal } from 'lucide-solid'
import { anbudStages as stageOptions } from '@/features/anbud/lib/pipeline'
import { TenderDetailView } from '@/features/anbud/components/TenderDetailView'
import { TenderCatalogueTable } from '@/features/anbud/components/TenderCatalogueTable'
import { defaultTenderColumns, plannedNoticeLabel, searchMatches, tenderColumns, type TenderColumnVisibility } from '@/features/anbud/lib/notice-presentation'
import { TenderFiltersPanel } from '@/features/anbud/components/TenderFiltersPanel'
import { handleTabKeyDown } from '@/shared/ui/tab-keyboard'
import { TenderProfileForm } from '@/features/anbud/components/TenderProfileForm'
import { loadWorkspace, markNewTendersSeen, saveProfile, setPipelineStage, PIPELINE_UPDATED_EVENT } from '@/features/anbud/lib/workspace-sync'
import { fitReason } from '@/features/anbud/lib/fit-reason'
import { newSince } from '@/features/anbud/lib/new-tenders'
import { downloadFile, toCalendar, toCsv, type ExportRow } from '@/features/anbud/lib/export'

export type AnbudSection = 'matches' | 'radar'

/**
 * Runs one call for the page. A newer call aborts the older one, and only the newest answer is applied: an answer
 * that arrives after a newer call started is dropped, so a stale result never lands on a changed search.
 */
function startCall<T>(
  guard: ReturnType<typeof createLatestRequest>,
  previous: AbortController | undefined,
  call: (signal: AbortSignal) => Promise<T>,
  onAnswer: (value: T) => void,
  onFailure: (err: unknown) => void,
): AbortController {
  previous?.abort()
  const controller = new AbortController()
  const token = guard.next()
  call(controller.signal).then(
    (value) => {
      if (guard.isCurrent(token)) onAnswer(value)
    },
    (err: unknown) => {
      if (guard.isCurrent(token) && !controller.signal.aborted) onFailure(err)
    },
  )
  return controller
}

/** Tender matching: a company profile, the matches it gets, each tender's audit sheet, and the re-tender radar. */
export default function AnbudPage(props: { section: AnbudSection }) {
  const { tr } = useI18n()
  const orgId = () => getSession().activeOrg?.id ?? ''

  const matchGuard = createLatestRequest()
  const radarGuard = createLatestRequest()
  let matchAbort: AbortController | undefined
  let radarAbort: AbortController | undefined
  let workspaceController = new AbortController()

  const { matches, setMatches, view, setView, profileOpen, setProfileOpen, stage, stages, loadFailed, setLoadFailed, persistFailed, profileSuggestion, lastSeen } = anbudWorkspace
  const [catalogue, setCatalogue] = createSignal<TenderCatalogue | null>(null)
  const [catalogueOwner, setCatalogueOwner] = createSignal('')
  // Page-local, bounded and organisation-scoped. Restored results are always
  // revalidated; nothing is written to browser storage.
  const catalogueCache = new Map<string, { data: TenderCatalogue; at: number }>()
  let prefetchAbort: AbortController | undefined
  const catalogueKey = (request: TenderCatalogueRequest, org: string) => JSON.stringify([org, request])
  const cacheCatalogue = (key: string, data: TenderCatalogue) => {
    catalogueCache.delete(key)
    catalogueCache.set(key, { data, at: Date.now() })
    while (catalogueCache.size > 8) catalogueCache.delete(catalogueCache.keys().next().value!)
  }
  const currentCatalogue = () => catalogueOwner() === orgId() ? catalogue() : null
  const [cursor, setCursor] = createSignal('')
  const [previousCursors, setPreviousCursors] = createSignal<string[]>([])
  const resultData = () => view() === 'all' ? currentCatalogue() : matches()
  const [pipelineCursor, setPipelineCursor] = createSignal('')
  const [pipelineNotice, setPipelineNotice] = createSignal<string | null>(null)
  const pipelinePages = new Map<string, TenderMatches>()
  let pipelinePrefetch: AbortController | undefined
  let pipelineEpoch = 0
  const [matchesBusy, setMatchesBusy] = createSignal(false)
  const [matchesError, setMatchesError] = createSignal<string | null>(null)
  const [radar, setRadar] = createSignal<Retenders | null>(null)
  const [radarBusy, setRadarBusy] = createSignal(false)
  const [radarError, setRadarError] = createSignal<string | null>(null)
  const [query, setQuery] = createSignal('')
  const [filtersOpen, setFiltersOpen] = createSignal(false)
  const { filters } = anbudFilters
  const [display, setDisplay] = createSignal<'list' | 'cards'>('list')
  const [columnsOpen, setColumnsOpen] = createSignal(false)
  const [columns, setColumns] = createSignal<TenderColumnVisibility>({ ...defaultTenderColumns })
  const [sort, setSort] = createSignal('deadline')
  const [direction, setDirection] = createSignal('asc')
  const sortColumn = (key: string) => { if (key === sort()) setDirection(value => value === 'asc' ? 'desc' : 'asc'); else { setSort(key); setDirection(key === 'published' || key === 'issued' ? 'desc' : 'asc') } }
  const visible = createMemo(() => {
    const sortBy = sort()
    if (view() === 'all') return currentCatalogue()?.results ?? []
    return (resultData()?.results ?? []).filter(item => {
    if (view() === 'pipeline' && (stages()[item.noticeId] ?? 'inbox') !== stage()) return false
    if (view() === 'pipeline' && stage() === 'inbox' && matches()?.page) return true
    if (view() !== 'all' && !item.retained && filters().verdict !== 'all' && item.verdict !== filters().verdict) return false
    if (!item.retained && filters().deadline === 'soon' && (!item.deadline || item.deadline.daysLeft < 0 || item.deadline.daysLeft > 14)) return false
    if (!item.retained && filters().status === 'active' && (!item.deadline || item.deadline.daysLeft < 0)) return false
    if (!item.retained && filters().status === 'expired' && (!item.deadline || item.deadline.daysLeft >= 0)) return false
    if (filters().buyer && !searchMatches(item.buyer ?? '', filters().buyer)) return false
    if (filters().region && !item.nuts.some(code => code.toLocaleLowerCase().startsWith(filters().region.toLocaleLowerCase()))) return false
    if (filters().cpv && !item.cpv.some(code => code.startsWith(filters().cpv))) return false
    return searchMatches(`${item.title} ${item.buyer ?? ''}`, query())
  }).sort((a, b) => (view() === 'pipeline' && stage() === 'inbox' && matches()?.page) || sortBy === 'relevance' ? 0 : sortBy === 'title' ? a.title.localeCompare(b.title) : (a.deadline?.daysLeft ?? Infinity) - (b.deadline?.daysLeft ?? Infinity))
  })
  function loadCatalogue(nextCursor = '', history: string[] = []) {
    setCursor(nextCursor)
    setPreviousCursors(history)
    setMatchesBusy(true)
    setMatchesError(null)
    anbudFilters.setFacetsBusy(true)
    const { buyer, region, cpv, status, deadline, noticeType } = filters()
    const order = sort()
    const orderDirection = direction()
    const searchText = query()
    const request: TenderCatalogueRequest = { cursor: nextCursor || undefined, query: searchText, buyer, region, cpv, status, deadline, noticeType, sort: order, direction: orderDirection }
    const org = orgId()
    const key = catalogueKey(request, org)
    const cached = catalogueCache.get(key)
    if (cached && Date.now() - cached.at < 60_000) {
      setCatalogueOwner(org); setCatalogue(cached.data)
      anbudFilters.setFacets(cached.data.facets ?? {})
    }
    prefetchAbort?.abort()
    matchAbort = startCall(matchGuard, matchAbort,
      signal => searchTenderCatalogue(request, signal),
      value => {
        if (org !== orgId()) return
        cacheCatalogue(key, value)
        setCatalogueOwner(org); setCatalogue(value)
        anbudFilters.setFacets(value.facets ?? {}); anbudFilters.setFacetsBusy(false); setMatchesBusy(false)
        if (value.nextCursor) {
          const nextRequest = { ...request, cursor: value.nextCursor }
          const nextKey = catalogueKey(nextRequest, org)
          if (!catalogueCache.has(nextKey)) {
            const controller = new AbortController()
            prefetchAbort = controller
            void searchTenderCatalogue(nextRequest, controller.signal).then(next => {
              if (!controller.signal.aborted && org === orgId()) cacheCatalogue(nextKey, next)
            }, () => { /* Foreground navigation retries a failed prefetch. */ })
          }
        }
      },
      error => {
        if (org !== orgId()) return
        setMatchesError(failureText(error)); anbudFilters.setFacetsBusy(false); setMatchesBusy(false)
      },
    )
  }
  createEffect(on(orgId, () => {
    matchAbort?.abort(); prefetchAbort?.abort(); matchGuard.next()
    catalogueCache.clear(); setCatalogue(null); setCatalogueOwner('')
    pipelinePrefetch?.abort(); pipelineEpoch++; pipelinePages.clear(); setPipelineCursor('')
    workspaceController.abort(); workspaceController = new AbortController()
    setMatches(null); anbudWorkspace.setStages({}); anbudState.setSubmitted(null)
    const org = orgId()
    void loadWorkspace(workspaceController.signal).then(() => { if (orgId() === org && view() === 'pipeline') loadMatches() }, () => { if (orgId() === org) setLoadFailed(true) })
    if (view() === 'all') loadCatalogue()
  }, { defer: true }))
  createEffect(on([query, filters, sort, direction, stage], () => {
    if (view() !== 'all' && view() !== 'pipeline') return
    if (view() === 'pipeline' && stage() !== 'inbox') return
    if (view() === 'pipeline') { setMatchesBusy(true); pipelinePrefetch?.abort() }
    // This callback deliberately checks the active view when the debounce expires.
    // eslint-disable-next-line solid/reactivity
    const timer = window.setTimeout(() => { if (view() === 'all') loadCatalogue(); else if (view() === 'pipeline') loadMatches() }, 300)
    onCleanup(() => window.clearTimeout(timer))
  }, { defer: true }))
  function switchView(next: 'pipeline' | 'all' | 'radar') {
    anbudDetail.close()
    setView(next)

  }
  const [problems, setProblems] = createSignal<ProfileProblem[]>([])
  const newCount = createMemo(() => newSince(matches()?.results ?? [], lastSeen()))

  const failureText = (err: unknown) =>
    translateApiError(err, tr, {
      no: 'Søket kunne ikke fullføres. Prøv igjen.',
      en: 'The search could not be completed. Try again.',
    })

  const requestFor = (): TenderMatchRequest | null => {
    const profile = anbudState.submitted()
    return profile ? { profile, strictness: anbudState.strictness(), ...(view() === 'all' ? { show: 'all' as const } : {}) } : view() === 'pipeline' && Object.values(stages()).some(value => ['review', 'working', 'sent', 'hidden'].includes(value)) ? { profile: { cpv: [], nuts: [], keywords: [] }, strictness: anbudState.strictness(), trackedOnly: true } : null
  }

  function applyMatches(value: TenderMatches, trackedOnly: boolean) {
    const merged = new Map((trackedOnly ? [] : value.results).map(item => [item.noticeId, item]))
    for (const item of value.trackedResults ?? []) merged.set(item.noticeId, item)
    setMatches({ ...value, results: [...merged.values()] })
  }
  function loadMatches(nextCursor = '', scrollToTop = false) {
    const base = requestFor()
    if (!base) return
    const org = orgId()
    const request: TenderMatchRequest = { ...base, ...(base.trackedOnly ? {} : {
      paginate: true, cursor: nextCursor || undefined,
      sort: sort() === 'title' ? 'title' : sort() === 'deadline' ? 'deadline' : 'relevance',
      filters: { query: query(), buyer: filters().buyer, region: filters().region, cpv: filters().cpv, status: filters().status, deadline: filters().deadline, verdict: filters().verdict },
    }) }
    if (!nextCursor) { pipelineEpoch++; pipelinePages.clear(); setPipelineNotice(null) }
    const epoch = pipelineEpoch
    setPipelineCursor(nextCursor)
    pipelinePrefetch?.abort()
    const cached = pipelinePages.get(nextCursor)
    if (cached) applyMatches(cached, !!request.trackedOnly)
    setMatchesBusy(true); setMatchesError(null)
    matchAbort = startCall(matchGuard, matchAbort,
      signal => searchTenderMatches(request, signal),
      value => {
        if (org !== orgId() || epoch !== pipelineEpoch) return
        applyMatches(value, !!request.trackedOnly)
        setMatchesBusy(false)
        if (scrollToTop) document.querySelector('.anbud-content-heading')?.scrollIntoView?.({ block: 'start' })
        pipelinePages.set(nextCursor, value)
        while (pipelinePages.size > 3) pipelinePages.delete(pipelinePages.keys().next().value!)
        if (value.nextCursor) {
          const controller = new AbortController()
          pipelinePrefetch = controller
          const cursor = value.nextCursor
          void searchTenderMatches({ ...request, cursor }, controller.signal).then(next => {
            if (!controller.signal.aborted && org === orgId() && epoch === pipelineEpoch) {
              pipelinePages.set(cursor, next)
              while (pipelinePages.size > 3) pipelinePages.delete(pipelinePages.keys().next().value!)
            }
          }, () => { /* A foreground request revalidates the snapshot. */ })
        }
      },
      err => {
        if (org !== orgId() || epoch !== pipelineEpoch) return
        if (nextCursor && err instanceof ApiError && err.code === 'matching_snapshot_expired') {
          pipelinePages.clear(); setMatches(null)
          loadMatches()
          setPipelineNotice(tr('Treffene er oppdatert. Du er tilbake på første side.', 'Results have changed. You are back on the first page.'))
          return
        }
        setMatchesError(failureText(err)); setMatchesBusy(false)
      },
    )
  }

  function loadRadar() {
    const request = requestFor()
    if (!request) return
    setRadarBusy(true)
    setRadarError(null)
    radarAbort = startCall(
      radarGuard,
      radarAbort,
      (signal) => fetchRetenders({ profile: request.profile, strictness: request.strictness, horizonMonths: 12 }, signal),
      (value: Retenders) => {
        setRadar(value)
        setRadarBusy(false)
      },
      (err) => {
        setRadarError(failureText(err))
        setRadarBusy(false)
      },
    )
  }

  // The export writes the rows the page shows, in the order it shows them, with the pipeline stage each has.
  function exportRows(): ExportRow[] {
    return visible().map((item) => ({
      id: item.noticeId,
      title: item.title,
      buyer: item.buyer ?? '',
      deadline: item.deadline?.osloDay ?? '',
      plannedNotice: item.plannedNotice ?? '',
      cpv: item.cpv.join(', '),
      nuts: item.nuts.join(', '),
      stage: stageOptions.find(s => s.id === (stages()[item.noticeId] ?? 'inbox'))?.no ?? '',
    }))
  }

  function exportCsv() {
    downloadFile(`anbud-${new Date().toISOString().slice(0, 10)}.csv`, 'text/csv;charset=utf-8', toCsv(exportRows()))
  }

  function exportCalendar() {
    const deadlines = visible().flatMap(item => item.deadline ? [{ uid: `${item.noticeId}@anbud`, day: item.deadline.osloDay, summary: `Anbudsfrist: ${item.title}`, description: [item.buyer, item.noticeId].filter(Boolean).join(' · ') }] : [])
    downloadFile('anbud-frister.ics', 'text/calendar;charset=utf-8', toCalendar(deadlines, new Date()))
  }

  function openAudit(noticeId: string) {
    const notice = resultData()?.results.find(item => item.noticeId === noticeId)
    if (notice) anbudDetail.open(notice)
  }

  function reload() {
    if (view() === 'all') loadCatalogue()
    else if (view() !== 'radar') loadMatches()
    else loadRadar()
  }

  function submit(event: SubmitEvent) {
    event.preventDefault()
    const parsed = parseProfile(anbudState.draft())
    if ('problems' in parsed) {
      setProblems(parsed.problems)
      return
    }
    setProblems([])
    anbudState.setSubmitted(parsed.profile)
    saveProfile()
    setProfileOpen(false)
    reload()
  }

  function chooseStrictness(step: TenderStrictness) {
    if (step === anbudState.strictness()) return
    anbudState.setStrictness(step)
    if (!anbudState.submitted()) return
    saveProfile()
    reload()
  }

  createEffect(on(view, () => {
    setFiltersOpen(false)
    if (view() === 'pipeline') { setSort('relevance'); setDirection('asc') }
    if (view() === 'all' && sort() === 'relevance') setSort('deadline')
    if (view() === 'all' || view() === 'pipeline' || anbudState.submitted()) reload()
  }, { defer: true }))

  onMount(() => {
    const refreshStage = () => { if (view() === 'pipeline') loadMatches() }
    window.addEventListener(PIPELINE_UPDATED_EVENT, refreshStage)
    onCleanup(() => window.removeEventListener(PIPELINE_UPDATED_EVENT, refreshStage))
    if (view() === 'pipeline') setSort('relevance')
    if (props.section === 'radar') setView('radar')
    else if (view() === 'radar') setView('all')
    if (view() === 'all' || view() === 'pipeline' || anbudState.submitted()) reload()
    // Read current state when the polling callback runs.
    // eslint-disable-next-line solid/reactivity
    const refreshTimer = window.setInterval(() => {
      if (view() === 'pipeline' && !anbudDetail.id() && !document.hidden && !matchesBusy()) loadMatches(matches()?.page && matches()!.page! > 1 ? pipelineCursor() : '')
    }, 60_000)
    onCleanup(() => window.clearInterval(refreshTimer))
    loadWorkspace(workspaceController.signal).then(
      // The answer is read when it arrives, against the view the page is in then.
      // eslint-disable-next-line solid/reactivity
      (applied) => {
        setLoadFailed(false)
        if (view() === 'pipeline' || (applied && view() !== 'all')) reload()
      },
      () => {
        if (workspaceController.signal.aborted) return
        setLoadFailed(true)
      },
    )
  })

  onCleanup(() => {
    matchAbort?.abort()
    prefetchAbort?.abort()
    pipelinePrefetch?.abort(); pipelinePages.clear()
    catalogueCache.clear()
    radarAbort?.abort()
    workspaceController.abort()
  })

  const PipelinePagination = (props: { position: 'top' | 'bottom' }) => (
    <Show when={view() === 'pipeline' && stage() === 'inbox' && matches()?.page}>
              <nav class="anbud-pagination" aria-label={props.position === 'top' ? tr('Sider med matchende anbud øverst', 'Matching tender pages at top') : tr('Sider med matchende anbud', 'Matching tender pages')} aria-busy={matchesBusy()}>
                <span>{tr(`Side ${matches()?.page} av ${Math.max(1, Math.ceil((matches()?.totalMatched ?? 0) / (matches()?.pageSize ?? 50)))}`, `Page ${matches()?.page} of ${Math.max(1, Math.ceil((matches()?.totalMatched ?? 0) / (matches()?.pageSize ?? 50)))}`)}</span>
                <button type="button" class="anbud-button" disabled={matchesBusy() || !matches()?.previousCursor} onClick={() => loadMatches(matches()!.previousCursor!, true)}>{tr('Forrige', 'Previous')}</button>
                <button type="button" class="anbud-button" disabled={matchesBusy() || !matches()?.nextCursor} onClick={() => loadMatches(matches()!.nextCursor!, true)}>{tr('Neste', 'Next')}</button>
              </nav>
            </Show>
  )

  return (
    <section class="verevon-anbud anbud-workspace">
      <nav class="verevon-support-switcher anbud-switcher" aria-label={tr('Anbudvisning', 'Tender view')} role="tablist">
        <For each={[{ id: 'all' as const, no: 'Alle anbud', en: 'All tenders' }, { id: 'radar' as const, no: 'Radar', en: 'Radar' }, { id: 'pipeline' as const, no: 'Oppfølging', en: 'Pipeline' }]}>{item =>
          <button type="button" role="tab" aria-selected={view() === item.id} tabIndex={view() === item.id ? 0 : -1} onKeyDown={handleTabKeyDown} classList={{ 'verevon-support-switcher__item--active': view() === item.id }} onClick={() => switchView(item.id)}>{tr(item.no, item.en)}</button>
        }</For>
      </nav>
      <Show when={orgId()} fallback={<p role="status">{tr('Velg en organisasjon først for å søke etter anbud.', 'Select an organization first to search for tenders.')}</p>}>
        <Show when={loadFailed()}><p role="alert" class="verevon-anbud__state--error">{tr('Kundeprofil og pipeline kunne ikke hentes. Last siden på nytt.', 'The company profile and pipeline could not be loaded. Reload the page.')}</p></Show>
        <Show when={persistFailed()}><p role="alert" class="verevon-anbud__state--error">{tr('Endringen ble ikke lagret. Prøv igjen.', 'The change was not saved. Try again.')}</p></Show>
        <Show when={profileSuggestion()}><p role="status" class="anbud-suggestion">{tr(`Kundeprofilen er foreslått fra ${profileSuggestion()?.wins} vunne anbud: ${profileSuggestion()?.cpv} CPV-koder og ${profileSuggestion()?.nuts} regioner. Kontroller den og lagre den før du søker.`, `The profile is suggested from ${profileSuggestion()?.wins} tenders won: ${profileSuggestion()?.cpv} CPV codes and ${profileSuggestion()?.nuts} regions. Check it and save it before you search.`)}</p></Show>
        <Show when={profileOpen()}>
          <section class="anbud-profile-editor" aria-label={tr('Rediger kundeprofil', 'Edit company profile')}>
            <header><div><h2>{tr('Kundeprofil', 'Company profile')}</h2><p>{tr('Velg tjenester, geografi og nøkkelord for kunden du søker for.', 'Choose services, geography and keywords for your customer.')}</p></div><button type="button" class="anbud-button" onClick={() => setProfileOpen(false)}>{tr('Lukk', 'Close')}</button></header>
            <TenderProfileForm draft={anbudState.draft} onDraft={anbudState.setDraft} strictness={anbudState.strictness} onStrictness={chooseStrictness} problems={problems()} busy={(!!anbudState.submitted() && matchesBusy()) || radarBusy()} onSubmit={submit} />
          </section>
        </Show>
        <Show when={!anbudDetail.id()} fallback={<TenderDetailView />}><div class="anbud-body">
          <main class="anbud-content">
            <header class="anbud-content-heading"><div><h2>{view() === 'pipeline' ? tr(stageOptions.find(s => s.id === stage())!.no, stageOptions.find(s => s.id === stage())!.en) : view() === 'all' ? tr('Alle anbud', 'All tenders') : tr('Radar', 'Radar')}</h2><p>{view() === 'pipeline' ? stage() === 'inbox' ? tr('Aktive anbud som passer kundeprofilen din.', 'Active tenders that fit your company profile.') : tr('Anbud organisasjonen følger opp, også når match eller frist endres.', 'Tenders your organisation is following up, even when the match or deadline changes.') : view() === 'all' ? tr('Søk i kunngjøringene og finn neste mulighet.', 'Search notices and find your next opportunity.') : tr('Oppdag muligheter før de kunngjøres.', 'Discover opportunities before they are announced.')}</p></div><div class="anbud-view-tools"><Show when={view() === 'all'}><div class="anbud-display-toggle" aria-label={tr('Visningsformat', 'Display format')}><button type="button" aria-pressed={display() === 'list'} onClick={() => setDisplay('list')}><List size={15} />{tr('Liste', 'List')}</button><button type="button" aria-pressed={display() === 'cards'} onClick={() => setDisplay('cards')}><LayoutGrid size={15} />{tr('Kort', 'Cards')}</button></div><div class="anbud-columns"><button type="button" class="anbud-button" disabled={display() !== 'list'} aria-expanded={columnsOpen()} onClick={() => setColumnsOpen(!columnsOpen())}><Columns3 size={15} />{tr('Kolonner', 'Columns')}</button><Show when={columnsOpen() && display() === 'list'}><fieldset class="anbud-columns-menu"><legend>{tr('Synlige kolonner', 'Visible columns')}</legend><For each={tenderColumns}>{col => <label><input type="checkbox" checked={columns()[col.id]} onChange={e => setColumns(prev => ({ ...prev, [col.id]: e.currentTarget.checked }))} />{tr(col.no, col.en)}</label>}</For></fieldset></Show></div><label class="anbud-sort-control"><span>{tr('Sorter', 'Sort')}</span><select class="anbud-button" aria-label={tr('Sorter anbud', 'Sort tenders')} title={tr('Sorterer hele søket', 'Sorts all search results')} value={sort()} onChange={e => setSort(e.currentTarget.value)}><option value="title">{tr('Tittel', 'Title')}</option><For each={tenderColumns}>{column => <option value={column.id}>{tr(column.no, column.en)}</option>}</For></select><select class="anbud-button" aria-label={tr('Sorteringsretning', 'Sort direction')} value={direction()} onChange={e => setDirection(e.currentTarget.value)}><option value="asc">{['published', 'issued', 'deadline', 'endDate'].includes(sort()) ? tr('Eldste først', 'Oldest first') : ['estimatedValue', 'awardedValue', 'duration'].includes(sort()) ? tr('Laveste først', 'Lowest first') : 'A–Å'}</option><option value="desc">{['published', 'issued', 'deadline', 'endDate'].includes(sort()) ? tr('Nyeste først', 'Newest first') : ['estimatedValue', 'awardedValue', 'duration'].includes(sort()) ? tr('Høyeste først', 'Highest first') : 'Å–A'}</option></select></label></Show><button type="button" class="anbud-button" aria-expanded={profileOpen()} onClick={() => setProfileOpen(!profileOpen())}><Settings2 size={16} />{tr('Kundeprofil', 'Company profile')}</button></div></header>
            <Show when={view() !== 'radar'}>
              <div class="anbud-toolbar"><label class="anbud-search"><Search size={18} /><input aria-label={tr('Søk i anbud', 'Search tenders')} placeholder={view() === 'pipeline' ? tr('Søk i pipeline…', 'Search pipeline…') : tr('Søk etter tittel eller innkjøper…', 'Search title or buyer…')} value={query()} onInput={e => setQuery(e.currentTarget.value)} /></label><button type="button" class="anbud-button" aria-expanded={filtersOpen()} onClick={() => setFiltersOpen(!filtersOpen())}><SlidersHorizontal size={16} />{tr('Filter', 'Filters')}</button><button type="button" class="anbud-button" disabled={!visible().length} onClick={exportCsv}><Download size={16} />{tr('Eksporter CSV', 'Export CSV')}</button><button type="button" class="anbud-button" disabled={!visible().some(item => item.deadline)} onClick={exportCalendar}><CalendarDays size={16} />{tr('Frister (.ics)', 'Deadlines (.ics)')}</button><Show when={view() !== 'all'}><select class="anbud-button" aria-label={tr('Sorter anbud', 'Sort tenders')} title={tr('Sorterer hele søket', 'Sorts all search results')} value={sort()} onChange={e => setSort(e.currentTarget.value)}><option value="relevance">{tr('Relevans', 'Relevance')}</option><option value="deadline">{tr('Frist først', 'Deadline first')}</option><option value="title">{tr('Tittel A–Å', 'Title A–Z')}</option></select></Show></div>
              <Show when={filtersOpen()}><div class="anbud-inline-filters"><TenderFiltersPanel /></div></Show>
            </Show>
            <Show when={view() !== 'all' && !anbudState.submitted() && !matches()?.results.some(item => item.retained)}><div class="anbud-empty"><Inbox size={32} /><h3>{tr('Dine relevante anbud starter her', 'Your relevant tenders start here')}</h3><p>{tr('Sett opp kundeprofilen for å se anbud som passer. Du kan deretter søke, filtrere og vurdere mulighetene.', 'Set up a company profile to see matching tenders. Then search, filter and review opportunities.')}</p><button type="button" class="anbud-button anbud-button--primary" onClick={() => setProfileOpen(true)}>{tr('Sett opp kundeprofil', 'Set up company profile')}<ArrowUpRight size={16} /></button></div></Show>
            <Show when={view() === 'radar' && anbudState.submitted()}><RetenderRadar data={radar() ? { ...radar()!, results: radar()!.results.filter(item => (anbudFilters.radarReach() === 'all' || (item.reach ?? 'exact') === anbudFilters.radarReach()) && searchMatches(`${item.title} ${item.buyer ?? ''}`, anbudFilters.radarBuyer())) } : null} busy={radarBusy()} error={radarError()} /></Show>
            <Show when={view() === 'all' || (view() === 'pipeline' && (anbudState.submitted() || matches()?.results.some(item => item.retained)))}>
              <Show when={view() === 'pipeline' && newCount() > 0}><p role="status" class="anbud-new-tenders">{tr(`${newCount()} nye anbud siden ${(lastSeen() ?? '').slice(0, 10)}`, `${newCount()} new tenders since ${(lastSeen() ?? '').slice(0, 10)}`)} <button type="button" class="anbud-button" onClick={markNewTendersSeen}>{tr('Marker som sett', 'Mark as seen')}</button></p></Show>
              <Show when={view() === 'pipeline' && matches()?.coverage === 'all_active'}><p class="anbud-filter-note">{tr(`Vurdert ${matches()?.activeAssessed ?? 0} aktive anbud i hele katalogen. Valgte anbud beholdes i oppfølging.`, `Assessed ${matches()?.activeAssessed ?? 0} active tenders across the full catalogue. Tracked tenders remain in the pipeline.`)}</p></Show><Show when={pipelineNotice() && view() === 'pipeline'}><p role="status" class="anbud-filter-note">{pipelineNotice()}</p></Show><Show when={matchesError()}><p role="alert" class="verevon-anbud__state--error">{matchesError()} <button type="button" class="anbud-button" onClick={reload}>{tr('Prøv igjen', 'Try again')}</button></p></Show>
              <Show when={matchesBusy()}><p role="status">{resultData() ? tr('Oppdaterer treff…', 'Updating results…') : tr('Søker…', 'Searching…')}</p></Show>
              <PipelinePagination position="top" />
              <Show when={resultData()}><p class="anbud-result-count" role="status">{view() === 'all' ? tr(`Viser ${visible().length} av ${catalogue()?.totalMatched ?? catalogue()?.totalStored ?? catalogue()?.results.length ?? 0} treff`, `Showing ${visible().length} of ${catalogue()?.totalMatched ?? catalogue()?.totalStored ?? catalogue()?.results.length ?? 0} results`) : view() === 'pipeline' && stage() === 'inbox' && matches()?.coverage === 'all_active' ? tr(`Viser ${visible().length} av ${matches()?.totalMatched ?? matches()?.funnel.matched ?? 0} relevante treff`, `Showing ${visible().length} of ${matches()?.totalMatched ?? matches()?.funnel.matched ?? 0} relevant matches`) : tr(`Viser ${visible().length} av ${resultData()!.results.length} anbud`, `Showing ${visible().length} of ${resultData()!.results.length} tenders`)}</p><Show when={visible().length} fallback={<div class="anbud-empty"><Search size={28} /><h3>{tr('Ingen anbud å vise', 'No tenders to show')}</h3><p>{tr('Prøv et annet søk, endre filtrene eller velg en annen status.', 'Try another search, change filters or choose a different status.')}</p></div>}>
                <Show when={view() === 'pipeline' || display() === 'cards'} fallback={<TenderCatalogueTable rows={visible()} columns={columns()} sort={sort()} direction={direction()} onSort={sortColumn} onSelect={openAudit} />}>
                  <div class="anbud-cards"><For each={visible()}>{item => <article class="anbud-card"><div class="anbud-card-main"><div class="anbud-card-buyer"><Building2 size={16} />{item.buyer ?? tr('Innkjøper ikke oppgitt', 'Buyer not stated')}</div><h3><button type="button" onClick={() => openAudit(item.noticeId)}>{item.title}</button></h3><div class="anbud-card-badges"><Show when={item.retained}><span class="verevon-anbud__badge">{tr('Beholdt i oppfølging', 'Kept in pipeline')}</span></Show><span class={`verevon-anbud__badge verevon-anbud__badge--${item.verdict}`}>{item.verdict === 'unknown' ? tr('Ikke vurdert', 'Not assessed') : item.band === 'strong' ? tr('Sterk match', 'Strong match') : item.band === 'good' ? tr('God match', 'Good match') : tr('Vurder match', 'Review match')}</span><span>{item.verdict === 'unknown' ? tr('Velg kundeprofil for vurdering', 'Select a company profile to assess') : item.verdict === 'pass' ? tr('Kan by', 'Can bid') : item.verdict === 'conditional' ? tr('Kan by med forbehold', 'Can bid with conditions') : tr('Kan ikke by', 'Cannot bid')}</span></div><Show when={item.retained && item.band === 'none' && item.verdict !== 'unknown'}><p class="anbud-filter-note">{tr('Matcher ikke lenger kundeprofilen. Beholdt fordi anbudet følges opp.', 'No longer matches the company profile. Kept because your company is following it up.')}</p></Show><Show when={item.verdict !== 'unknown'}><p class="anbud-card-reason">{fitReason(item, tr)}</p></Show><p class="anbud-card-meta"><CalendarDays size={15} />{item.deadline ? `${item.deadline.osloDay} · ${tr(`${item.deadline.daysLeft} dager igjen`, `${item.deadline.daysLeft} days left`)}` : plannedNoticeLabel(item, tr) ?? tr('Ingen frist oppgitt', 'No deadline stated')}</p></div><footer><select aria-label={tr(`Status for ${item.title}`, `Status for ${item.title}`)} value={stages()[item.noticeId] ?? 'inbox'} onChange={e => setPipelineStage(item.noticeId, e.currentTarget.value)}><For each={stageOptions}>{s => <option value={s.id}>{tr(s.no, s.en)}</option>}</For></select><button type="button" class="anbud-button anbud-button--primary" onClick={() => openAudit(item.noticeId)}>{tr('Åpne anbud', 'Open tender')}<ArrowUpRight size={15} /></button></footer></article>}</For></div>
                </Show>
              </Show></Show>
            </Show>
            <PipelinePagination position="bottom" />
            <Show when={view() === 'all' && currentCatalogue()}><nav class="anbud-pagination" aria-label={tr('Kunngjøringssider', 'Notice pages')}><span>{tr(`Side ${previousCursors().length + 1}`, `Page ${previousCursors().length + 1}`)}</span><button type="button" class="anbud-button" disabled={matchesBusy() || !previousCursors().length} onClick={() => { const history = previousCursors(); loadCatalogue(history[history.length - 1], history.slice(0, -1)) }}>{tr('Forrige', 'Previous')}</button><button type="button" class="anbud-button" disabled={matchesBusy() || !catalogue()?.nextCursor} onClick={() => loadCatalogue(catalogue()!.nextCursor!, [...previousCursors(), cursor()])}>{tr('Neste', 'Next')}</button></nav></Show>
          </main>
        </div></Show>
      </Show>
    </section>
  )
}
