/**
 * Scene-mode runtime installer.
 *
 * Installed from the guarded hook in `src/index.tsx` BEFORE the app renders.
 * It replaces `window.fetch` for same-origin `/api/*` requests with fixture
 * answers (see `fixtures/fjordform.ts`) and plays the active scene's SSE
 * script for `POST /api/v1/chat/stream` through a `ReadableStream` whose
 * frames are enqueued with `setTimeout`, relative to the instant the request
 * arrived — so the whole answer advances under paused/advancing virtual time.
 *
 * Non-API same-origin fetches pass through untouched. Unknown `/api/*` fetches
 * get an empty but shape-valid body and are recorded with kind `unmatched-api`
 * in the scene log so they can be reviewed. Image requests are blocked unless
 * they map to a fixture asset.
 *
 * Nothing in this module touches a chat component: fixtures enter only through
 * fetch and the stream, exactly as a real gateway's answers would.
 */
import {
  FJORDFORM_NOW_ISO,
  fjordformConnections,
  fjordformGdprStatus,
  fjordformMemories,
  fjordformModels,
  fjordformNavbar,
  fjordformProviders,
  fjordformSession,
  fjordformSkills,
  fjordformUser,
  nordlysApproval,
} from './fixtures/fjordform'
import type { SceneExtras, SceneLogEntry, SceneRunStream, SceneStreamScript } from './scene-types'

type Route = {
  method?: 'GET' | 'POST' | 'PUT' | 'DELETE' | 'PATCH'
  match: RegExp
  respond: (match: RegExpMatchArray, request: { url: URL; init?: RequestInit }) => unknown | Response
}

const sceneLog: SceneLogEntry[] = []
let sceneClockStart = 0
let activeStream: SceneStreamScript | null = null
let originalFetch: typeof window.fetch | null = null
let previousSceneStorage: { locale: string | null; browseWeb: string | null } | null = null
const requestCounts = new Map<string, number>()

/** Scene time in ms, relative to `start()` (0 before the scene starts). */
export function sceneNow(): number {
  return sceneClockStart ? performance.now() - sceneClockStart : 0
}

export function markSceneStart(): void {
  sceneClockStart = performance.now()
}

export function appendSceneLog(kind: string, note: string): void {
  sceneLog.push({ atMs: Math.round(sceneNow()), kind, note })
}

export function getSceneLog(): ReadonlyArray<SceneLogEntry> {
  return sceneLog
}

/** How many times each `/api` path was requested (navbar polling check). */
export function getRequestCounts(): ReadonlyMap<string, number> {
  return requestCounts
}

/** The scene page registers the stream script the next chat POST should play. */
export function setActiveSceneStream(script: SceneStreamScript | null): void {
  activeStream = script
}

let activeExtras: SceneExtras | null = null
/** `performance.now()` when the chat POST arrived: the clock run streams share. */
let chatPostAt: number | null = null

/**
 * Scene-specific routes, run-event streams and fixture images (all optional;
 * a scene without them behaves exactly as before).
 */
export function setActiveSceneExtras(extras: SceneExtras | null): void {
  activeExtras = extras
  chatPostAt = null
  if (extras?.images && Object.keys(extras.images).length > 0) installImageShim()
}

/**
 * Browser step screenshots are loaded by a plain `<img src>` pointing at the
 * BFF artifact route, which `fetch` never sees. In scene mode the src is
 * mapped, at the moment it is set, to a fixture asset served by Vite, so no
 * request ever leaves for `/api` and the component renders its real frame.
 */
let imageShimInstalled = false
let originalImageSrcDescriptor: PropertyDescriptor | undefined
let originalSetAttribute: typeof Element.prototype.setAttribute | null = null
const BLOCKED_IMAGE_SRC = 'data:,'
function rewriteImageSource(value: string): string {
  if (!value) return value
  try {
    const url = new URL(value, window.location.origin)
    if (url.protocol === 'data:' || url.protocol === 'blob:') return value
    if (url.origin !== window.location.origin) {
      appendSceneLog('blocked-image', `${url.origin}${url.pathname}`)
      return BLOCKED_IMAGE_SRC
    }
    if (url.pathname.startsWith('/api/') || url.pathname === '/health') {
      const asset = activeExtras?.images?.[url.pathname]
      if (asset) {
        appendSceneLog('image', `${url.pathname} → fixture asset`)
        return asset
      }
      appendSceneLog('blocked-image', url.pathname)
      return BLOCKED_IMAGE_SRC
    }
    return value
  } catch {
    appendSceneLog('blocked-image', 'invalid URL')
    return BLOCKED_IMAGE_SRC
  }
}

