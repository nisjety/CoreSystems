/**
 * The fictional home of the interactive demo (stage 04 of the product loop).
 *
 * Everything the real dashboard home, the navbar, Ctrl+K, the Nett history and
 * the Hent ut tab read is answered from here, in the exact shape the real client
 * unwraps. `requestJson` (src/shared/api/http.ts) unwraps a top-level `data` key,
 * so the traffic answer is double-wrapped on purpose. A missing field can crash
 * the whole SPA (there is no root ErrorBoundary), so every shape is typed against
 * the client types it feeds.
 *
 * The world is the morning of Friday 9 October 2026. Three e-mails have arrived
 * (the unread messages of the navbar), and the three cases start from the cards
 * under «Dette har skjedd». Nothing here tells how a case ends. The recent work of
 * Fjordform AS is older and says nothing about those cases.
 *
 * Times are fixed relative to `now` (the demo clock, local noon) and to the
 * visitor's own calendar day, so «i dag» and «08:30» read the same wherever the
 * visitor sits. For a given `now` the world is always the same. What a visitor
 * changes (read marks, added calendar entries, the theme) lives in the closures of
 * one world.
 *
 * The engine owns every `/api/v1/chat/threads*` and `/api/v1/chat/stream` route.
 * This module registers none of them.
 */
import type { CalendarEvent, CalendarNote, NavbarNotification, NavbarPayload, NavbarSearchResult, ThemePayload } from '@/shared/api/navbar-client'
import type { InformationDashboardPayload, InformationNewsArticle, InformationNewsPayload, InformationObservation, InformationTrafficPayload, InformationTrafficStation, InformationWeatherPayload } from '@/shared/api/information-client'
import type { RunItem, ScheduleItem } from '@/shared/api/ingestions-client'
import type { DemoRoute, DemoRouteContext, DemoRouteMethod, DemoRouteResult, HomeWorld, ThreadRow } from '../live/shared-types'
import { fjordformGdprStatus, fjordformOrg, fjordformSession, fjordformUser } from './fjordform'

const MINUTE_MS = 60_000
const DAY_MINUTES = 24 * 60
// Reserved .example host: the product's SharePoint check accepts it only in the demo build.
const intranetSite = 'https://intranett.fjordform.example/sites/intranett'

type WallClock = { year: number; month: number; day: number; hour: number; minute: number }

/** The wall-clock fields of an instant in the visitor's own time zone (the demo clock carries no offset). */
function localWallClock(epochMs: number): WallClock {
  const parts = new Intl.DateTimeFormat('en-GB', {
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    hourCycle: 'h23',
  }).formatToParts(new Date(epochMs))
  const field = (type: string) => Number(parts.find((part) => part.type === type)?.value)
  return { year: field('year'), month: field('month'), day: field('day'), hour: field('hour'), minute: field('minute') }
}

/** The instant of a wall-clock time in the visitor's zone: a first guess, corrected twice by the zone's own offset. */
function localInstant(wall: WallClock): number {
  const wanted = Date.UTC(wall.year, wall.month - 1, wall.day, wall.hour, wall.minute)
  let instant = wanted
  for (let pass = 0; pass < 2; pass += 1) {
    const seen = localWallClock(instant)
    instant += wanted - Date.UTC(seen.year, seen.month - 1, seen.day, seen.hour, seen.minute)
  }
  return instant
}

/** Every time of the world, relative to the demo clock and to the visitor's calendar day. */
function demoClock(now: number) {
  const today = localWallClock(now)
  const dayStart = (plusDays: number) => new Date(Date.UTC(today.year, today.month - 1, today.day + plusDays))
  return {
    nowIso: new Date(now).toISOString(),
    ago: (minutes: number) => new Date(now - minutes * MINUTE_MS).toISOString(),
    /** A wall-clock time of the demo calendar, `plusDays` after today. */
    at: (hour: number, minute = 0, plusDays = 0) => {
      const day = dayStart(plusDays)
      return new Date(localInstant({ year: day.getUTCFullYear(), month: day.getUTCMonth() + 1, day: day.getUTCDate(), hour, minute })).toISOString()
    },
    /** The calendar day, `plusDays` after today, as YYYY-MM-DD. */
    dayKey: (plusDays = 0) => dayStart(plusDays).toISOString().slice(0, 10),
  }
}

