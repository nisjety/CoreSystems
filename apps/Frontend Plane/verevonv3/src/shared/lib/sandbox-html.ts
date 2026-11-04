/**
 * Wrap untrusted HTML for the chat/artifact preview iframe.
 *
 * The iframe's sandbox removes same-origin access and navigation privileges;
 * this CSP closes the remaining ambient network and embedding channels. Inline
 * scripts/styles remain available for a generated preview, but they cannot
 * load remote code, submit forms, open child frames, or exfiltrate via fetch or
 * WebSocket.
 */
const PREVIEW_CSP = [
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

const CSP_META = `<meta http-equiv="Content-Security-Policy" content="${PREVIEW_CSP}">`

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
 */
export function sandboxWidgetDocument(html: string, themeCss: string, options: { bridgeShim?: string } = {}): string {
  const source = html.trim()
  const shim = options.bridgeShim ? `<script>${options.bridgeShim}</script>` : ''
  return `<!doctype html><html><head>${CSP_META}<style>${themeCss}${WIDGET_BASE_CSS}</style>${shim}</head><body>${source}</body></html>`
}
