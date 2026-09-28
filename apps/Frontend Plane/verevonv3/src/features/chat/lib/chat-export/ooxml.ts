/**
 * OOXML text plumbing shared by the docx/xlsx/pptx builders: XML escaping
 * that keeps a well-formed part even when the model's answer contains bytes
 * XML 1.0 cannot carry at all, and an inline-markdown tokenizer that mirrors
 * `parseInline`'s grammar in `chat-media-markdown.tsx` closely enough that an
 * exported run and the on-screen chat bubble never disagree about what a
 * stretch of text means.
 *
 * Deliberately NOT importing from `chat-media-markdown.tsx`: that file pulls
 * in solid-js JSX and KaTeX, neither of which means anything outside a
 * browser — and nothing at all to a plain `node --experimental-strip-types`
 * run of `sample-exports.mts`, which has no JSX transform. This module
 * borrows the GRAMMAR only, not the code, and says so at each borrowed
 * pattern.
 */

// Explicit `.ts` extension (`allowImportingTsExtensions` in tsconfig.app.json
// permits it): every file in this directory except `index.ts`/`download.ts`/
// `print.ts` must also be importable by a plain `node --experimental-strip-types`
// run of `sample-exports.mts`, and Node's ESM resolver — unlike Vite's —
// requires the extension on a relative specifier.
import type { MarkdownBlock } from '../../components/chat-types'
import type { ExportLocale } from './types.ts'

export type InlineRun = {
  text: string
  bold?: boolean
  italic?: boolean
  code?: boolean
  /** Set only for a link/bare-URL token; never combined with bold/italic/code — see `tokenizeInline`. */
  href?: string
}

/**
 * XML 1.0's legal character set is `#x9 | #xA | #xD | [#x20-#xD7FF] |
 * [#xE000-#xFFFD] | [#x10000-#x10FFFF]` (production [2] of the spec) — every
 * other UTF-16 code unit is simply not representable, and Word/Excel/
 * PowerPoint reject or silently corrupt a part that contains one rather than
 * politely dropping it. A model's answer can end up with either kind: pasted
 * terminal output carries raw C0 control bytes, and a stream that ends
 * mid-token can cut a multi-byte emoji in half, leaving a lone surrogate
 * behind with no partner to pair it into a real code point.
 */
export function sanitizeXmlText(text: string): string {
  let out = ''
  for (let i = 0; i < text.length; i += 1) {
    const code = text.charCodeAt(i)
    if (code >= 0xd800 && code <= 0xdbff) {
      const next = text.charCodeAt(i + 1)
      if (next >= 0xdc00 && next <= 0xdfff) {
        out += text.charAt(i) + text.charAt(i + 1)
        i += 1
        continue
      }
      continue // lone high surrogate: no low surrogate follows, drop it
    }
    if (code >= 0xdc00 && code <= 0xdfff) continue // lone low surrogate
    if (code === 0x09 || code === 0x0a || code === 0x0d) {
      out += text.charAt(i)
      continue
    }
    if (code < 0x20 || code === 0x7f) continue // other C0 controls, plus DEL
    out += text.charAt(i)
  }
  return out
}

/** Escapes the characters that are never legal literally in XML TEXT content, after stripping illegal characters. */
export function escapeXmlText(text: string): string {
  return sanitizeXmlText(text).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
}