type DemoClock = ReturnType<typeof demoClock>

type Respond = (context: DemoRouteContext) => DemoRouteResult

const route = (method: DemoRouteMethod, match: RegExp, respond: Respond): DemoRoute => ({ method, match, respond })
const get = (match: RegExp, respond: Respond) => route('GET', match, respond)
const post = (match: RegExp, respond: Respond) => route('POST', match, respond)
const put = (match: RegExp, respond: Respond) => route('PUT', match, respond)

function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

/** Free text a visitor typed: one line, capped, never empty. */
function clipText(value: unknown): string {
  return typeof value === 'string' ? value.replace(/\s+/g, ' ').trim().slice(0, 200) : ''
}

function foldText(value: string): string {
  return value.normalize('NFKD').replace(/[̀-ͯ]/g, '').toLowerCase()
}

// ── Threads (seed only: the engine answers /api/v1/chat/threads*) ──────────

/** Five older conversations of the furniture maker. They carry no transcripts and say nothing about the three cases. */
function threadRows(clock: DemoClock): ThreadRow[] {
  return [
    {
      threadId: 'thr_lagerrapport_hall_b',
      title: 'Ukens lagerrapport · hall B',
      preview: 'Hall B: tre varer under bestillingspunkt. Ferdigvarer flyttes til nye reoler fra uke 42.',
      updatedAt: clock.ago(2 * DAY_MINUTES + 180),
      pinned: true,
      personaEnabled: true,
    },
    {
      threadId: 'thr_monteringsplan_uke43',
      title: 'Monteringsplan · uke 43',
      preview: 'Fire montørlag i uke 43, med prioritet til to prosjekter før helgen.',
      updatedAt: clock.ago(3 * DAY_MINUTES + 60),
      pinned: false,
      personaEnabled: true,
    },
    {
      threadId: 'thr_produksjon_uke40',
      title: 'Produksjon uke 40 · oppsummering',
      preview: 'Uke 40: 46 stoler og 12 bord ferdig. Oljing av ask gikk etter plan.',
      updatedAt: clock.ago(4 * DAY_MINUTES + 30),
      pinned: false,
      personaEnabled: true,
    },
    {
      threadId: 'thr_prisliste_2027',
      title: 'Prisliste 2027 · utkast',
      preview: 'Utkast med 3 % økning på eik og ask fra 1. januar. Ikke godkjent ennå.',
      updatedAt: clock.ago(5 * DAY_MINUTES),
      pinned: true,
      personaEnabled: true,
    },
    {
      threadId: 'thr_leveringsvilkar_2027',
      title: 'Leveringsvilkår 2027 · spørsmål',
      preview: 'Hva gjelder for fraktpriser og leveringstid i 2027 for kunder utenfor Østlandet?',
      updatedAt: clock.ago(6 * DAY_MINUTES + 90),
      pinned: false,
      personaEnabled: true,
    },
  ]
}

/**
 * The browser's copy of the thread list, in the stored shape of
 * `verevon.chat.threadHistory.v1` (flags only where they differ from the default).
 */
function threadHistoryStored(threads: ThreadRow[]) {
  return threads.map((thread) => ({
    threadId: thread.threadId,
    title: thread.title,
    preview: thread.preview,
    updatedAt: thread.updatedAt,
    ...(thread.pinned ? { pinned: true } : {}),
    ...(thread.personaEnabled ? {} : { personaEnabled: false }),
  }))
}

// ── Weather, news and traffic (the information client) ─────────────────────

function weatherPayload(clock: DemoClock): InformationWeatherPayload {
  return {
    current: {
      condition: 'Lettskyet',
      humidity: 78,
      icon: 'partly-cloudy',
      lastUpdated: clock.ago(18),
      location: 'Oslo',
      precipitation: 0,
      pressure: 1012,
      temperature: 9,
      windDirection: 220,
      windSpeed: 4,
    },
    forecast: [
      { condition: 'Lettskyet', date: clock.dayKey(1), icon: 'partly-cloudy', precipitation: 0, temperature: { max: 11, min: 5 } },
      { condition: 'Regn', date: clock.dayKey(2), icon: 'rain', precipitation: 6.4, temperature: { max: 9, min: 6 } },
      { condition: 'Skyet', date: clock.dayKey(3), icon: 'cloudy', precipitation: 0.6, temperature: { max: 10, min: 4 } },
    ],
  }
}

