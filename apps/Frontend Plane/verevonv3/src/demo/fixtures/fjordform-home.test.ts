/**
 * The home world must answer every endpoint the home reads in the shape the
 * real client unwraps: each route is driven through the real clients (and
 * requestJson's `data` unwrap), so a shape drift fails here instead of crashing
 * the SPA in the visitor's browser. The world is also checked for fiction only,
 * for Norwegian text, and for saying nothing about the three cases beyond the
 * morning's e-mails and the calendar's true facts.
 *
 * The forbidden terms are written in pieces (FORBIDDEN_PATTERNS), so that this
 * file never carries one whole and a search of the repository stays clean.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { DEMO_NOW_ISO, type DemoRouteMethod, type HomeWorld } from '../live/shared-types'
import { buildHomeWorld } from './fjordform-home'
import { anbudSuggestion, anbudTitle } from './fjordform-anbud'
import { reklamasjonSuggestion, reklamasjonTitle } from './fjordform-reklamasjon'
import { tilbudSuggestion, tilbudTitle } from './fjordform-tilbud'
import {
  createNavbarCalendarEvent,
  createNavbarCalendarNote,
  getNavbarCalendarState,
  getNavbarData,
  markNavbarNotificationRead,
  saveNavbarTheme,
  searchNavbar,
} from '@/shared/api/navbar-client'
import {
  formatTrafficObservation,
  loadDashboardInformationSnapshot,
  loadNews,
  loadTraffic,
  loadWeather,
  newsCategoryOptions,
} from '@/shared/api/information-client'
import { listIngestionRuns, listIngestionSchedules } from '@/shared/api/ingestions-client'
import { listBrowserProfiles } from '@/shared/api/browser-client'
import { listOrganizations } from '@/shared/api/organization-client'
import { getDeletionStatus } from '@/shared/api/org-deletion-client'
import { loadWorkspacePages } from '@/shared/api/home-updates-client'
import { listChatThreads } from '@/shared/api/chat-client'
import { CHAT_THREAD_HISTORY_KEY, readChatThreadHistory } from '@/features/chat/lib/chat-thread-history'
import { webHistoryEntrySchema } from '@/features/dashboard/home/web-history'
import { webHistoryStorageKey } from '@/shared/session/home-local-content'

const NOW = Date.parse(DEMO_NOW_ISO)

const PROBE_PATHS = [
  '/api/v1/auth/session',
  '/api/v1/session/current',
  '/api/v1/orgs',
  '/api/v1/orgs/org_fjordform/gdpr/deletion/status',
  '/api/v1/navbar',
  '/api/v1/navbar/calendar',
  '/api/v1/navbar/search?q=eik&scope=knowledge',
  // A one-letter query matches all six Ctrl+K results, so the scans read every one of them.
  '/api/v1/navbar/search?q=e&scope=knowledge',
  '/api/v1/information/dashboard?newsLimit=8&newsMaxAge=24&trafficRadius=40',
  '/api/v1/information/weather',
  '/api/v1/information/news?limit=8&maxAge=24',
  '/api/v1/information/news?limit=8&maxAge=24&category=General',
  '/api/v1/information/news?limit=8&maxAge=24&category=Business',
  '/api/v1/information/news?limit=8&maxAge=24&category=Technology',
  '/api/v1/information/traffic?radius=40',
  '/api/v1/knowledge/documents?type=sharepoint_page&limit=100&offset=0',
  '/api/ingestions/runs',
  '/api/ingestions/schedules',
  '/api/v1/browser/profiles',
]

/** The navbar payload and the calendar hold the morning's e-mails and facts, so they are read apart from the recent work. */
const NAVBAR_PATHS = new Set(['/api/v1/navbar', '/api/v1/navbar/calendar'])

/**
 * The terms no visitor may meet in the home. Each pattern is joined from pieces,
 * and the scan runs case-insensitively over every body, the thread seed and the
 * storage seed.
 */
