import { requestBytes } from './http'

/**
 * Document reading (Quarry INNH-11), through the gateway's `/api/ingestions/parse`. A PDF or
 * a DOCX goes up as its raw bytes and comes back as Markdown. Nothing is stored, and nothing is
 * redacted: personal data in the document comes back as it is, to the signed-in caller only.
 * The route is not metered, so an answer carries no cost.
 */

/** The largest document Quarry reads: the parsers' own limit, 25 MiB. */
export const MAX_DOCUMENT_BYTES = 25 * 1024 * 1024

export type DocumentKind = 'pdf' | 'docx'

/** The text is paragraphs only: headings, lists and tables are not marked up. */
export type DocumentStructure = 'plain'

export interface DocumentWarning {
  code: string
  message: string
}

/** One document read. `pages` is present for a PDF only, never null. */
export interface DocumentParse {
  markdown: string
  kind: DocumentKind
  bytes: number
  structure: DocumentStructure
  truncated: boolean
  pages?: number
  warnings: DocumentWarning[]
}

const CONTENT_TYPE_BY_KIND: Record<DocumentKind, string> = {
  pdf: 'application/pdf',
  docx: 'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
}

/**
 * The kind a file name claims, or `null` for anything else. Quarry decides the kind from the
 * bytes; this only chooses the `Content-Type` to send with them.
 */
export function documentKindOf(fileName: string): DocumentKind | null {
  const name = fileName.trim().toLowerCase()
  if (name.endsWith('.pdf')) return 'pdf'
  if (name.endsWith('.docx')) return 'docx'
  return null
}

/** Read one PDF or DOCX to Markdown. Rejects with an `ApiError` carrying Quarry's code and any `Retry-After`. */
export function parseDocument(file: Blob, kind: DocumentKind, signal?: AbortSignal): Promise<DocumentParse> {
  return requestBytes<DocumentParse>('/api/ingestions/parse', file, CONTENT_TYPE_BY_KIND[kind], { signal })
}
