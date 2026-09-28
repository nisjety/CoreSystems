/**
 * "Eksporter til Word/Excel/PowerPoint" / "Lagre som PDF" — the single entry
 * point `ChatMessages.tsx` calls. Everything downstream (`docx.ts`,
 * `xlsx.ts`, `pptx.ts`, `zip.ts`, `ooxml.ts`) is pure and DOM-free; this file
 * is the one place that's allowed to touch the DOM (`download.ts`,
 * `print.ts`) and the real markdown parser (`parseMarkdownBlocks`, which
 * lives in `chat-media-markdown.tsx` alongside solid-js JSX this directory's
 * builders otherwise stay clear of).
 *
 * Nothing here makes a network request: parsing, building the archive and
 * saving the file all happen in the browser's own memory, which is also why
 * this is a genuine privacy feature, not just a convenience one.
 */

import { createPreview, parseMarkdownBlocks, slugFileName } from '../../components/chat-media-markdown'
import type { MarkdownBlock } from '../../components/chat-types'
import { buildDocx } from './docx'
import { buildXlsx } from './xlsx'
import { buildPptx } from './pptx'
import { downloadBytes } from './download'
import { printAnswerAsPdf } from './print'
import { leadHeadingText, stripInlineMarkdown } from './ooxml'
import type { ExportLocale, ExportMeta, ExportSource } from './types'

export type { ExportLocale, ExportMeta, ExportSource } from './types'
export { detectExcelNumber, sanitizeSheetName } from './xlsx'
export { buildDocx } from './docx'
export { buildXlsx } from './xlsx'
export { buildPptx } from './pptx'
export { downloadBytes } from './download'
export { printAnswerAsPdf } from './print'

export type ExportFormat = 'docx' | 'xlsx' | 'pptx' | 'pdf'

/**
 * Microsoft Copilot shows its export button on answers of 600 characters or
 * more (blogs.windows.com, 2025-10-09); shorter ones keep the choices in the
 * overflow menu only. Measured with `answerExportSize`.
 */
export const EXPORT_BUTTON_MIN_CHARS = 600

export type ExportChatAnswerInput = {
  /** Raw markdown, exactly as the chat bubble renders it. */
  content: string
  title?: string
  /** ISO 8601. Defaults to "now" wherever a builder needs a date and this is absent. */
  createdAt?: string
  sources?: ExportSource[]
  /** Required for `format: 'pdf'` — the rendered answer DOM node `print.ts` clones into its hidden iframe. Absent (or null) for any other format, which never touches the DOM. */
  element?: HTMLElement | null
  locale?: ExportLocale
}

const MIME_TYPES: Record<Exclude<ExportFormat, 'pdf'>, string> = {
  docx: 'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
  xlsx: 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet',
  pptx: 'application/vnd.openxmlformats-officedocument.presentationml.presentation',
}

/**
 * The document's title, best source first: a title the caller actually has,
 * then the heading the answer opens with, then a short preview of its first
 * text block with the markdown syntax removed (a preview cut from the raw
 * markdown put "##" and "**" into the title and the file name), and only
 * then a plain "Svar"/"Answer".
 */
function resolveTitle(input: ExportChatAnswerInput, locale: ExportLocale, blocks: readonly MarkdownBlock[]): string {
  const explicit = input.title?.trim()
  if (explicit) return explicit
  const heading = leadHeadingText(blocks)
  if (heading) return heading
  const preview = createPreview(firstBlockText(blocks), 60)
  if (preview) return preview
  return locale === 'en' ? 'Answer' : 'Svar'
}

/** Plain text of the first block that reads as prose, '' when there is none. */
function firstBlockText(blocks: readonly MarkdownBlock[]): string {
  for (const block of blocks) {
    if (block.kind === 'paragraph' || block.kind === 'heading') return stripInlineMarkdown(block.text)
    if (block.kind === 'list' && block.items[0]) return stripInlineMarkdown(block.items[0].text)
    if (block.kind === 'quote' && block.lines[0]) return stripInlineMarkdown(block.lines[0].text)
  }
  return ''
}

function fileDateStamp(createdAt: string | undefined): string {
  const parsed = createdAt ? new Date(createdAt) : new Date()
  const date = Number.isNaN(parsed.getTime()) ? new Date() : parsed
  const pad = (value: number) => String(value).padStart(2, '0')
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`
}

function buildFileBaseName(input: ExportChatAnswerInput, locale: ExportLocale, blocks: readonly MarkdownBlock[]): string {
  return `${slugFileName(resolveTitle(input, locale, blocks))}-${fileDateStamp(input.createdAt)}`
}

function toExportMeta(input: ExportChatAnswerInput, locale: ExportLocale, blocks: readonly MarkdownBlock[]): ExportMeta {
  return {
    title: resolveTitle(input, locale, blocks),
    createdAt: input.createdAt,
    sources: input.sources,
    locale,
  }
}

/**
 * How much visible text an answer renders as, markdown syntax discarded —
 * the same signal Microsoft Copilot's export button uses to decide whether
 * to show itself at all (its threshold is 600 characters; see
 * `ChatMessages.tsx`'s use of this). Pure text-in, number-out, so the UI can
 * call it on every token of a still-streaming answer without touching the
 * DOM or re-running it against a debounce.
 */
export function answerExportSize(content: string): number {
  return visiblePlainTextLength(parseMarkdownBlocks(content))
}

function visiblePlainTextLength(blocks: readonly MarkdownBlock[]): number {
  let total = 0
  for (const block of blocks) {
    switch (block.kind) {
      case 'heading':
      case 'paragraph':
        total += stripInlineMarkdown(block.text).length
        break
      case 'list':
        for (const item of block.items) total += stripInlineMarkdown(item.text).length
        break
      case 'quote':
        for (const line of block.lines) total += stripInlineMarkdown(line.text).length
        break
      case 'code':
      case 'math':
        total += block.text.length
        break
      case 'table':
        for (const cell of block.header) total += stripInlineMarkdown(cell).length
        for (const row of block.rows) {
          for (const cell of row) total += stripInlineMarkdown(cell).length
        }
        break
      case 'details':
        total += stripInlineMarkdown(block.summary).length + visiblePlainTextLength(block.blocks)
        break
      case 'hr':
        break
    }
  }
  return total
}

/**
 * Builds (docx/xlsx/pptx) or prints (pdf) one finished answer and hands it
 * to the browser to save. Throws on failure — the caller (the message's
 * export menu) is expected to catch it and show a brief inline notice; see
 * `ChatMessages.tsx`.
 */
export function exportChatAnswer(format: ExportFormat, input: ExportChatAnswerInput): void {
  const locale = input.locale ?? 'no'
  const blocks = parseMarkdownBlocks(input.content)

  if (format === 'pdf') {
    if (!input.element) {
      throw new Error('PDF export requires the rendered answer element.')
    }
    printAnswerAsPdf(input.element, buildFileBaseName(input, locale, blocks))
    return
  }

  const meta = toExportMeta(input, locale, blocks)
  const bytes =
    format === 'docx' ? buildDocx(blocks, meta) : format === 'xlsx' ? buildXlsx(blocks, meta) : buildPptx(blocks, meta)
  const fileName = `${buildFileBaseName(input, locale, blocks)}.${format}`
  downloadBytes(bytes, fileName, MIME_TYPES[format])
}
