/**
 * Pure markdown formatting transforms for the room composer's toolbar (WS15-S3). No DOM and no
 * state: a value and a selection go in, a new value and selection come out, so the textarea
 * stays the single source of truth and the same function serves the buttons and the shortcuts.
 */

export type MarkdownFormatKind = 'bold' | 'italic' | 'code' | 'link' | 'bullet' | 'quote'

export interface MarkdownEdit {
  readonly value: string
  readonly selStart: number
  readonly selEnd: number
}

const LINK_PLACEHOLDER = 'https://'

function clampSelection(value: string, selStart: number, selEnd: number): [number, number] {
  const a = Math.max(0, Math.min(selStart, value.length))
  const b = Math.max(0, Math.min(selEnd, value.length))
  return a <= b ? [a, b] : [b, a]
}

/** Wrap the selection in `mark`; wrapping an already-wrapped selection removes the marks. */
function wrap(value: string, start: number, end: number, mark: string): MarkdownEdit {
  const before = value.slice(0, start)
  const selected = value.slice(start, end)
  const after = value.slice(end)
  // Marks sit just outside the selection: unwrap.
  if (before.endsWith(mark) && after.startsWith(mark)) {
    return {
      value: before.slice(0, before.length - mark.length) + selected + after.slice(mark.length),
      selStart: start - mark.length,
      selEnd: end - mark.length,
    }
  }
  // Marks sit just inside the selection: unwrap.
  if (selected.length >= mark.length * 2 && selected.startsWith(mark) && selected.endsWith(mark)) {
    const inner = selected.slice(mark.length, selected.length - mark.length)
    return { value: before + inner + after, selStart: start, selEnd: start + inner.length }
  }
  return {
    value: before + mark + selected + mark + after,
    selStart: start + mark.length,
    selEnd: end + mark.length,
  }
}

/** The start of the first line and the end of the last line the selection touches. */
function lineBounds(value: string, start: number, end: number): [number, number] {
  const lineStart = value.lastIndexOf('\n', start - 1) + 1
  // A selection that ends right after a newline does not include the next line.
  const probe = end > start && value[end - 1] === '\n' ? end - 1 : end
  const nextBreak = value.indexOf('\n', probe)
  return [lineStart, nextBreak === -1 ? value.length : nextBreak]
}

/** Prefix every touched line with `prefix`; when all already carry it, remove it. */
function prefixLines(value: string, start: number, end: number, prefix: string): MarkdownEdit {
  const [lineStart, lineEnd] = lineBounds(value, start, end)
  const lines = value.slice(lineStart, lineEnd).split('\n')
  const allPrefixed = lines.every((line) => line.startsWith(prefix))
  const next = lines.map((line) => (allPrefixed ? line.slice(prefix.length) : `${prefix}${line}`))
  const block = next.join('\n')
  const delta = block.length - (lineEnd - lineStart)
  const firstDelta = allPrefixed ? -prefix.length : prefix.length
  return {
    value: value.slice(0, lineStart) + block + value.slice(lineEnd),
    selStart: Math.max(lineStart, start + firstDelta),
    selEnd: Math.max(lineStart, end + delta),
  }
}

function link(value: string, start: number, end: number): MarkdownEdit {
  const before = value.slice(0, start)
  const selected = value.slice(start, end)
  const after = value.slice(end)
  if (selected.length === 0) {
    const text = '[](' + LINK_PLACEHOLDER + ')'
    // Caret inside the empty label.
    return { value: before + text + after, selStart: start + 1, selEnd: start + 1 }
  }
  const urlLike = /^(https?:\/\/|mailto:)\S+$/i.test(selected)
  if (urlLike) {
    // The selection is the address: label it, select the label to type over it.
    return { value: `${before}[](${selected})${after}`, selStart: start + 1, selEnd: start + 1 }
  }
  const head = `[${selected}](`
  const urlStart = start + head.length
  return {
    value: `${before}${head}${LINK_PLACEHOLDER})${after}`,
    selStart: urlStart,
    selEnd: urlStart + LINK_PLACEHOLDER.length,
  }
}

/** Apply one formatting action to `value` with the selection `[selStart, selEnd)`. */
export function applyFormat(value: string, selStart: number, selEnd: number, kind: MarkdownFormatKind): MarkdownEdit {
  const [start, end] = clampSelection(value, selStart, selEnd)
  switch (kind) {
    case 'bold':
      return wrap(value, start, end, '**')
    case 'italic':
      return wrap(value, start, end, '_')
    case 'code':
      return wrap(value, start, end, '`')
    case 'link':
      return link(value, start, end)
    case 'bullet':
      return prefixLines(value, start, end, '- ')
    case 'quote':
      return prefixLines(value, start, end, '> ')
  }
}

/** The format a Ctrl/Cmd shortcut stands for, or undefined. */
export function markdownShortcut(key: string): MarkdownFormatKind | undefined {
  switch (key.toLowerCase()) {
    case 'b':
      return 'bold'
    case 'i':
      return 'italic'
    case 'k':
      return 'link'
    default:
      return undefined
  }
}