function installImageShim(): void {
  if (imageShimInstalled) return
  imageShimInstalled = true
  const descriptor = Object.getOwnPropertyDescriptor(HTMLImageElement.prototype, 'src')
  if (descriptor?.set && descriptor.get) {
    originalImageSrcDescriptor = descriptor
    const setSrc = descriptor.set
    Object.defineProperty(HTMLImageElement.prototype, 'src', {
      ...descriptor,
      set(this: HTMLImageElement, value: string) {
        setSrc.call(this, rewriteImageSource(String(value)))
      },
    })
  }
  const setAttribute = Element.prototype.setAttribute
  originalSetAttribute = setAttribute
  Element.prototype.setAttribute = function patchedSetAttribute(this: Element, name: string, value: string) {
    if (this instanceof HTMLImageElement && name.toLowerCase() === 'src') {
      return setAttribute.call(this, name, rewriteImageSource(String(value)))
    }
    return setAttribute.call(this, name, value)
  }
}

/**
 * A run's live tail as an event stream. Frame times are relative to the chat
 * POST; frames whose time has already passed when the subscriber connects are
 * delivered at once, in order, so every panel watching the run sees the same
 * history regardless of when it mounted.
 */
function runStreamResponse(stream: SceneRunStream): Response {
  const encoder = new TextEncoder()
  const timers: number[] = []
  const elapsed = chatPostAt === null ? 0 : performance.now() - chatPostAt
  appendSceneLog('run-sse', `GET run events ${stream.runId} (+${Math.round(elapsed)} ms after the chat POST)`)
  const body = new ReadableStream<Uint8Array>({
    start(controller) {
      let lastAt = 0
      let order = 0
      for (const frame of stream.frames) {
        lastAt = Math.max(lastAt, frame.at)
        const delay = Math.max(0, frame.at - elapsed)
        const sequence = order
        order += 1
        timers.push(window.setTimeout(() => {
          appendSceneLog('run-sse', `${stream.runId} ${frame.event} (+${frame.at} ms)`)
          controller.enqueue(encoder.encode(sseFrame(frame.event, frame.data, frame.id ?? `${stream.runId}-${sequence}`)))
        }, delay))
      }
      if (stream.closeAfterMs != null) {
        timers.push(window.setTimeout(() => controller.close(), Math.max(0, lastAt + stream.closeAfterMs - elapsed)))
      }
    },
    cancel() {
      for (const timer of timers) window.clearTimeout(timer)
    },
  })
  return new Response(body, {
    status: 200,
    headers: { 'Content-Type': 'text/event-stream', 'Cache-Control': 'no-cache' },
  })
}

function sceneExtraRoute(method: string, url: URL, path: string): Response | null {
  const extras = activeExtras
  if (!extras) return null
  const events = url.pathname.match(/^\/api\/v1\/runs\/([^/]+)\/events$/)
  if (events && method === 'GET') {
    const runId = decodeURIComponent(events[1] ?? '')
    const stream = extras.runStreams?.find((candidate) => candidate.runId === runId)
    if (stream) return runStreamResponse(stream)
  }
  for (const route of extras.routes ?? []) {
    if (route.method && route.method !== method) continue
    if (!route.match.test(path) && !route.match.test(url.pathname)) continue
    return json(route.body, route.status ?? 200)
  }
  return null
}

function json(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'Content-Type': 'application/json' },
  })
}

function sseFrame(event: string, data: unknown, id?: string): string {
  const lines = [`event: ${event}`]
  if (id) lines.push(`id: ${id}`)
  lines.push(`data: ${JSON.stringify(data)}`)
  return `${lines.join('\n')}\n\n`
}

/**
 * The chat answer as a server-sent-event body. Every frame is scheduled with
 * `setTimeout` from the moment the POST arrives (never from `Date.now`), so the
 * capture harness's virtual clock drives it frame by frame.
 */
function streamResponse(script: SceneStreamScript): Response {
  const encoder = new TextEncoder()
  const timers: number[] = []
  const body = new ReadableStream<Uint8Array>({
    start(controller) {
      let lastAt = 0
      for (const frame of script.frames) {
        lastAt = Math.max(lastAt, frame.at)
        timers.push(window.setTimeout(() => {
          appendSceneLog('sse', `${frame.event} (+${frame.at} ms)`)
          controller.enqueue(encoder.encode(sseFrame(frame.event, frame.data, frame.id)))
        }, frame.at))
      }
      timers.push(window.setTimeout(() => {
        appendSceneLog('sse', 'stream closed')
        controller.close()
      }, lastAt + (script.closeAfterMs ?? 40)))
    },
    cancel() {
      for (const timer of timers) window.clearTimeout(timer)
    },
  })
  return new Response(body, {
    status: 200,
    headers: { 'Content-Type': 'text/event-stream', 'Cache-Control': 'no-cache' },
  })
}