const FORBIDDEN_PATTERNS: readonly RegExp[] = [
  ['Nord', 'lys'],
  ['Bri', 'ng'],
  ['48', '21'],
  ['204', '71'],
  ['forsin', 'k'],
  ['fak', 'tura'],
  ['Aqua', 'tiq'],
  ['Vel', 'ion'],
  ['core', 'system'],
  ['Dac', 'osta'],
  ['elk', 'jop'],
  ['\\bIm', 'a\\b'],
  ['vg', '\\.no\\b'],
].map((parts) => new RegExp(parts.join(''), 'i'))

/** The three cases: their customers' names and the figures of their outcomes. None may appear in the recent work. */
const CASE_TERMS = /Solhaug|Havnebrygga|Nordhavn|sprekk|bordplate|serie 2609|ordre 5120|TB-7312|125 840|100 672|\bFSC\b|\bEPD\b|\btilbud\b|\banbud\b/i

/**
 * English words a visitor must not read in the home. Each is plain English that
 * Norwegian does not use, so no word that Norwegian shares (such as for, at, is
 * or ask) is in the list.
 */
const ENGLISH_WORDS: ReadonlySet<string> = new Set([
  'the', 'and', 'with', 'from', 'your', 'you', 'this', 'that', 'have', 'has', 'will', 'would', 'should', 'could', 'please',
  'update', 'updated', 'error', 'loading', 'open', 'close', 'view', 'save', 'cancel', 'submit', 'search', 'delivery',
  'invoice', 'order', 'customer', 'customers', 'product', 'products', 'report', 'document', 'documents', 'message',
  'messages', 'notification', 'notifications', 'calendar', 'meeting', 'morning', 'today', 'tomorrow', 'week', 'month',
  'price', 'prices', 'warehouse', 'supplier', 'sales', 'stock', 'business', 'general', 'technology', 'weather', 'traffic',
  'dashboard', 'settings', 'details', 'back', 'next', 'continue', 'edit', 'delete', 'name', 'title', 'description',
  'upload', 'download', 'workspace', 'page', 'pages', 'files', 'new', 'read', 'unread', 'nothing', 'something', 'welcome',
  'hello', 'thanks', 'regards', 'light', 'dark', 'all', 'more', 'show', 'hide', 'select', 'empty', 'finish', 'ready', 'here',
  'click', 'sorry',
])

function json(body: unknown, status: number): Response {
  return new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } })
}

function parseBody(text: string): unknown {
  try {
    return JSON.parse(text)
  } catch {
    return text
  }
}

/**
 * Answers `fetch` from the world's routes the way the demo interceptor does: a
 * route's result is sent as 200 JSON, and a path no route claims is a 404.
 */
function serve(world: HomeWorld): typeof fetch {
  return (async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = new URL(
      typeof input === 'string' ? input : input instanceof URL ? input.href : input.url,
      'https://demo.invalid',
    )
    const method = (init?.method ?? 'GET').toUpperCase() as DemoRouteMethod
    const body = typeof init?.body === 'string' ? parseBody(init.body) : undefined
    const found = world.routes.find((candidate) => candidate.method === method && candidate.match.test(url.pathname))
    const result = found
      ? await found.respond({ method, path: url.pathname, search: url.search, url, body, headers: new Headers(init?.headers) })
      : undefined
    if (result instanceof Response) return result
    if (result === undefined) return json({ error: { code: 'demo_unavailable', message: 'Dette er ikke med i demoen.' } }, 404)
    return json(result, 200)
  }) as typeof fetch
}

type Probe = { path: string; body: unknown }

/** The body of every probe path, answered by the world's own routes. */
async function probes(world: HomeWorld): Promise<Probe[]> {
  const served = serve(world)
  return Promise.all(
    PROBE_PATHS.map(async (path) => ({ path, body: (await (await served(path)).json()) as unknown })),
  )
}

