import { createSignal, onCleanup, Show } from 'solid-js'

/**
 * Gmail/Outlook-grade rendering of a message body.
 *
 * HTML emails render inside a SANDBOXED iframe (no `allow-scripts`), which is
 * the same isolation strategy Gmail/Outlook/Superhuman use: the email's own
 * CSS can't leak into the app, and its scripts never execute. We keep
 * `allow-same-origin` (safe precisely because scripts are disabled) so the
 * parent can measure content height for auto-sizing. Quoted reply history is
 * collapsed behind a native <details> disclosure — no script needed inside the
 * sandbox. Plain-text bodies are linkified with whitespace preserved.
 */

const QUOTE_MARKERS = [
  'class="gmail_quote"',
  'class="gmail_attr"',
  '-----Original Message-----',
  'id="appendonsend"',
  'id="divRplyFwdMsg"',
  'border-top:solid #E1E1E1',
  'border-top: solid #e1e1e1',
  'WordSection1',
]

// Strip anything that could execute or auto-navigate. The sandbox already
// blocks scripts; this is defense-in-depth (and future-proofs against an
// accidental allow-scripts).
function sanitizeEmailHtml(html: string): string {
  return html
    .replace(/<script[\s\S]*?<\/script>/gi, '')
    .replace(/<iframe[\s\S]*?<\/iframe>/gi, '')
    .replace(/<object[\s\S]*?<\/object>/gi, '')
    .replace(/<embed[\s\S]*?>/gi, '')
    .replace(/<base[^>]*>/gi, '')
    .replace(/<meta[^>]*http-equiv[^>]*>/gi, '')
    .replace(/\son\w+\s*=\s*"[^"]*"/gi, '')
    .replace(/\son\w+\s*=\s*'[^']*'/gi, '')
    .replace(/\son\w+\s*=\s*[^\s>]+/gi, '')
    .replace(/javascript:/gi, '')
}

// Find the earliest quoted-history boundary and split the email into the fresh
// reply (main) and the collapsible quoted tail.
function splitQuotedHtml(html: string): { main: string; quoted: string } {
  let cut = -1
  for (const marker of QUOTE_MARKERS) {
    const idx = html.indexOf(marker)
    if (idx !== -1 && (cut === -1 || idx < cut)) cut = idx
  }
  // Also match a bare <blockquote> that begins the quoted history.
  const bq = html.search(/<blockquote/i)
  if (bq !== -1 && (cut === -1 || bq < cut)) cut = bq
  if (cut <= 0) return { main: html, quoted: '' }
  // Rewind to the start of the enclosing tag so we don't split mid-element.
  const tagStart = html.lastIndexOf('<', cut)
  const at = tagStart === -1 ? cut : tagStart
  return { main: html.slice(0, at), quoted: html.slice(at) }
}

function buildSrcDoc(html: string): string {
  const { main, quoted } = splitQuotedHtml(sanitizeEmailHtml(html))
  const quotedBlock = quoted
    ? `<details class="vln-quote"><summary aria-label="Show quoted text">&#8230;</summary>${quoted}</details>`
    : ''
  return `<!doctype html><html><head><meta charset="utf-8">
<base target="_blank" rel="noopener noreferrer">
<style>
  :root { color-scheme: light dark; }
  html,body { margin:0; padding:0; }
  body {
    font-family: ui-sans-serif, -apple-system, "Segoe UI", Roboto, Helvetica, Arial, sans-serif;
    font-size: 14px; line-height: 1.55; color: #1a1a19;
    padding: 2px; word-break: break-word; overflow-wrap: anywhere;
  }
  img { max-width: 100%; height: auto; }
  table { max-width: 100%; }
  a { color: #b0532b; }
  blockquote { margin: 8px 0; padding-left: 12px; border-left: 2px solid #d3cec6; color: #626260; }
  .vln-quote { margin-top: 10px; }
  .vln-quote > summary {
    cursor: pointer; list-style: none; display: inline-flex; align-items: center;
    gap: 4px; padding: 1px 8px; border: 1px solid #d3cec6; border-radius: 9999px;
    color: #7b7b78; font-size: 12px; letter-spacing: 2px; line-height: 1.4; user-select: none;
  }
  .vln-quote > summary::-webkit-details-marker { display: none; }
  @media (prefers-color-scheme: dark) { body { color: #e6e4e1; } }
</style></head><body>${main}${quotedBlock}</body></html>`
}

function SafeEmailFrame(props: { html: string }) {
  const [height, setHeight] = createSignal(120)
  let frame: HTMLIFrameElement | undefined
  let observer: ResizeObserver | undefined

  const measure = () => {
    const doc = frame?.contentDocument
    if (!doc?.body) return
    const next = Math.min(Math.max(doc.body.scrollHeight + 6, 40), 4000)
    setHeight(next)
  }

  const onLoad = () => {
    measure()
    const doc = frame?.contentDocument
    if (!doc) return
    // Re-measure once remote images finish loading (they change layout height).
    doc.querySelectorAll('img').forEach((img) => {
      if (!(img as HTMLImageElement).complete) img.addEventListener('load', measure, { once: true })
    })
    if ('ResizeObserver' in window && doc.body) {
      observer = new ResizeObserver(() => measure())
      observer.observe(doc.body)
    }
  }

  onCleanup(() => observer?.disconnect())

  return (
    <iframe
      ref={frame}
      class="velion-inbox-email-frame"
      sandbox="allow-same-origin allow-popups allow-popups-to-escape-sandbox"
      srcdoc={buildSrcDoc(props.html)}
      onLoad={onLoad}
      style={{ height: `${height()}px` }}
      title="Message content"
      loading="lazy"
    />
  )
}

const HTML_HINT = /<\/?(?:div|p|table|br|span|a|img|blockquote|html|body|ul|ol|li|h[1-6]|strong|b|em)\b/i

export function EmailBody(props: { html?: string; text?: string }) {
  const isHtml = () => {
    const h = props.html?.trim()
    return Boolean(h && HTML_HINT.test(h))
  }
  return (
    <Show when={isHtml()} fallback={<PlainTextBody text={props.text || props.html || ''} />}>
      <SafeEmailFrame html={props.html!} />
    </Show>
  )
}

// Plain-text fallback: preserve line breaks and turn bare URLs into links.
function PlainTextBody(props: { text: string }) {
  const segments = () => {
    const clean = props.text.trim()
    if (!clean) return [] as Array<{ url: boolean; value: string }>
    return clean.split(/(https?:\/\/[^\s<]+)/g).map((part) => ({
      url: /^https?:\/\//.test(part),
      value: part,
    }))
  }
  return (
    <Show when={props.text.trim()} fallback={<span class="velion-inbox-article__empty">No message body.</span>}>
      <div class="velion-inbox-email-plain">
        {segments().map((seg) =>
          seg.url ? (
            <a href={seg.value} target="_blank" rel="noopener noreferrer">
              {seg.value}
            </a>
          ) : (
            <span>{seg.value}</span>
          ),
        )}
      </div>
    </Show>
  )
}