/** Four mild industry items from clearly fictional publishers (.example hosts). */
function newsPayload(clock: DemoClock): InformationNewsPayload {
  const article = (
    id: string,
    title: string,
    source: string,
    minutes: number,
    description: string,
    categories: string[],
    link: string,
  ): InformationNewsArticle => ({ id, title, source, publishDate: clock.ago(minutes), description, categories, link })
  return {
    articles: [
      article('n1', 'Flere møbelprodusenter flytter lager nærmere kundene', 'Næringsnytt', 55,
        'Nærhet til kundene veier tyngre enn lavere lagerkostnad, viser en rundspørring i bransjen.', ['Business'],
        'https://naeringsnytt.example/artikler/lager-naermere-kundene'),
      article('n2', 'Designmesse i Oslo: flere etterspør eik og oljet ask', 'Bransjebrevet', 170,
        'Utstillerne melder om økt interesse for lokalt produserte møbler i naturlige materialer.', ['Business'],
        'https://bransjebrevet.example/artikler/designmesse-eik-ask'),
      article('n3', 'Rådgivere: gå gjennom rammeavtalene før årsskiftet', 'Revisorbrevet', 290,
        'Mange avtaler fornyes automatisk uten gjennomgang. Revisorer anbefaler en sjekk av priser og vilkår i god tid.', ['General'],
        'https://revisorbrevet.example/artikler/rammeavtaler-for-arsskiftet'),
      article('n4', 'Små verksteder tar i bruk KI i kundeservice', 'Næringsnytt', 420,
        'Svar på e-post går raskere når et utkast er klart før saksbehandleren åpner saken.', ['Technology'],
        'https://naeringsnytt.example/artikler/ki-i-kundeservice'),
    ],
    hasMore: false,
    lastUpdated: clock.ago(12),
    totalCount: 4,
  }
}

/**
 * Two fictional measuring points. Every value is labelled synthetic and comes
 * from «Demodata», so the product's own formatter prints «Syntetisk · Demodata».
 */
function trafficPayload(clock: DemoClock): InformationTrafficPayload {
  const provenance = { observationType: 'estimated', quality: 'derived', source: 'demo' } as const
  const observation = (value: number, unit: string): InformationObservation => ({
    confidence: 0.8,
    fetchedAt: clock.ago(9),
    freshness: 'fresh',
    observationType: 'synthetic',
    observedAt: clock.ago(10),
    provider: 'Demodata',
    quality: 'good',
    source: 'demo',
    unit,
    value,
  })
  const station = (id: string, name: string, roadReference: string, county: string, speed: number, volume: number): InformationTrafficStation => ({
    id,
    name,
    locationName: name.replace('Målepunkt ', ''),
    roadReference,
    county,
    countyProvenance: provenance,
    roadReferenceProvenance: provenance,
    lastUpdated: clock.ago(10),
    status: 'operational',
    averageSpeed: observation(speed, 'km/h'),
    trafficVolume: observation(volume, 'vehicles_per_hour'),
  })
  return {
    data: [
      station('st_1', 'Målepunkt Fjordveien nord', 'Fv 160', 'Akershus', 54, 1240),
      station('st_2', 'Målepunkt Havnelageret', 'Kv 12', 'Oslo', 38, 610),
    ],
    success: true,
    timestamp: clock.ago(9),
  }
}

// ── Intranet pages (the third feed, «Fra <workspace>») and Ctrl+K knowledge ─

type IntranetPage = {
  document_id: string
  source: 'sharepoint'
  type: 'sharepoint_page'
  title: string
  document_date: string
  metadata: { web_url: string; site_web_url: string; page_layout: 'article'; publishing_level: 'published' }
}