/** The stored values of the storage seed, parsed where they are JSON. */
function storedValues(storage: Record<string, string>): unknown[] {
  return Object.values(storage).map((raw) => {
    try {
      return JSON.parse(raw) as unknown
    } catch {
      return raw
    }
  })
}

function forbiddenHits(text: string): string[] {
  return FORBIDDEN_PATTERNS.flatMap((pattern) => {
    const hit = text.match(pattern)
    return hit ? [hit[0]] : []
  })
}

/** A sentence or a label a visitor reads: it has a space or starts with a capital, and it is not a link, an address or a date. */
function isVisitorText(text: string): boolean {
  if (/:\/\/|^\/|@|^\d{4}-\d{2}-\d{2}/.test(text)) return false
  if (!/\p{L}/u.test(text)) return false
  return /\s/.test(text) || /^\p{Lu}/u.test(text)
}

/**
 * The strings a visitor could read from a value. A news article's `categories`
 * are topic values (General, Business, Technology) that the product translates
 * into Norwegian tab labels (newsCategoryLabel and the updates card), so the
 * values are never shown and are skipped here. The news test checks that every
 * value is one of the feed's own topics.
 */
function visitorStrings(value: unknown, key = ''): string[] {
  if (typeof value === 'string') return key !== 'categories' && isVisitorText(value) ? [value] : []
  if (Array.isArray(value)) return value.flatMap((item) => visitorStrings(item, key))
  if (value !== null && typeof value === 'object') {
    return Object.entries(value).flatMap(([childKey, child]) => visitorStrings(child, childKey))
  }
  return []
}

function englishWordsIn(text: string): string[] {
  return text.split(/[^\p{L}]+/u).map((word) => word.toLowerCase()).filter((word) => ENGLISH_WORDS.has(word))
}

/** The local clock time of an ISO instant, as HH:MM. */
function clockTime(iso: string): string {
  const date = new Date(iso)
  return `${String(date.getHours()).padStart(2, '0')}:${String(date.getMinutes()).padStart(2, '0')}`
}