const routes: Route[] = [
  // ── Session ──────────────────────────────────────────────────────────────
  { match: /^\/api\/v1\/auth\/session$/, respond: () => ({ user: fjordformUser }) },
  { match: /^\/api\/v1\/session\/current$/, respond: () => fjordformSession },
  { match: /^\/api\/v1\/me$/, respond: () => fjordformUser },
  {
    match: /^\/api\/v1\/me\/session-context$/,
    respond: () => ({
      userId: fjordformUser.id,
      email: fjordformUser.email,
      name: fjordformUser.name,
      orgId: fjordformSession.org.id,
      role: fjordformSession.org.role,
      permissions: fjordformSession.permissions,
      onboardingStatus: 'COMPLETED',
      orgs: [fjordformSession.org],
    }),
  },
  // ── Shell ────────────────────────────────────────────────────────────────
  { match: /^\/api\/v1\/navbar$/, respond: () => fjordformNavbar },
  { match: /^\/api\/v1\/navbar\/calendar$/, respond: () => fjordformNavbar.calendar },
  { match: /^\/api\/v1\/orgs\/[^/]+\/gdpr\/deletion\/status$/, respond: () => fjordformGdprStatus },
  { match: /^\/api\/v1\/ownership\/status$/, respond: () => ({ configured: true, ok: true }) },
  { match: /^\/api\/v1\/notifications/, respond: () => ({ notifications: [], unreadCount: 0 }) },
  // The start screen asks for open inbox conversations next to the alerts
  // (EmptyChatState's context-aware suggestions). The films carry their own
  // suggestion card (suggestions.ts), so the real feature gets none here.
  { match: /^\/api\/v1\/inbox\/conversations(\?.*)?$/, respond: () => ({ tickets: [], total: 0 }) },
  { match: /^\/api\/v1\/home-updates/, respond: () => ({ updates: [], items: [] }) },
  // ── Composer ─────────────────────────────────────────────────────────────
  { match: /^\/api\/v1\/models$/, respond: () => fjordformModels },
  { match: /^\/api\/v1\/skills$/, respond: () => fjordformSkills },
  { match: /^\/api\/v1\/integrations\/providers$/, respond: () => fjordformProviders },
  { match: /^\/api\/v1\/integrations\/connections$/, respond: () => fjordformConnections },
  { match: /^\/api\/v1\/integrations\/profile$/, respond: () => ({ connections: [], providers: [] }) },
  { match: /^\/api\/v1\/memory(\?.*)?$/, respond: () => fjordformMemories },
  { match: /^\/api\/v1\/chatgpt-subscription/, respond: () => ({ connections: [], connected: false }) },
  { match: /^\/api\/v1\/subscriptions/, respond: () => ({ connections: [], items: [] }) },
  { match: /^\/api\/v1\/actions/, respond: () => ({ actions: [], items: [] }) },
  { match: /^\/api\/v1\/persona/, respond: () => ({ persona: null, enabled: false }) },
  { match: /^\/api\/v1\/privacy/, respond: () => ({ tiers: [], items: [] }) },
  { match: /^\/api\/v1\/billing/, respond: () => ({ allowances: [], items: [] }) },
  { match: /^\/api\/v1\/cron/, respond: () => ({ schedules: [], items: [] }) },
  { match: /^\/api\/v1\/knowledge\/source-list/, respond: () => ({ sources: [], items: [] }) },
  { match: /^\/api\/v1\/knowledge\/documents/, respond: () => ({ documents: [] }) },
  { match: /^\/api\/v1\/knowledge/, respond: () => ({ items: [], sources: [], documents: [], pages: [] }) },
  // ── Threads ──────────────────────────────────────────────────────────────
  { method: 'GET', match: /^\/api\/v1\/chat\/threads$/, respond: () => ({ threads: [] }) },
  { method: 'GET', match: /^\/api\/v1\/chat\/threads\/[^/]+\/transcript$/, respond: () => json(null, 404) },
  { method: 'GET', match: /^\/api\/v1\/chat\/threads\/[^/]+\/messages$/, respond: () => ({ messages: [] }) },
  { method: 'GET', match: /^\/api\/v1\/chat\/threads\/[^/]+\/events/, respond: () => ({ events: [], has_more: false }) },
  { method: 'GET', match: /^\/api\/v1\/chat\/threads\/[^/]+\/context/, respond: () => ({ estimatedTokens: 0, budgetTokens: 0, segments: [] }) },
  { method: 'PUT', match: /^\/api\/v1\/chat\/threads\/[^/]+$/, respond: () => ({ ok: true }) },
  { method: 'DELETE', match: /^\/api\/v1\/chat\/threads/, respond: () => ({ ok: true }) },
  { match: /^\/api\/v1\/runs(\?.*)?$/, respond: () => ({ runs: [], items: [], next_cursor: null }) },
  { match: /^\/api\/v1\/orchestration\/runs\/[^/]+\/approvals$/, respond: () => ({ approvals: [nordlysApproval] }) },
  { match: /^\/api\/v1\/orchestration\/runs\/[^/]+\/proof-bundle$/, respond: () => ({}) },
  { match: /^\/api\/v1\/chat\/feedback$/, respond: () => ({ ok: true }) },
  // ── The answer ───────────────────────────────────────────────────────────
  {
    method: 'POST',
    match: /^\/api\/v1\/chat\/stream$/,
    respond: () => {
      if (!activeStream) return json({ error: { code: 'scene_not_registered', message: 'Ingen scene er registrert.' } }, 503)
      chatPostAt = performance.now()
      appendSceneLog('sse', 'POST /api/v1/chat/stream received; stream clock starts')
      return streamResponse(activeStream)
    },
  },
]