/** The product's SharePoint check accepts a reserved .example host only in the demo build; the host is never requested. */
function intranetPayload(clock: DemoClock): { documents: IntranetPage[]; total: number } {
  const page = (id: string, title: string, minutes: number, slug: string): IntranetPage => ({
    document_id: id,
    source: 'sharepoint',
    type: 'sharepoint_page',
    title,
    document_date: clock.ago(minutes),
    metadata: {
      web_url: `${intranetSite}/SitePages/${slug}.aspx`,
      site_web_url: intranetSite,
      page_layout: 'article',
      publishing_level: 'published',
    },
  })
  return {
    documents: [
      page('sp_1', 'Nye rutiner for reklamasjoner', 300, 'rutiner-reklamasjoner'),
      page('sp_2', 'Høstens produktnyheter: eik og oljet ask', 1560, 'produktnyheter-host'),
      page('sp_3', 'Lagerflytting til hall B fullført', 5760, 'lagerflytting-hall-b'),
    ],
    total: 3,
  }
}

const knowledgeResults: NavbarSearchResult[] = [
  { id: 'kb_katalog_eik_ask', label: 'Katalog 2026 · eik og ask', excerpt: 'Fjord Stol Eik (FS-210), Fjord Bord Eik 180 (FB-180) og Fjord Stol Ask (FS-310). Oljert overflate som standard.', source: 'Dokumenter', href: '/knowledge' },
  { id: 'kb_ramme_beslag', label: 'Rammeavtale festebeslag 2026', excerpt: 'Avtalt enhetspris 1 240,00 kr for braketter, gyldig til 31.12.', source: 'Dokumenter', href: '/knowledge' },
  { id: 'kb_leveransevilkar', label: 'Leveransevilkår', excerpt: 'Leveringstid 5–7 virkedager fra ordrebekreftelse. Endringer avtales skriftlig.', source: 'Intranett', href: '/knowledge' },
  { id: 'kb_returpolicy', label: 'Returpolicy 2026', excerpt: 'Varer kan returneres innen 30 dager. Spesialbestillinger: 14 dager.', source: 'Intranett', href: '/knowledge' },
  { id: 'kb_hms_lager', label: 'HMS-håndbok for lager', excerpt: 'Truckførerbevis kreves før bruk av motorisert utstyr.', source: 'Intranett', href: '/knowledge' },
  { id: 'kb_prisliste', label: 'Prisliste 2026', excerpt: 'Gjelder fra 1. september. Nye varianter i eik og oljet ask.', source: 'Dokumenter', href: '/knowledge' },
]

function searchKnowledge(query: string): NavbarSearchResult[] {
  const terms = foldText(query).split(/\s+/).filter(Boolean)
  if (terms.length === 0) return []
  return knowledgeResults
    .filter((result) => {
      const text = foldText(`${result.label} ${result.excerpt}`)
      return terms.every((term) => text.includes(term))
    })
    .slice(0, 8)
}

// ── Hent ut (crawl runs and schedules: raw arrays on /api/ingestions) ─────

function runItems(clock: DemoClock): RunItem[] {
  return [
    {
      id: 'job_prisliste',
      runId: 'run_prisliste',
      kind: 'scrape',
      status: 'completed',
      createdAt: clock.ago(95),
      startedAt: clock.ago(95),
      completedAt: clock.ago(93),
      target: 'leverandor.fjordform.example/prisliste',
      progress: { completed: 1, total: 1, pages: 1 },
      stats: {},
    },
    {
      id: 'job_intranett',
      runId: 'run_intranett',
      kind: 'crawl',
      status: 'running',
      createdAt: clock.ago(12),
      startedAt: clock.ago(11),
      completedAt: null,
      target: 'intranett.fjordform.example',
      progress: { completed: 38, total: 120, pages: 38 },
      stats: {},
    },
    {
      id: 'job_vilkar',
      runId: 'run_vilkar',
      kind: 'extract',
      status: 'completed',
      createdAt: clock.ago(1560),
      startedAt: clock.ago(1560),
      completedAt: clock.ago(1557),
      target: 'Leveransevilkår.pdf',
      progress: { completed: 1, total: 1 },
      stats: {},
    },
  ]
}

