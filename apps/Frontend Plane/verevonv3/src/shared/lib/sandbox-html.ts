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
  if (/<head(?:\s[^>]*)?>/i.test(source)) {
    return source.replace(/<head(?:\s[^>]*)?>/i, (head) => `${head}${CSP_META}`)
  }
  return `<!doctype html><html><head>${CSP_META}</head><body>${source}</body></html>`
}
