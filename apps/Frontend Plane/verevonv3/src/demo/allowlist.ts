/**
 * The links the interactive demo follows. Every href a visitor clicks is checked
 * here: an allowed href is taken by the in-demo router, anything else gets the
 * demo's notice and no navigation.
 *
 * Only plain same-origin paths of the demo's own pages pass: `/dashboard` (with
 * its home tabs `?tab=web` or `?tab=extract`), `/chat`, and `/chat/<thread id>`.
 * Absolute URLs, other origins, protocol-relative and backslash forms, mailto:,
 * tel:, javascript: and data: are refused, as are spaces, control characters,
 * percent-encoding and dot segments (they could normalise into an allowed path).
 */

/** A reserved name (RFC 2606) used only as the base for parsing; it never resolves. */
const DEMO_ORIGIN = 'https://demo.invalid'

const allowedPath = /^\/(?:dashboard|chat(?:\/[A-Za-z0-9_-]+)?)$/
const allowedDashboardTabs: ReadonlySet<string> = new Set(['web', 'extract'])

export function demoAllows(href: string): boolean {
  if (typeof href !== 'string' || href.length === 0) return false
  if (!href.startsWith('/') || href.startsWith('//')) return false
  if (/[\s\\%\u0000-\u001f\u007f]/.test(href)) return false

  let url: URL
  try {
    url = new URL(href, DEMO_ORIGIN)
  } catch {
    return false
  }
  // The path as written must be the path the URL parser yields: no dot segments.
  const writtenPath = href.split(/[?#]/, 1)[0]
  if (url.origin !== DEMO_ORIGIN || url.pathname !== writtenPath || !allowedPath.test(url.pathname)) return false

  for (const [key, value] of url.searchParams) {
    if (url.pathname !== '/dashboard' || key !== 'tab' || !allowedDashboardTabs.has(value)) return false
  }
  return true
}