/** Each schedule's last and next run fall at its cron's time: Monday 06:00, and 02:00 every night. */
function scheduleItems(clock: DemoClock): ScheduleItem[] {
  return [
    {
      id: 'sch_prisliste',
      name: 'Leverandørpriser hver mandag',
      kind: 'scrape',
      status: 'active',
      cron: '0 6 * * 1',
      createdAt: clock.ago(43200),
      lastRunAt: clock.at(6, 0, -4),
      nextRunAt: clock.at(6, 0, 3),
      target: 'leverandor.fjordform.example/prisliste',
      config: {},
    },
    {
      id: 'sch_intranett',
      name: 'Intranett hver natt',
      kind: 'crawl',
      status: 'active',
      cron: '0 2 * * *',
      createdAt: clock.ago(86400),
      lastRunAt: clock.at(2, 0),
      nextRunAt: clock.at(2, 0, 1),
      target: 'intranett.fjordform.example',
      config: {},
    },
  ]
}

// ── Navbar: notifications, messages, calendar, theme (kept in memory) ─────

type HomeState = {
  notifications: NavbarNotification[]
  messages: NavbarNotification[]
  events: CalendarEvent[]
  notes: CalendarNote[]
  theme: ThemePayload
  entries: number
}

const notice = (input: Omit<NavbarNotification, 'archived' | 'seen'>): NavbarNotification => ({
  archived: false,
  seen: input.read,
  ...input,
})

/**
 * The morning of 9 October. The e-mails are unread and say what the sender asked,
 * never how it turns out. The calendar holds facts that are already true: a
 * deadline and the all-day opening are dated notes, because the calendar shows an
 * event as a time range and a deadline has none.
 */
function initialHomeState(clock: DemoClock): HomeState {
  return {
    notifications: [
      notice({ id: 'ntf_intranett', source: 'notification', feed: 'system', title: 'Intranett · synkronisering pågår', body: 'Hent ut har lest 38 av 120 sider så langt.', createdAt: clock.ago(12), href: '/dashboard?tab=extract', read: false }),
      notice({ id: 'ntf_sikkerhetskopi', source: 'notification', feed: 'system', title: 'Sikkerhetskopi er fullført', body: 'Nattlig kopi av dokumenter og avtaler ble lagret uten avvik.', createdAt: clock.at(3, 10), href: '/dashboard', read: true }),
    ],
    messages: [
      notice({ id: 'msg_nordhavn', source: 'message', feed: 'mail', title: 'Nordhavn kommune', body: 'Anbud: FSC- og EPD-dokumentasjon til fredag 16. oktober kl. 12.', createdAt: clock.at(11, 5), href: '/chat', read: false }),
      notice({ id: 'msg_havnebrygga', source: 'message', feed: 'mail', title: 'Havnebrygga Kafé AS', body: 'Sprekk i bordplaten på bord 6. Kan dere hjelpe?', createdAt: clock.at(9, 35), href: '/chat', read: false }),
      notice({ id: 'msg_solhaug', source: 'message', feed: 'mail', title: 'Solhaug Hotell AS', body: 'Kan dere levere og montere 40 stoler og 10 bord før åpningen 30. november?', createdAt: clock.at(8, 47), href: '/chat', read: false }),
      notice({ id: 'msg_lager', source: 'message', feed: 'chat', title: 'Lager · Fjordform', body: 'Morgenmøtet starter 08:30 i hall B.', createdAt: clock.at(7, 40), href: '/chat', read: true }),
    ],
    events: [
      { id: 'evt_morgenmote', title: 'Morgenmøte lager', start: clock.at(8, 30), end: clock.at(8, 45), type: 'event', status: 'confirmed', createdAt: clock.ago(10080) },
      { id: 'evt_produksjon', title: 'Produksjonsmøte · uke 42', start: clock.at(13, 0, 3), end: clock.at(14, 0, 3), type: 'event', status: 'confirmed', createdAt: clock.ago(4320) },
    ],
    notes: [
      { id: 'note_hall_b', date: clock.dayKey(0), text: 'Hall B: ny plassering for ferdigvarer fra mandag.', createdAt: clock.ago(200) },
      { id: 'note_frist_anbud', date: clock.dayKey(7), text: 'Frist for anbud · Nordhavn kommune, kl. 12', createdAt: clock.ago(20160) },
      { id: 'note_apning', date: clock.dayKey(52), text: 'Åpning · Solhaug Hotell', createdAt: clock.ago(4320) },
    ],
    theme: { theme: 'light', configured: true, colorScheme: null },
    entries: 0,
  }
}

