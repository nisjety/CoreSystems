/**
 * The text a person selects in an artifact, as the selection edit (EP-3 S6) names it: a character range in the revision
 * on screen, and the SHA-256 of the text in that range. The gateway counts characters as code points, so the offsets
 * here do too; a DOM selection counts UTF-16 units and is converted.
 *
 * A range is taken only from a source view whose visible text is the artifact's text exactly. Rendered Markdown is not,
 * so a selection in it offers nothing and the person selects in the source view instead. The check reads the text the
 * view shows, not its markup, so it holds whatever the view renders.
 */

/** A range of the artifact's text the person selected: code-point offsets, and the selected text. */
export type ArtifactSelectionRange = {
  start: number
  end: number
  quote: string
}

/** What the panel hands the conversation to send one selection edit: the range, the version it was made on, and the quote's hash. */
export type ArtifactSelectionRequest = {
  artifactId: string
  title: string
  version: number
  start: number
  end: number
  quoteSha256: string
  instruction: string
}

/** Marks the element whose visible text is the artifact's source; a selection is read only inside an element that carries it. */
export const SOURCE_VIEW_ATTRIBUTE = 'data-artifact-source'

/** A line of a highlighted code view. The lines are joined by a newline that the DOM does not carry. */
export const SOURCE_LINE_SELECTOR = '.verevon-chat-artifact-code__line'

/** The number of code points in `text`, the unit the gateway counts a range in. */
export function codePointLength(text: string): number {
  return Array.from(text).length
}

/** The number of code points before the UTF-16 index `index` of `text`; the index is clamped to the text. */
function codePointIndex(text: string, index: number): number {
  return codePointLength(text.slice(0, Math.max(0, Math.min(index, text.length))))
}

/**
 * The range of `source` between two UTF-16 indices, in code points, with the text it covers. `null` when the indices
 * are equal or lie outside `source`.
 */
export function selectionRange(source: string, fromIndex: number, toIndex: number): ArtifactSelectionRange | null {
  const from = Math.min(fromIndex, toIndex)
  const to = Math.max(fromIndex, toIndex)
  if (from < 0 || to > source.length || from === to) return null
  const quote = source.slice(from, to)
  const start = codePointIndex(source, from)
  return { start, end: start + codePointLength(quote), quote }
}

/** The lowercase hex SHA-256 of the UTF-8 bytes of `text`, the form the gateway checks a quote in. */
export async function quoteSha256(text: string): Promise<string> {
  const digest = await globalThis.crypto.subtle.digest('SHA-256', new TextEncoder().encode(text))
  return Array.from(new Uint8Array(digest), (byte) => byte.toString(16).padStart(2, '0')).join('')
}

/** The text a source view shows, read from the DOM, with the text node each piece of it came from. */
export type SourceView = {
  text: string
  pieces: Array<{ node: Text; start: number }>
}

/**
 * The text a source view shows: its text nodes in document order, a newline before each code line after the first, and
 * nothing inside an `aria-hidden` subtree (the line-number gutter). The text equals the artifact's only when the view
 * shows the source exactly.
 */
export function readSourceView(root: Element): SourceView {
  const pieces: SourceView['pieces'] = []
  let text = ''
  let lines = 0
  const walker = root.ownerDocument.createTreeWalker(root, NodeFilter.SHOW_ELEMENT | NodeFilter.SHOW_TEXT, {
    acceptNode(node) {
      if (node.nodeType === Node.TEXT_NODE) return NodeFilter.FILTER_ACCEPT
      const element = node as Element
      if (element.getAttribute('aria-hidden') === 'true') return NodeFilter.FILTER_REJECT
      return element.matches(SOURCE_LINE_SELECTOR) ? NodeFilter.FILTER_ACCEPT : NodeFilter.FILTER_SKIP
    },
  })
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    if (node.nodeType === Node.ELEMENT_NODE) {
      if (lines > 0) text += '\n'
      lines += 1
      continue
    }
    pieces.push({ node: node as Text, start: text.length })
    text += node.textContent ?? ''
  }
  return { text, pieces }
}

/**
 * The index in `view.text` of a DOM position: a text node of the view and an offset into it. `null` for a position that
 * is not in the view's text (an element boundary, or the gutter), so that no selection is offered from it.
 */
export function sourceIndex(view: SourceView, node: Node, offset: number): number | null {
  const piece = view.pieces.find((entry) => entry.node === node)
  if (!piece) return null
  return piece.start + Math.max(0, Math.min(offset, piece.node.data.length))
}

/**
 * The range the person selected inside `root`, when `root`'s visible text is `content` exactly. `null` when nothing is
 * selected, the selection leaves `root`, or the view does not show the text as it is.
 */
export function sourceSelectionRange(root: Element, content: string): ArtifactSelectionRange | null {
  const selection = root.ownerDocument.getSelection()
  if (!selection || selection.isCollapsed) return null
  const { anchorNode, anchorOffset, focusNode, focusOffset } = selection
  if (!anchorNode || !focusNode || !root.contains(anchorNode) || !root.contains(focusNode)) return null
  const view = readSourceView(root)
  if (view.text !== content) return null
  const from = sourceIndex(view, anchorNode, anchorOffset)
  const to = sourceIndex(view, focusNode, focusOffset)
  if (from == null || to == null) return null
  return selectionRange(content, from, to)
}