describe('home world: each answer has the shape its real client reads', () => {
  let world: HomeWorld

  beforeEach(() => {
    // The demo build serves the reserved .example intranet host that the SharePoint check accepts only there.
    vi.stubEnv('VITE_DEMO', '1')
    world = buildHomeWorld(NOW)
    vi.stubGlobal('fetch', serve(world))
  })

  afterEach(() => {
    vi.unstubAllGlobals()
    vi.unstubAllEnvs()
  })

  it('answers the greeting context, the navbar and the calendar', async () => {
    const navbar = await getNavbarData()
    expect(navbar.profile?.name).toBe('Ida Berg')
    expect(navbar.plan).toBe('pro')
    expect(navbar.notifications.configured).toBe(true)
    expect(navbar.notifications.unreadCount).toBe(1)
    expect(navbar.notifications.notifications.map((item) => item.id)).toEqual(['ntf_intranett', 'ntf_sikkerhetskopi'])
    expect(navbar.notifications.messages.map((item) => item.id)).toEqual(['msg_nordhavn', 'msg_havnebrygga', 'msg_solhaug', 'msg_lager'])
    expect(navbar.notifications.messages.filter((item) => !item.read).map((item) => item.title)).toEqual([
      'Nordhavn kommune',
      'Havnebrygga Kafé AS',
      'Solhaug Hotell AS',
    ])
    const calendar = await getNavbarCalendarState()
    expect(calendar.events.map((event) => event.title)).toEqual(['Morgenmøte lager', 'Produksjonsmøte · uke 42'])
    expect(calendar.notes.map((note) => note.text)).toEqual([
      'Hall B: ny plassering for ferdigvarer fra mandag.',
      'Frist for anbud · Nordhavn kommune, kl. 12',
      'Åpning · Solhaug Hotell',
    ])
  })

  it('receives the three unread e-mails at the times the case cards name, saying what was asked and not how it ends', async () => {
    const mails = (await getNavbarData()).notifications.messages.filter((item) => item.feed === 'mail')
    const sentAt = new Map(mails.map((mail) => [mail.title, clockTime(mail.createdAt ?? '')]))
    expect(sentAt.get('Solhaug Hotell AS')).toBe(tilbudSuggestion.time.replace('i dag ', ''))
    expect(sentAt.get('Havnebrygga Kafé AS')).toBe(reklamasjonSuggestion.time.replace('i dag ', ''))
    expect(sentAt.get('Nordhavn kommune')).toBe(anbudSuggestion.time.replace('i dag ', ''))
    expect(mails.every((mail) => !mail.read)).toBe(true)
    expect(tilbudSuggestion.source).toContain('Solhaug Hotell AS')
    expect(reklamasjonSuggestion.source).toContain('Havnebrygga Kafé AS')
    expect(anbudSuggestion.source).toContain('Nordhavn kommune')
    expect(mails.map((mail) => mail.body).join('\n')).not.toMatch(/garanti|dekkes|godkjent|reservert|bekreftet|utløpt|gyldig|kreditnota|125 840|100 672|TB-7312/i)
  })

  it('loads weather, news and traffic through the information client', async () => {
    const snapshot = await loadDashboardInformationSnapshot()
    expect(snapshot.weather?.current.location).toBe('Oslo')
    expect(snapshot.weather?.current.condition).toBe('Lettskyet')
    expect(snapshot.weatherError).toBeNull()
    expect(snapshot.news?.articles).toHaveLength(4)
    expect(snapshot.newsError).toBeNull()
    expect(snapshot.traffic?.data).toHaveLength(2)
    expect(snapshot.trafficError).toBeNull()

    const weather = await loadWeather()
    expect(weather.data.forecast.map((day) => day.date)).toEqual(['2026-10-10', '2026-10-11', '2026-10-12'])

    const traffic = await loadTraffic()
    expect(traffic.data.success).toBe(true)
    expect(traffic.data.data).toHaveLength(2)
    expect(formatTrafficObservation(traffic.data.data[0]!.averageSpeed, 'nb-NO')).toBe('54 km/h · Syntetisk · Demodata')
    expect(formatTrafficObservation(traffic.data.data[0]!.trafficVolume, 'nb-NO')).toContain('kjøretøy/time · Syntetisk · Demodata')
  })

  it('serves each news topic the feed offers, and only that topic', async () => {
    expect((await loadNews('all')).articles).toHaveLength(4)
    expect((await loadNews('Business')).articles.map((article) => article.id)).toEqual(['n1', 'n2'])
    expect((await loadNews('General')).articles.map((article) => article.id)).toEqual(['n3'])
    expect((await loadNews('Technology')).articles.map((article) => article.id)).toEqual(['n4'])
    expect((await loadNews('Sport')).articles).toEqual([])
    // Every article sits under a topic the category tabs offer, so each one is reachable from its tab.
    const topics: string[] = newsCategoryOptions.map((option) => option.value)
    for (const article of (await loadNews('all')).articles) {
      for (const category of article.categories) expect(topics).toContain(category)
    }
  })

  it('answers Ctrl+K from the knowledge documents', async () => {
    expect((await searchNavbar('eik')).results.map((result) => result.label)).toEqual(['Katalog 2026 · eik og ask', 'Prisliste 2026'])
    expect((await searchNavbar('e')).results).toHaveLength(6)
    expect((await searchNavbar('festebeslag')).results.map((result) => result.label)).toEqual(['Rammeavtale festebeslag 2026'])
    expect((await searchNavbar('zzz-ingen-treff')).results).toEqual([])
  })

  it('lists the workspace intranet pages, newest first, all with SharePoint links', async () => {
    const pages = await loadWorkspacePages('org_fjordform')
    expect(pages.pages.map((page) => page.title)).toEqual([
      'Nye rutiner for reklamasjoner',
      'Høstens produktnyheter: eik og oljet ask',
      'Lagerflytting til hall B fullført',
    ])
    expect(pages.pages.every((page) => page.url.startsWith('https://intranett.fjordform.example/'))).toBe(true)
    expect(pages.hasMore).toBe(false)
  })

  it('answers the Hent ut lists, the workspace list, the browser profiles and the GDPR status', async () => {
    expect((await listIngestionRuns()).map((run) => run.status)).toEqual(['completed', 'running', 'completed'])
    expect((await listIngestionSchedules()).map((schedule) => schedule.name)).toEqual([
      'Leverandørpriser hver mandag',
      'Intranett hver natt',
    ])
    // Each run falls at its cron's time: Monday 06:00 for the price list, 02:00 every night for the intranet.
    const [prisliste, intranett] = await listIngestionSchedules()
    expect([prisliste?.cron, intranett?.cron]).toEqual(['0 6 * * 1', '0 2 * * *'])
    expect([clockTime(prisliste?.lastRunAt ?? ''), clockTime(prisliste?.nextRunAt ?? '')]).toEqual(['06:00', '06:00'])
    expect([clockTime(intranett?.lastRunAt ?? ''), clockTime(intranett?.nextRunAt ?? '')]).toEqual(['02:00', '02:00'])
    expect(new Date(prisliste?.nextRunAt ?? '').getDay()).toBe(1)
    expect(Date.parse(prisliste?.lastRunAt ?? '')).toBeLessThan(NOW)
    expect(Date.parse(prisliste?.nextRunAt ?? '')).toBeGreaterThan(NOW)
    expect(Date.parse(intranett?.nextRunAt ?? '')).toBeGreaterThan(NOW)
    expect(await listBrowserProfiles('org_fjordform')).toEqual({ profiles: [] })
    expect(await listOrganizations()).toEqual([{ id: 'org_fjordform', name: 'Fjordform AS', slug: 'fjordform' }])
    expect(await getDeletionStatus('org_fjordform')).toMatchObject({ pending: false, org_name: 'Fjordform AS' })
  })

  it('takes the visitor’s own changes in memory: read marks, calendar entries and theme', async () => {
    await markNavbarNotificationRead('msg_solhaug')
    expect((await getNavbarData()).notifications.messages.filter((item) => !item.read)).toHaveLength(2)
    await markNavbarNotificationRead('ntf_intranett')
    expect((await getNavbarData()).notifications.unreadCount).toBe(0)

    const event = await createNavbarCalendarEvent({
      title: 'Lagergjennomgang',
      start: '2026-10-09T14:00:00.000Z',
      end: '2026-10-09T14:30:00.000Z',
      type: 'event',
    })
    expect(event.event.title).toBe('Lagergjennomgang')
    const note = await createNavbarCalendarNote({ date: '2026-10-09', kind: 'note', text: 'Sjekk lagerstatus' })
    expect(note.note.text).toBe('Sjekk lagerstatus')
    const calendar = await getNavbarCalendarState()
    expect(calendar.events[0]?.title).toBe('Lagergjennomgang')
    expect(calendar.notes[0]?.text).toBe('Sjekk lagerstatus')

    await saveNavbarTheme('dark')
    expect((await getNavbarData()).theme?.theme).toBe('dark')
  })
})