function navbarPayload(state: HomeState): NavbarPayload {
  return {
    calendar: { events: state.events, notes: state.notes },
    notifications: {
      configured: true,
      messages: state.messages,
      notifications: state.notifications,
      unreadCount: state.notifications.filter((item) => !item.read).length,
    },
    plan: 'pro',
    planTrial: false,
    trialEndsAt: null,
    profile: { id: fjordformUser.id, name: fjordformUser.name, email: fjordformUser.email, status: 'online' },
    theme: state.theme,
  }
}

function markNotificationRead(state: HomeState, body: unknown): DemoRouteResult {
  const id = isRecord(body) && typeof body.notificationId === 'string' ? body.notificationId : null
  if (!id) return undefined
  for (const item of [...state.notifications, ...state.messages]) {
    if (item.id === id) {
      item.read = true
      item.seen = true
    }
  }
  return {}
}

function nextEntryId(state: HomeState, prefix: string): string {
  state.entries += 1
  return `${prefix}_demo_${state.entries}`
}

/** A calendar note or event the visitor adds: echoed back and kept in memory only. */
function addCalendarEntry(state: HomeState, clock: DemoClock, body: unknown): DemoRouteResult {
  if (!isRecord(body)) return undefined
  if (body.kind === 'note') {
    const date = typeof body.date === 'string' ? body.date : ''
    const text = clipText(body.text)
    if (!/^\d{4}-\d{2}-\d{2}$/.test(date) || !text) return undefined
    const note: CalendarNote = { id: nextEntryId(state, 'note'), date, text, createdAt: clock.nowIso }
    state.notes.unshift(note)
    return { note }
  }
  const title = clipText(body.title)
  const start = typeof body.start === 'string' ? body.start : ''
  const end = typeof body.end === 'string' ? body.end : ''
  if (!title || Number.isNaN(Date.parse(start)) || Number.isNaN(Date.parse(end))) return undefined
  const event: CalendarEvent = {
    id: nextEntryId(state, 'evt'),
    title,
    start: new Date(start).toISOString(),
    end: new Date(end).toISOString(),
    type: typeof body.type === 'string' ? body.type : 'event',
    status: 'confirmed',
    createdAt: clock.nowIso,
  }
  state.events.unshift(event)
  return { event }
}

function themeChoice(value: unknown): ThemePayload['theme'] | null {
  return value === 'light' || value === 'dark' || value === 'system' ? value : null
}

function setTheme(state: HomeState, body: unknown): DemoRouteResult {
  const theme = isRecord(body) ? themeChoice(body.theme) : null
  if (!theme) return undefined
  state.theme = {
    theme,
    configured: true,
    colorScheme: isRecord(body) && typeof body.colorScheme === 'string' ? body.colorScheme : null,
  }
  return {}
}

// ── Organisation, session and the browser's local copies ───────────────────