/** As `escapeXmlText`, plus the two characters an XML ATTRIBUTE value additionally needs escaped. */
export function escapeXmlAttr(text: string): string {
  return escapeXmlText(text).replace(/"/g, '&quot;').replace(/'/g, '&apos;')
}

/**
 * CommonMark backslash escapes — the same rule `unescapeMarkdown` in
 * chat-media-markdown.tsx applies, duplicated rather than imported (see this
 * file's header comment).
 */
const BACKSLASH_ESCAPE_PATTERN = /\\([!"#$%&'()*+,\-./:;<=>?@[\\\]^_`{|}~])/g

function unescapeInline(text: string): string {
  return text.replace(BACKSLASH_ESCAPE_PATTERN, '$1')
}

/**
 * Same alternation, in the same order, as `parseInline`'s pattern in
 * chat-media-markdown.tsx: code span, bold, italic, inline math, markdown
 * link, citation marker, bare URL. Alternation is tried left-to-right at the
 * earliest matching position, so (as there) a URL already inside a code span
 * or a `[text](url)` link is consumed whole by that earlier branch and never
 * double-matched by the bare-URL one.
 */
const INLINE_PATTERN =
  /((?<!\\)`[^`]+`|(?<!\\)\*\*[^*]+\*\*|(?<!\\)\*[^*]+\*|(?<!\\)\$(?!\s|\$|\d)[^$\n]*?(?<!\\)(?<!\s)\$|(?<!\\)\[[^\]]+\]\([^)]+\)|(?<!\\)\[(?:\d+(?:\s*,\s*\d+)*)\]|https?:\/\/[^\s<>"'\])]+)/g

const CITATION_MARKER_PATTERN = /^\[\d+(?:\s*,\s*\d+)*\]$/
const MARKDOWN_LINK_PATTERN = /^\[([^\]]+)\]\(([^)]+)\)$/
const BARE_URL_PATTERN = /^https?:\/\//

/**
 * Schemes an exported document is allowed to turn into a real, clickable
 * hyperlink. A bare URL (`BARE_URL_PATTERN`, above) already only ever matches
 * `http(s)://`, but a `[text](url)` markdown link's `url` half is whatever
 * the model wrote — `javascript:`, `data:`, `file:` and friends all parse as
 * valid link targets there. Word (and, via `docx.ts`, its `TargetMode`
 * `"External"` relationships) would happily launch any of those on a click,
 * so a link whose scheme isn't in this allowlist is kept as plain, inert
 * text instead of becoming a run with an `href` — the same bar the export
 * spec sets for every hyperlink it writes.
 */
const SAFE_HREF_PATTERN = /^(?:https?:|mailto:)/i

/** Whether `url` is safe to become a real hyperlink target in an exported document — see `SAFE_HREF_PATTERN`. */
export function isSafeExportHref(url: string): boolean {
  return SAFE_HREF_PATTERN.test(url.trim())
}

/**
 * Splits trailing sentence punctuation off a bare URL, same as `parseInline`
 * does before autolinking one — "... se https://example.com." must not pull
 * the period into the href.
 */
function splitUrlTrailingPunctuation(url: string): { url: string; trailing: string } {
  const trailing = /[.,;:!?]+$/.exec(url)
  return trailing ? { url: url.slice(0, trailing.index), trailing: trailing[0] } : { url, trailing: '' }
}

function pushPlainRun(runs: InlineRun[], text: string): void {
  if (text) runs.push({ text })
}

/**
 * Tokenizes one line (or cell, or list item) of inline markdown into runs a
 * document format can render directly. Two token kinds are deliberately kept
 * as literal, unstyled text rather than becoming a run of their own:
 *
 *  - `[n]` / `[n, m]` citation markers: an exported document has no source
 *    panel for a chip to open into, so the marker is left exactly as typed —
 *    digits in brackets — rather than silently dropped or turned into a
 *    number no one can trace back to anything.
 *  - `$...$` inline math: rendering it would mean bundling a LaTeX layout
 *    engine into three from-scratch OOXML writers for a rare case. The raw
 *    TeX source (delimiters included) is kept as-is instead — the same
 *    fallback the chat UI itself uses for a malformed or still-streaming
 *    expression (see `renderInlineMath` in chat-media-markdown.tsx).
 */
export function tokenizeInline(text: string): InlineRun[] {
  const runs: InlineRun[] = []
  let cursor = 0
  for (const match of text.matchAll(INLINE_PATTERN)) {
    if (match.index == null) continue
    const token = match[0]
    if (match.index > cursor) pushPlainRun(runs, unescapeInline(text.slice(cursor, match.index)))
    if (token.startsWith('`')) {
      runs.push({ text: token.slice(1, -1), code: true })
    } else if (token.startsWith('**')) {
      runs.push({ text: unescapeInline(token.slice(2, -2)), bold: true })
    } else if (token.startsWith('*')) {
      runs.push({ text: unescapeInline(token.slice(1, -1)), italic: true })
    } else if (token.startsWith('$')) {
      pushPlainRun(runs, token)
    } else if (CITATION_MARKER_PATTERN.test(token)) {
      pushPlainRun(runs, token)
    } else if (BARE_URL_PATTERN.test(token)) {
      const { url, trailing } = splitUrlTrailingPunctuation(token)
      runs.push({ text: url, href: url })
      pushPlainRun(runs, trailing)
    } else {
      const link = MARKDOWN_LINK_PATTERN.exec(token)
      if (link) {
        const linkText = unescapeInline(link[1] ?? '')
        const linkUrl = link[2] ?? ''
        // An unsafe scheme keeps the link's VISIBLE text but drops the href
        // entirely — never falls back to the raw `[text](url)` source, which
        // would just show the dangerous URL as literal text instead of
        // hiding it.
        if (isSafeExportHref(linkUrl)) runs.push({ text: linkText, href: linkUrl })
        else pushPlainRun(runs, linkText)
      } else pushPlainRun(runs, token)
    }
    cursor = match.index + token.length
  }
  if (cursor < text.length) pushPlainRun(runs, unescapeInline(text.slice(cursor)))
  return runs
}

/** The plain-text content of a run list, markdown syntax discarded. */
export function plainTextFromRuns(runs: readonly InlineRun[]): string {
  return runs.map((run) => run.text).join('')
}

/** Convenience for a caller that only ever needs plain text, e.g. an xlsx cell. */
export function stripInlineMarkdown(text: string): string {
  return plainTextFromRuns(tokenizeInline(text))
}

/**
 * The plain text of the heading an answer opens with, or '' when it opens
 * with anything else. Such an answer already names itself, so `index.ts`
 * takes this as the document title and `docx.ts` then leaves that first
 * heading out of the body instead of printing the same line twice under
 * the Title.
 */
export function leadHeadingText(blocks: readonly MarkdownBlock[]): string {
  const first = blocks[0]
  return first?.kind === 'heading' ? stripInlineMarkdown(first.text).replace(/\s+/g, ' ').trim() : ''
}

export const XML_DECLARATION = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n'

const utf8Encoder = new TextEncoder()

/** UTF-8 encodes an XML (or relationships) document into the bytes a zip entry needs. */
export function encodeUtf8(text: string): Uint8Array {
  return utf8Encoder.encode(text)
}

/** A human date for a title slide, a document footer, or `docProps/core.xml` — in the export's own locale. Falls back to "now" when `createdAt` is missing or unparseable. */
export function formatExportDate(createdAt: string | undefined, locale: ExportLocale): string {
  const parsed = createdAt ? new Date(createdAt) : new Date()
  const date = Number.isNaN(parsed.getTime()) ? new Date() : parsed
  return new Intl.DateTimeFormat(locale === 'en' ? 'en-US' : 'nb-NO', {
    year: 'numeric',
    month: 'long',
    day: 'numeric',
  }).format(date)
}

/** W3CDTF timestamp (`docProps/core.xml`'s `dcterms:created`/`modified`), falling back to "now" the same way `formatExportDate` does. */
export function w3cdtf(createdAt: string | undefined): string {
  const parsed = createdAt ? new Date(createdAt) : new Date()
  const date = Number.isNaN(parsed.getTime()) ? new Date() : parsed
  return date.toISOString().replace(/\.\d+Z$/, 'Z')
}

/**
 * `docProps/core.xml`, identical in shape across all three formats — only
 * `[Content_Types].xml`'s part-type declarations differ between them.
 */
export function buildCorePropsXml(title: string, createdAt: string | undefined): string {
  const timestamp = w3cdtf(createdAt)
  return (
    XML_DECLARATION +
    '<cp:coreProperties' +
    ' xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties"' +
    ' xmlns:dc="http://purl.org/dc/elements/1.1/"' +
    ' xmlns:dcterms="http://purl.org/dc/terms/"' +
    ' xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">' +
    `<dc:title>${escapeXmlText(title)}</dc:title>` +
    '<dc:creator>Verevon</dc:creator>' +
    '<cp:lastModifiedBy>Verevon</cp:lastModifiedBy>' +
    `<dcterms:created xsi:type="dcterms:W3CDTF">${timestamp}</dcterms:created>` +
    `<dcterms:modified xsi:type="dcterms:W3CDTF">${timestamp}</dcterms:modified>` +
    '</cp:coreProperties>'
  )
}

/** `docProps/app.xml`, minimal but valid — no `TitlesOfParts` list, which every one of these three consumers tolerates being absent. */
export function buildAppPropsXml(): string {
  return (
    XML_DECLARATION +
    '<Properties' +
    ' xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties"' +
    ' xmlns:vt="http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes">' +
    '<Application>Verevon</Application>' +
    '</Properties>'
  )
}

/** Package-level `_rels/.rels`, identical in shape for docx/xlsx/pptx — only the middle relationship's type name differs, which is why it's a parameter. */
export function buildPackageRelsXml(officeDocumentTarget: string): string {
  return (
    XML_DECLARATION +
    '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">' +
    `<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="${officeDocumentTarget}"/>` +
    '<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/>' +
    '<Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties" Target="docProps/app.xml"/>' +
    '</Relationships>'
  )
}
