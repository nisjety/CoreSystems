import type { ScrapePreviewResult, ScrapeResult } from '@/shared/api/knowledge-client'
import type { BrowserSessionResponse } from '@/shared/api/browser-client'

export type ScrapeBlock = { heading: boolean; raw: string; text: string }

export type ScrapePreview = {
  browserSession?: BrowserSessionResponse
  blocks: ScrapeBlock[]
  charCount: number
  description: string
  markdown: string
  source: ScrapePreviewResult['source'] | 'legacy'
  title: string
  url: string
}

type JsonRecord = Record<string, unknown>

const sourceValues = new Set(['artifact', 'empty', 'extract', 'inline'])

export function hostnameOf(value: string): string {
  try {
    return new URL(value).hostname.replace(/^www\./, '')
  } catch {
    return value
  }
}

export function normalizeUrl(input: string): string | null {
  const trimmed = input.trim()
  if (!trimmed) return null
  const withScheme = /^https?:\/\//i.test(trimmed) ? trimmed : `https://${trimmed}`
  try {
    const parsed = new URL(withScheme)
    if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') return null
    if (!parsed.hostname.includes('.')) return null
    return withScheme
  } catch {
    return null
  }
}

// quarry runs Mozilla-style readability then html2md, but on link-dense pages
// (news fronts, teaser cards) a single <a> wrapping a heading + rule lands as a
// markdown link whose label spans several blank-line-separated lines:
//   [
//   Se nå: I gang ----------
//   ](https://…)
// Splitting on blank lines then shatters one link into `[`, the text, and
// `](url)` — three junk blocks. So we collapse the whitespace inside every link
// first (it survives the split as one block) and strip rule runs (`----`/`====`).
const LINK_RE = /!?\[[^\]]*\]\([^)]*\)/g
const RULE_RUN_RE = /[-=_~]{3,}/g
// A block whose visible text is nothing but punctuation / brackets / rules is
// structural noise (a stray `[`, a `]`, a `----` divider), never real content.
const STRUCTURAL_ONLY_RE = /^[\s\-_=~*|·•#>[\]().,:;!?–—]+$/

function repairMultilineLinks(markdown: string): string {
  return markdown.replace(LINK_RE, (link) => link.replace(/\s+/g, ' '))
}

export function cleanBlock(raw: string): ScrapeBlock {
  const trimmed = raw.trim()
  const headingMatch = /^(#{1,6})\s+(.*)$/s.exec(trimmed)
  if (headingMatch) {
    const text = (headingMatch[2] ?? '').replace(RULE_RUN_RE, ' ').replace(/\s+/g, ' ').trim()
    return { heading: true, raw: trimmed, text }
  }
  const text = trimmed
    .replace(/^[-*+>]\s+/, '')
    .replace(/!?\[([^\]]*)\]\([^)]*\)/g, '$1')
    .replace(/<[^>]+>/g, ' ')
    .replace(RULE_RUN_RE, ' ')
    .replace(/[*_`]/g, '')
    .replace(/\s+/g, ' ')
    .trim()
  return { heading: false, raw: trimmed, text }
}

export function splitMarkdownBlocks(markdown: string): ScrapeBlock[] {
  const normalized = repairMultilineLinks(markdown.replace(/\r\n?/g, '\n')).trim()
  if (!normalized) return []

  const paragraphs = normalized.split(/\n{2,}/)
  const candidates = paragraphs.length > 1 ? paragraphs : normalized.split(/\n+/)

  return candidates
    .map(cleanBlock)
    .filter((block) => block.text.length > 0 && !STRUCTURAL_ONLY_RE.test(block.text))
    .map((block) => ({
      ...block,
      text: block.text.length > 400 ? `${block.text.slice(0, 397)}...` : block.text,
    }))
    .slice(0, 40)
}

export function toScrapePreview(url: string, result: ScrapeResult | ScrapePreviewResult): ScrapePreview {
  const root = result as unknown
  const markdown = markdownFromResult(root).trim()
  const resolvedUrl = firstNestedString(root, [
    ['url'],
    ['data', 'url', 'final_url'],
    ['data', 'url', 'final'],
    ['data', 'url', 'requested'],
    ['data', 'metadata', 'url'],
    ['data', 'metadata', 'sourceURL'],
    ['data', 'metadata', 'source_url'],
    ['metadata', 'url'],
    ['metadata', 'sourceURL'],
    ['metadata', 'source_url'],
  ]) ?? url
  const title = firstNestedString(root, [
    ['title'],
    ['data', 'title'],
    ['data', 'metadata', 'title'],
    ['metadata', 'title'],
  ])?.trim()
  const description = firstNestedString(root, [
    ['description'],
    ['data', 'description'],
    ['data', 'metadata', 'description'],
    ['metadata', 'description'],
  ])?.trim() ?? ''

  return {
    blocks: splitMarkdownBlocks(markdown),
    charCount: markdown.length,
    description,
    markdown,
    source: previewSource(firstNestedString(root, [['source']])),
    title: title || hostnameOf(resolvedUrl),
    url: resolvedUrl,
  }
}

function markdownFromResult(value: unknown): string {
  const inline = firstNestedString(value, [
    ['markdown'],
    ['data', 'markdown'],
    ['content'],
    ['data', 'content'],
    ['text'],
    ['data', 'text'],
  ])
  if (inline) return inline

  const html = firstNestedString(value, [
    ['html'],
    ['rawHtml'],
    ['data', 'html'],
    ['data', 'rawHtml'],
  ])
  return html ? htmlToReadableText(html) : ''
}

function htmlToReadableText(html: string): string {
  return html
    .replace(/<script\b[^>]*>[\s\S]*?<\/script>/gi, ' ')
    .replace(/<style\b[^>]*>[\s\S]*?<\/style>/gi, ' ')
    .replace(/<(h[1-6]|p|li|br|div|section|article|main|header|footer)\b[^>]*>/gi, '\n\n')
    .replace(/<\/(h[1-6]|p|li|div|section|article|main|header|footer)>/gi, '\n\n')
    .replace(/<[^>]+>/g, ' ')
    .replace(/&nbsp;/g, ' ')
    .replace(/&amp;/g, '&')
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .replace(/&quot;/g, '"')
    .replace(/&#39;/g, "'")
    .replace(/[ \t]+/g, ' ')
    .replace(/\n{3,}/g, '\n\n')
    .trim()
}

function previewSource(value: string | undefined): ScrapePreview['source'] {
  return value && sourceValues.has(value) ? value as ScrapePreview['source'] : 'legacy'
}

function isRecord(value: unknown): value is JsonRecord {
  return Boolean(value) && typeof value === 'object' && !Array.isArray(value)
}

function firstNestedString(value: unknown, paths: string[][]): string | undefined {
  for (const path of paths) {
    const match = nestedString(value, path)
    if (match) return match
  }
  return undefined
}

function nestedString(value: unknown, path: string[]): string | undefined {
  let current = value
  for (const segment of path) {
    if (!isRecord(current)) return undefined
    current = current[segment]
  }
  return typeof current === 'string' && current.trim() ? current : undefined
}