function requestMethod(input: RequestInfo | URL, init?: RequestInit): string {
  const fromInit = init?.method
  if (fromInit) return fromInit.toUpperCase()
  if (input instanceof Request) return input.method.toUpperCase()
  return 'GET'
}

function requestUrl(input: RequestInfo | URL): URL {
  const raw = input instanceof Request ? input.url : String(input)
  return new URL(raw, window.location.origin)
}

async function routeApi(input: RequestInfo | URL, init?: RequestInit): Promise<Response> {
  const url = requestUrl(input)
  const method = requestMethod(input, init)
  const path = url.pathname + url.search
  requestCounts.set(url.pathname, (requestCounts.get(url.pathname) ?? 0) + 1)
  const sceneAnswer = sceneExtraRoute(method, url, path)
  if (sceneAnswer) return sceneAnswer
  for (const route of routes) {
    if (route.method && route.method !== method) continue
    const match = path.match(route.match) ?? url.pathname.match(route.match)
    if (!match) continue
    const result = route.respond(match, { url, init })
    return result instanceof Response ? result : json(result)
  }
  appendSceneLog('unmatched-api', `${method} ${path}`)
  return json({})
}

export function installSceneRuntime(): void {
  if (originalFetch) return
  originalFetch = window.fetch
  installImageShim()
  try {
    previousSceneStorage = {
      locale: window.localStorage.getItem('verevon.locale'),
      browseWeb: window.localStorage.getItem('verevon.chat.browseWeb.v1'),
    }
    window.localStorage.setItem('verevon.locale', 'no')
    window.localStorage.setItem('verevon.chat.browseWeb.v1', '1')
  } catch {
    // Storage may be unavailable; the app's defaults are Norwegian and browse-on anyway.
  }
  const real = originalFetch
  window.fetch = ((input: RequestInfo | URL, init?: RequestInit) => {
    const url = requestUrl(input)
    const sameOrigin = url.origin === window.location.origin
    if (url.pathname.startsWith('/api/') || url.pathname === '/health') {
      return routeApi(input, init)
    }
    if (!sameOrigin) {
      appendSceneLog('blocked-external-fetch', `${url.origin}${url.pathname}`)
      return Promise.resolve(json({ error: { code: 'scene_external_fetch_blocked', message: 'Scene mode does not fetch external data.' } }, 503))
    }
    return real.call(window, input, init)
  }) as typeof window.fetch
  window.__verevonSceneRequestCounts = requestCounts
  appendSceneLog('runtime', `scene runtime installed (fictional now: ${FJORDFORM_NOW_ISO})`)
}

/** Restore all global overrides before leaving a capture scene. */
export function uninstallSceneRuntime(): void {
  if (!originalFetch) return
  window.fetch = originalFetch
  originalFetch = null
  if (originalImageSrcDescriptor) {
    Object.defineProperty(HTMLImageElement.prototype, 'src', originalImageSrcDescriptor)
    originalImageSrcDescriptor = undefined
  }
  if (originalSetAttribute) {
    Element.prototype.setAttribute = originalSetAttribute
    originalSetAttribute = null
  }
  imageShimInstalled = false
  activeStream = null
  activeExtras = null
  chatPostAt = null
  delete window.__verevonSceneRequestCounts
  try {
    if (previousSceneStorage) {
      for (const [key, value] of [
        ['verevon.locale', previousSceneStorage.locale],
        ['verevon.chat.browseWeb.v1', previousSceneStorage.browseWeb],
      ] as const) {
        if (value === null) window.localStorage.removeItem(key)
        else window.localStorage.setItem(key, value)
      }
    }
  } catch {
    // Storage may be unavailable during teardown, just as it may be at install.
  }
  previousSceneStorage = null
}
