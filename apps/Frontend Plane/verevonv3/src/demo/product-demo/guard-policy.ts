/**
 * Pure policy of the demo's privacy guard. guard.ts applies these decisions to the
 * browser's network and storage APIs. Keeping them free of side effects makes them
 * unit testable (guard-policy.test.ts).
 */

/** Every console line the guard writes starts with this, so the probe can recognise them. */
export const GUARD_LOG_PREFIX = '[demo-guard]'

/** Body of the 404 the guard answers for a product API call that no responder handled. */
export const DEMO_UNAVAILABLE_BODY = {
  error: { code: 'demo_unavailable', message: 'Dette er ikke med i demoen.' },
} as const

export type NetworkVerdict =
  | { action: 'allow' }
  | { action: 'unhandled-api'; path: string }
  | { action: 'block'; reason: string }

export type TransportKind = 'websocket' | 'eventsource' | 'beacon'

function parseUrl(input: string, documentBaseUrl: string): URL | null {
  try {
    return new URL(input, documentBaseUrl)
  } catch {
    return null
  }
}

/** The product's API lives under /api and its health probe under /health. The network never answers them. */
export function isApiPath(pathname: string): boolean {
  return pathname === '/api' || pathname.startsWith('/api/') || pathname === '/health'
}

/**
 * Decides one fetch or XHR request. Only GET and HEAD of the demo's own static files,
 * under `demoBase`, may reach the network. `documentBaseUrl` is the demo document's URL:
 * the frame itself has an opaque origin, but it keeps the URL it was loaded from.
 */
export function classifyRequest(
  request: { url: string; method: string },
  documentBaseUrl: string,
  demoBase: string,
): NetworkVerdict {
  const target = parseUrl(request.url, documentBaseUrl)
  if (!target) return { action: 'block', reason: 'invalid URL' }
  if (target.protocol === 'data:' || target.protocol === 'blob:') return { action: 'allow' }
  if (target.origin !== new URL(documentBaseUrl).origin) {
    return { action: 'block', reason: `third-party origin ${target.origin}` }
  }
  if (isApiPath(target.pathname)) return { action: 'unhandled-api', path: target.pathname }
  const method = request.method.toUpperCase()
  if (method !== 'GET' && method !== 'HEAD') {
    return { action: 'block', reason: `${method} ${target.pathname}` }
  }
  if (target.pathname.startsWith(demoBase)) return { action: 'allow' }
  return { action: 'block', reason: `outside the demo ${target.pathname}` }
}

/** WebSocket, EventSource and sendBeacon never reach the network in the demo. The log names only the origin. */
export function classifyTransport(kind: TransportKind, url: string, documentBaseUrl: string): NetworkVerdict {
  const origin = parseUrl(url, documentBaseUrl)?.origin ?? 'invalid URL'
  return { action: 'block', reason: `${kind} to ${origin}` }
}

/** The path (with query and hash) an anchor points to inside the demo, or null when it leaves the demo. */
export function internalPath(href: string, documentBaseUrl: string): string | null {
  const target = parseUrl(href, documentBaseUrl)
  if (!target || target.origin !== new URL(documentBaseUrl).origin) return null
  return `${target.pathname}${target.search}${target.hash}`
}

/**
 * Product routes the demo does not offer, although the allowlist (src/demo/allowlist.ts) matches their
 * paths. The product serves them under /chat beside the threads. The demo shows the notice for a link to
 * them and the unavailable page for a direct route.
 */
const UNOFFERED_PATHS: ReadonlySet<string> = new Set(['/chat/findings', '/chat/reports', '/chat/routines'])

/** True when the demo must not show this path (a query and a hash are ignored). */
export function isUnofferedPath(path: string): boolean {
  return UNOFFERED_PATHS.has(path.split(/[?#]/, 1)[0] ?? '')
}