function homeRoutes(state: HomeState, clock: DemoClock): DemoRoute[] {
  const weather = weatherPayload(clock)
  const news = newsPayload(clock)
  const traffic = trafficPayload(clock)
  const intranet = intranetPayload(clock)
  const runs = runItems(clock)
  const schedules = scheduleItems(clock)
  return [
    get(/^\/api\/v1\/auth\/session$/, () => ({ user: fjordformUser })),
    get(/^\/api\/v1\/session\/current$/, () => fjordformSession),
    get(/^\/api\/v1\/orgs$/, () => ({ organizations: [{ id: fjordformOrg.id, name: fjordformOrg.name, slug: 'fjordform' }] })),
    get(/^\/api\/v1\/orgs\/[^/]+\/gdpr\/deletion\/status$/, () => fjordformGdprStatus),
    get(/^\/api\/v1\/navbar$/, () => navbarPayload(state)),
    get(/^\/api\/v1\/navbar\/calendar$/, () => ({ events: state.events, notes: state.notes })),
    get(/^\/api\/v1\/navbar\/search$/, ({ url }) => ({ results: searchKnowledge(url.searchParams.get('q') ?? '') })),
    post(/^\/api\/v1\/navbar\/notifications$/, ({ body }) => markNotificationRead(state, body)),
    post(/^\/api\/v1\/navbar\/calendar$/, ({ body }) => addCalendarEntry(state, clock, body)),
    put(/^\/api\/v1\/navbar\/theme$/, ({ body }) => setTheme(state, body)),
    get(/^\/api\/v1\/information\/dashboard$/, () => dashboardPayload(weather, news, traffic)),
    get(/^\/api\/v1\/information\/weather$/, () => weather),
    get(/^\/api\/v1\/information\/news$/, ({ url }) => newsFor(news, url.searchParams.get('category'))),
    // requestJson unwraps the top-level `data`, so the traffic payload travels one level deeper.
    get(/^\/api\/v1\/information\/traffic$/, () => ({ data: traffic })),
    get(/^\/api\/v1\/knowledge\/documents$/, ({ url }) => (url.searchParams.get('type') === 'sharepoint_page' ? intranet : undefined)),
    get(/^\/api\/ingestions\/runs$/, () => runs),
    get(/^\/api\/ingestions\/schedules$/, () => schedules),
    get(/^\/api\/v1\/browser\/profiles$/, () => ({ profiles: [] })),
  ]
}

function dashboardPayload(
  weather: InformationWeatherPayload,
  news: InformationNewsPayload,
  traffic: InformationTrafficPayload,
): InformationDashboardPayload {
  return { weather, weatherError: null, news, newsError: null, traffic, trafficError: null }
}

function newsFor(news: InformationNewsPayload, category: string | null): InformationNewsPayload {
  const articles = category ? news.articles.filter((article) => article.categories.includes(category)) : news.articles
  return { ...news, articles, totalCount: articles.length }
}

type WebHistorySeedEntry =
  | { kind: 'query'; text: string; at: string }
  | { kind: 'page'; text: string; url: string; at: string }

/** The Nett history of the browser, as `verevon.home-web-history.v1` stores it. */
function webHistory(clock: DemoClock): WebHistorySeedEntry[] {
  return [
    { kind: 'query', text: 'stoler eik pris', at: clock.ago(35) },
    { kind: 'page', text: 'Prisliste 2026', url: 'https://dokumenter.fjordform.example/priser/prisliste-2026.pdf', at: clock.ago(100) },
    { kind: 'query', text: 'rammeavtale festebeslag prisregulering', at: clock.ago(150) },
    { kind: 'page', text: 'Leveransevilkår', url: 'https://intranett.fjordform.example/leveransevilkar', at: clock.ago(1380) },
  ]
}

function storageSeedFor(clock: DemoClock, threads: ThreadRow[]): Record<string, string> {
  // Keys as the product builds them: the Nett history is scoped by workspace, then user.
  const webHistoryKey = `verevon.home-web-history.v1:${encodeURIComponent(fjordformOrg.id)}:${encodeURIComponent(fjordformUser.id)}`
  return {
    'verevon.locale': 'no',
    'verevon.chat.browseWeb.v1': '1',
    'verevon.chat.threadHistory.v1': JSON.stringify(threadHistoryStored(threads)),
    [webHistoryKey]: JSON.stringify(webHistory(clock)),
  }
}

/**
 * The home world for one fictional «now» (epoch ms). Pure for a given `now`:
 * routes, thread seed and storage seed are the same on every call with it.
 */
export function buildHomeWorld(now: number): HomeWorld {
  if (!Number.isFinite(now)) throw new RangeError('buildHomeWorld: now must be a finite epoch time')
  const clock = demoClock(now)
  const threadSeed = threadRows(clock)
  return {
    routes: homeRoutes(initialHomeState(clock), clock),
    threadSeed,
    storageSeed: storageSeedFor(clock, threadSeed),
  }
}
