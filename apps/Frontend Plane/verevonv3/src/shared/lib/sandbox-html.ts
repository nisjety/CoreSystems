/**
 * Wrap untrusted HTML for the chat/artifact preview iframe.
 *
 * The iframe's sandbox removes same-origin access and navigation privileges;
 * this CSP closes the remaining ambient network and embedding channels. Inline
 * scripts/styles remain available for a generated preview, but they cannot
 * load remote code, submit forms, open child frames, or exfiltrate via fetch or
 * WebSocket.
 *
 * An MCP app frame may load a wider policy the server built (W4-M2), but only one
 * `validatedAppCsp` accepts: these directives, with https origins added to
 * `connect-src`, `img-src`, `font-src` and `media-src` and nowhere else.
 */
export const PREVIEW_CSP = [
  "default-src 'none'",
  "base-uri 'none'",
  "child-src 'none'",
  "connect-src 'none'",
  "font-src data:",
  "form-action 'none'",
  "frame-ancestors 'none'",
  "frame-src 'none'",
  "img-src data: blob:",
  "media-src data: blob:",
  "object-src 'none'",
  "script-src 'unsafe-inline'",
  "style-src 'unsafe-inline'",
  "worker-src 'none'",
].join('; ')

function cspMeta(policy: string): string {
  return `<meta http-equiv="Content-Security-Policy" content="${policy}">`
}

const CSP_META = cspMeta(PREVIEW_CSP)

/** One directive of a policy: its name and the sources it lists, in order. */
type CspDirective = { name: string; sources: string[] }

/** Split a policy into its directives. An empty part (a trailing `;`) is skipped. */
function parseCspDirectives(policy: string): CspDirective[] {
  const directives: CspDirective[] = []
  for (const part of policy.split(';')) {
    const tokens = part.trim().split(/\s+/)
    const name = tokens[0]
    if (name) directives.push({ name, sources: tokens.slice(1) })
  }
  return directives
}

/** The directives of `PREVIEW_CSP`, in its order: an app's policy names each of them, and no other. */
const PREVIEW_DIRECTIVES = parseCspDirectives(PREVIEW_CSP)

/**
 * The only directives an app's policy may add https origins to. `connect-src` lists origins in place of its
 * `'none'`; the others keep the sources `PREVIEW_CSP` gives them (`data:`, `blob:`) and add origins after them.
 */
const APP_ORIGIN_DIRECTIVES: ReadonlySet<string> = new Set(['connect-src', 'img-src', 'font-src', 'media-src'])

/** The longest origin accepted, as the server bounds one (`MAX_ID_LEN` in mp-contracts). */
const MAX_ORIGIN_LENGTH = 200

/**
 * One canonical https origin, written as the server writes one (`is_https_origin` in mp-contracts): `https://`,
 * lowercase DNS labels, and an optional port with no sign and no leading zero. No path, userinfo, wildcard or upper
 * case. The port's range is checked in `isHttpsOrigin`.
 */
const HTTPS_ORIGIN =
  /^https:\/\/[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?(?:\.[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?)*(?::([1-9][0-9]{0,4}))?$/

function isHttpsOrigin(source: string): boolean {
  if (source.length > MAX_ORIGIN_LENGTH) return false
  const match = HTTPS_ORIGIN.exec(source)
  return match !== null && (match[1] === undefined || Number(match[1]) <= 65535)
}

function sameSources(a: readonly string[], b: readonly string[]): boolean {
  return a.length === b.length && a.every((source, index) => source === b[index])
}

/**
 * The sources one directive of an app's policy may carry: its `PREVIEW_CSP` sources, or those with https origins
 * added where `APP_ORIGIN_DIRECTIVES` allows it. `null` for anything else.
 */
function acceptedSources(base: CspDirective, offered: readonly string[]): readonly string[] | null {
  if (sameSources(offered, base.sources)) return base.sources
  if (!APP_ORIGIN_DIRECTIVES.has(base.name)) return null
  if (base.name === 'connect-src') {
    // `'none'` is the no-origin source, so a listed origin replaces it: the list must be origins only.
    return offered.length > 0 && offered.every(isHttpsOrigin) ? offered : null
  }
  const kept = offered.slice(0, base.sources.length)
  const added = offered.slice(base.sources.length)
  return sameSources(kept, base.sources) && added.length > 0 && added.every(isHttpsOrigin) ? offered : null
}

/**
 * The policy an MCP app frame may load, or `null` (W4-M2). The server builds it from the origins an app declares and
 * the org allows, and the client does not take it on trust: it must name every `PREVIEW_CSP` directive, in order, and
 * differ only by https origins added to `connect-src`, `img-src`, `font-src` and `media-src`. A wildcard, a path, a
 * keyword, another directive or a missing one gives `null`, and the caller keeps `PREVIEW_CSP`. The result is rebuilt
 * from the accepted sources, so it carries nothing the check did not see.
 */
export function validatedAppCsp(csp: string): string | null {
  const offered = parseCspDirectives(csp)
  if (offered.length !== PREVIEW_DIRECTIVES.length) return null
  const lines: string[] = []
  for (const [index, base] of PREVIEW_DIRECTIVES.entries()) {
    const directive = offered[index]
    if (!directive || directive.name !== base.name) return null
    const sources = acceptedSources(base, directive.sources)
    if (sources === null) return null
    lines.push([base.name, ...sources].join(' '))
  }
  return lines.join('; ')
}

export function sandboxHtmlDocument(html: string): string {
  const source = html.trim()
  // Always our own head first: splicing into the artifact's first `<head>`
  // match could hit a comment or a script string and leave no CSP at all.
  return `<!doctype html><html><head>${CSP_META}</head><body>${source}</body></html>`
}

/**
 * Margins off and the host's font and text colour as defaults; the body stays
 * transparent so the widget sits on the transcript's own surface.
 */
const WIDGET_BASE_CSS =
  ':root{color-scheme:var(--vv-color-scheme,normal)}' +
  'html,body{margin:0;background:transparent}' +
  'body{color:var(--vv-text,inherit);font-family:var(--vv-font-family,system-ui,sans-serif);font-size:var(--vv-font-size,15px)}'

/**
 * Wrap an inline chat widget (EP-4 S1): the same CSP first, then the host theme
 * (`themeCss`, the `:root{--vv-*}` rule from `widgetThemeCss`) and a base reset,
 * then the widget. `themeCss` is host-built from filtered token values; the
 * widget's own markup only ever lands in the body.
 *
 * With `bridgeShim` (EP-4 S3, the bridge switch on) the host's own script goes
 * after the theme and before the body, so `window.verevon` exists before any
 * widget script runs. Without it the document is exactly the S1 one.
 *
 * With `csp` (W4-M2, an MCP app frame) the document carries that policy in place of `PREVIEW_CSP`, but only when
 * `validatedAppCsp` accepts it; otherwise, and without `csp`, it carries `PREVIEW_CSP`.
 */
export function sandboxWidgetDocument(
  html: string,
  themeCss: string,
  options: { bridgeShim?: string; csp?: string } = {},
): string {
  const source = html.trim()
  const shim = options.bridgeShim ? `<script>${options.bridgeShim}</script>` : ''
  const policy = options.csp === undefined ? null : validatedAppCsp(options.csp)
  const meta = policy === null ? CSP_META : cspMeta(policy)
  return `<!doctype html><html><head>${meta}<style>${themeCss}${WIDGET_BASE_CSS}</style>${shim}</head><body>${source}</body></html>`
}
