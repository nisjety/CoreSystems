/**
 * "Lagre som PDF" — goes through the browser's own print pipeline rather
 * than building PDF bytes: printing to "Save as PDF" is a feature every
 * browser already ships, so this stays a genuinely zero-dependency path (no
 * PDF layout engine bundled) while still producing a real, selectable-text
 * PDF instead of a screenshot.
 *
 * The technique is a hidden `<iframe>`, not `window.print()` on the page
 * itself: printing the live page would also print the sidebar, the composer,
 * every other message and all of this message's own buttons. The iframe
 * gets a copy of just the rendered answer, the page's stylesheets (so it
 * looks the same), and print-only CSS that hides interactive chrome and
 * sets the page size — then it alone is told to print.
 */

const PRINT_IFRAME_REMOVE_FALLBACK_MS = 15000

function copyDocumentStyles(target: Document): void {
  const nodes = document.querySelectorAll('link[rel="stylesheet"], style')
  for (const node of nodes) {
    target.head.appendChild(node.cloneNode(true))
  }
}

function printStyleSheet(): string {
  return `
    @page { size: A4; margin: 16mm; }
    html, body {
      background: #fff !important;
      margin: 0;
      padding: 0;
    }
    .verevon-chat-print-root {
      max-width: none !important;
    }
    .verevon-chat-message-actions,
    .verevon-chat-menu,
    .verevon-chat-feedback-reasons,
    button,
    [role="button"] {
      display: none !important;
    }
  `
}

/** Resolves once every `<img>` inside `root` has finished loading (or failed — a broken image must never hang the print). */
function waitForImages(root: HTMLElement): Promise<void> {
  const images = [...root.querySelectorAll('img')].filter((img) => !img.complete)
  if (images.length === 0) return Promise.resolve()
  return Promise.all(
    images.map(
      (img) =>
        new Promise<void>((resolve) => {
          img.addEventListener('load', () => resolve(), { once: true })
          img.addEventListener('error', () => resolve(), { once: true })
        }),
    ),
  ).then(() => undefined)
}

/** Resolves once the iframe document's own fonts have finished loading, or immediately if the browser has no `document.fonts` (a very old engine — nothing to wait for). */
function waitForFonts(target: Document): Promise<void> {
  const fonts = (target as Document & { fonts?: { ready: Promise<unknown> } }).fonts
  return fonts ? fonts.ready.then(() => undefined) : Promise.resolve()
}

/** Class of the hidden export twin that `InlineWidget` and `McpAppFrame` render inside their frame: the code a print shows. */
const EXPORT_TWIN_CLASS = 'verevon-inline-widget__export'
/** The frame roots a print never copies live: an inline widget, and an MCP app (its root carries both classes). */
const FRAME_SELECTOR = '.verevon-inline-widget, .verevon-mcp-app'

/**
 * The code one frame prints as: its export twin, moved out of the frame and
 * made visible; else a `<pre>` of its `data-source`; else `null`, so the frame
 * is dropped from the print rather than printed as a frame.
 */
function codeForFrame(frame: Element, target: Document): Element | null {
  const twin = Array.from(frame.children).find((child) => child.classList.contains(EXPORT_TWIN_CLASS))
  if (twin) {
    twin.removeAttribute('hidden')
    twin.remove()
    return twin
  }
  const source = frame.getAttribute('data-source')
  if (!source) return null
  const pre = target.createElement('pre')
  const code = target.createElement('code')
  code.textContent = source
  pre.appendChild(code)
  return pre
}

/**
 * Replaces every framed widget or app under `root` with its code, then drops
 * any iframe and toolbar still left there. `root` must be the print's own clone
 * of the answer: the live answer keeps its frames.
 */
function swapFramesForCode(root: Element, target: Document): void {
  for (const frame of root.querySelectorAll(FRAME_SELECTOR)) {
    const code = codeForFrame(frame, target)
    if (code) {
      frame.replaceWith(code)
    } else {
      frame.remove()
    }
  }
  for (const leftover of root.querySelectorAll('iframe, .verevon-inline-widget__toolbar')) {
    leftover.remove()
  }
}

/**
 * Renders `element`'s content into a hidden iframe styled like the page and
 * calls the browser's print dialog on it, so the person can "Save as PDF".
 * `title` becomes the iframe document's `<title>`, which is what browsers
 * offer as the default save-as filename. A widget or MCP app in the answer
 * prints as its code, never as its live frame (`swapFramesForCode`).
 */
export function printAnswerAsPdf(element: HTMLElement, title: string): void {
  const iframe = document.createElement('iframe')
  iframe.setAttribute('aria-hidden', 'true')
  iframe.style.position = 'fixed'
  iframe.style.right = '0'
  iframe.style.bottom = '0'
  iframe.style.width = '0'
  iframe.style.height = '0'
  iframe.style.border = '0'
  iframe.style.visibility = 'hidden'
  document.body.appendChild(iframe)

  let removed = false
  const remove = () => {
    if (removed) return
    removed = true
    iframe.contentWindow?.removeEventListener('afterprint', remove)
    iframe.remove()
  }
  // A fallback timeout: some browsers (or a print dialog the person just
  // cancels) never fire `afterprint` reliably. The iframe would otherwise sit
  // in the DOM forever.
  const fallback = window.setTimeout(remove, PRINT_IFRAME_REMOVE_FALLBACK_MS)

  const target = iframe.contentDocument
  if (!target) {
    window.clearTimeout(fallback)
    remove()
    return
  }

  target.open()
  target.write('<!DOCTYPE html><html><head></head><body></body></html>')
  target.close()
  target.title = title

  copyDocumentStyles(target)
  const style = target.createElement('style')
  style.textContent = printStyleSheet()
  target.head.appendChild(style)

  const root = target.createElement('div')
  root.className = 'verevon-chat-print-root'
  root.appendChild(element.cloneNode(true))
  swapFramesForCode(root, target)
  target.body.appendChild(root)

  void Promise.all([waitForFonts(target), waitForImages(root)]).then(() => {
    const win = iframe.contentWindow
    if (!win) {
      remove()
      return
    }
    win.addEventListener('afterprint', () => {
      window.clearTimeout(fallback)
      remove()
    })
    win.focus()
    win.print()
  })
}