describe('home world: the seeds, the routes and the calendar', () => {
  it('seeds the chat list the real client reads: five older threads, none of the three cases', async () => {
    const world = buildHomeWorld(NOW)
    vi.stubGlobal('fetch', async () => json({ sessions: world.threadSeed, retention: { zdr: false } }, 200))
    try {
      const sessions = await listChatThreads()
      expect(sessions.map((session) => session.threadId)).toEqual([
        'thr_lagerrapport_hall_b',
        'thr_monteringsplan_uke43',
        'thr_produksjon_uke40',
        'thr_prisliste_2027',
        'thr_leveringsvilkar_2027',
      ])
      expect(sessions.every((session) => session.title.length > 0)).toBe(true)
    } finally {
      vi.unstubAllGlobals()
    }

    for (const thread of world.threadSeed) {
      expect(Object.keys(thread).sort()).toEqual(['personaEnabled', 'pinned', 'preview', 'threadId', 'title', 'updatedAt'])
      expect(Date.parse(thread.updatedAt)).toBeLessThan(NOW)
    }
    expect(world.threadSeed.filter((thread) => thread.pinned).map((thread) => thread.threadId)).toEqual([
      'thr_lagerrapport_hall_b',
      'thr_prisliste_2027',
    ])
    const titles = world.threadSeed.map((thread) => thread.title)
    for (const caseTitle of [tilbudTitle, reklamasjonTitle, anbudTitle]) expect(titles).not.toContain(caseTitle)
  })

  it('stores the browser copies in the shape the product reads on boot', () => {
    const world = buildHomeWorld(NOW)
    localStorage.clear()
    for (const [key, value] of Object.entries(world.storageSeed)) localStorage.setItem(key, value)

    expect(readChatThreadHistory().map((item) => item.threadId)).toEqual(world.threadSeed.map((thread) => thread.threadId))
    expect(world.storageSeed[CHAT_THREAD_HISTORY_KEY]).toBeDefined()

    const webKey = webHistoryStorageKey('usr_fjordform_ida', 'org_fjordform')
    expect(webKey).toBe('verevon.home-web-history.v1:org_fjordform:usr_fjordform_ida')
    const entries = JSON.parse(world.storageSeed[webKey!]!) as unknown[]
    expect(entries).toHaveLength(4)
    expect(entries.every((entry) => {
      const parsed = webHistoryEntrySchema.safeParse(entry)
      return parsed.success && parsed.data !== null
    })).toBe(true)
    expect(entries[0]).toMatchObject({ kind: 'query', text: 'stoler eik pris' })
  })

  it('registers no chat thread or chat stream route', () => {
    const world = buildHomeWorld(NOW)
    for (const path of [
      '/api/v1/chat/threads',
      '/api/v1/chat/threads/thr_probe',
      '/api/v1/chat/threads/thr_probe/messages',
      '/api/v1/chat/stream',
    ]) {
      expect(world.routes.some((candidate) => candidate.match.test(path))).toBe(false)
    }
  })

  it('answers a path it does not own with 404 and leaves other knowledge types to the next responder', async () => {
    const served = serve(buildHomeWorld(NOW))
    expect((await served('/api/v1/chat/threads')).status).toBe(404)
    expect((await served('/api/v1/knowledge/documents?type=web_page')).status).toBe(404)
    expect((await served('/api/v1/knowledge/documents?type=sharepoint_page')).status).toBe(200)
  })

  it('wraps only the traffic answer one level deeper, as requestJson expects', async () => {
    const served = serve(buildHomeWorld(NOW))
    const body = (await (await served('/api/v1/information/traffic?radius=40')).json()) as { data: { data: unknown[]; success: boolean } }
    expect(Array.isArray(body.data.data)).toBe(true)
    expect(body.data.success).toBe(true)
  })

  it('places every time on the demo day in the visitor’s own zone, so 08:30 reads 08:30 wherever they sit', async () => {
    const world = buildHomeWorld(NOW)
    vi.stubGlobal('fetch', serve(world))
    try {
      const calendar = await getNavbarCalendarState()
      expect(calendar.events[0]?.start).toBe(new Date(2026, 9, 9, 8, 30).toISOString())
      expect(calendar.events[0]?.end).toBe(new Date(2026, 9, 9, 8, 45).toISOString())
      expect(calendar.events[1]?.start).toBe(new Date(2026, 9, 12, 13, 0).toISOString())
      expect(calendar.notes.map((note) => note.date)).toEqual(['2026-10-09', '2026-10-16', '2026-11-30'])
      // The weekdays the texts name: the production meeting on Monday 12 October, the deadline on Friday 16 October, the opening on Monday 30 November.
      expect([new Date(2026, 9, 12).getDay(), new Date(2026, 9, 16).getDay(), new Date(2026, 10, 30).getDay()]).toEqual([1, 5, 1])
      const mails = (await getNavbarData()).notifications.messages.filter((item) => item.feed === 'mail')
      expect(mails.map((mail) => mail.createdAt)).toEqual([
        new Date(2026, 9, 9, 11, 5).toISOString(),
        new Date(2026, 9, 9, 9, 35).toISOString(),
        new Date(2026, 9, 9, 8, 47).toISOString(),
      ])
    } finally {
      vi.unstubAllGlobals()
    }
  })

  it('puts into the calendar only facts that are true: no assembly slot and no replacement visit', async () => {
    const served = serve(buildHomeWorld(NOW))
    const calendar = (await (await served('/api/v1/navbar/calendar')).json()) as {
      events: Array<{ title: string }>
      notes: Array<{ text: string }>
    }
    const entries = [...calendar.events.map((event) => event.title), ...calendar.notes.map((note) => note.text)]
    expect(entries.join('\n')).not.toMatch(/montør|monter|bytt|levere|tilbud|reklamasjon|sprekk/i)
  })

  it('is the same world for the same clock', () => {
    const first = buildHomeWorld(NOW)
    const second = buildHomeWorld(NOW)
    expect(second.threadSeed).toEqual(first.threadSeed)
    expect(second.storageSeed).toEqual(first.storageSeed)
  })

  it('refuses a clock that is not a time', () => {
    expect(() => buildHomeWorld(Number.NaN)).toThrow(RangeError)
  })

  it('keeps the forbidden terms out of every body, the thread seed and the storage seed', async () => {
    const world = buildHomeWorld(NOW)
    const text = [JSON.stringify(await probes(world)), JSON.stringify(world.threadSeed), JSON.stringify(world.storageSeed)].join('\n')
    expect(forbiddenHits(text)).toEqual([])
    // The scan itself finds a term when one is there.
    expect(forbiddenHits(['Nord', 'lys'].join(''))).toHaveLength(1)
    expect(forbiddenHits('Ida Berg, Fjordform AS')).toEqual([])
  })

  it('names only fictional addresses, and only .example hosts', async () => {
    const world = buildHomeWorld(NOW)
    const text = [JSON.stringify(await probes(world)), JSON.stringify(world.threadSeed), JSON.stringify(world.storageSeed)].join('\n')
    for (const address of text.match(/[\w.+-]+@[\w.-]+/g) ?? []) {
      expect(address.endsWith('@fjordform.example')).toBe(true)
    }
    for (const match of text.matchAll(/https?:\/\/([^/"\s\\]+)/g)) {
      expect((match[1] ?? '').endsWith('.example')).toBe(true)
    }
  })

  it('speaks Norwegian: no English word in any text a visitor reads', async () => {
    const world = buildHomeWorld(NOW)
    const texts = [...(await probes(world)).map((probe) => probe.body), world.threadSeed, storedValues(world.storageSeed)]
      .flatMap((root) => visitorStrings(root))
    expect(texts.filter((text) => englishWordsIn(text).length > 0)).toEqual([])
    // The check itself finds English where there is some, and not in the Norwegian the world uses.
    expect(englishWordsIn('The new delivery')).toEqual(['the', 'new', 'delivery'])
    expect(englishWordsIn('Lettskyet, Oslo · 9 grader')).toEqual([])
  })

  it('says nothing about the three cases in the recent work', async () => {
    const world = buildHomeWorld(NOW)
    const recent = (await probes(world))
      .filter((probe) => !NAVBAR_PATHS.has(probe.path.split('?')[0] ?? ''))
      .map((probe) => JSON.stringify(probe.body))
    const text = [...recent, JSON.stringify(world.threadSeed), JSON.stringify(world.storageSeed)].join('\n')
    expect(CASE_TERMS.test(text)).toBe(false)
  })
})
