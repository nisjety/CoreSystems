import { describe, expect, it } from 'vitest'
import { ApiError } from '@/shared/api/http'
import { documentErrorMessage, documentSizeLabel, documentWarningMessage } from '@/features/ingestions/lib/documents'

const tr = (no: string) => no
const fallback = { no: 'Dokumentet kunne ikke leses.', en: 'The document could not be read.' }

describe('document read copy', () => {
  it('says what each warning means for the text, and shows an unknown one as Quarry sent it', () => {
    expect(documentWarningMessage({ code: 'document_has_no_text_layer', message: 'x' }, tr)).toContain('skanning')
    expect(documentWarningMessage({ code: 'truncated_at_8mib', message: 'x' }, tr)).toContain('8 MiB')
    expect(documentWarningMessage({ code: 'a_new_code', message: 'Quarry said so' }, tr)).toBe('Quarry said so')
  })

  it('names the switch when Quarry has document reading off', () => {
    const off = new ApiError('off', 501, 'UNSUPPORTED', {
      details: { reason: 'parse_route_disabled', env: 'QUARRY_EDGE__PARSE_ROUTE' },
    })
    expect(documentErrorMessage(off, tr, fallback)).toContain('QUARRY_EDGE__PARSE_ROUTE')
  })

  it('tells isolation apart from a switch that is off', () => {
    const isolation = new ApiError('not isolated', 501, 'UNSUPPORTED', { details: { reason: 'isolation_required' } })
    const copy = documentErrorMessage(isolation, tr, fallback)
    expect(copy).toContain('isolert')
    expect(copy).not.toContain('QUARRY_EDGE__PARSE_ROUTE')
  })

  it('gives each refusal about the file its own sentence', () => {
    const codes = [
      'too_large',
      'empty_body',
      'body_unreadable',
      'unsupported_media',
      'content_type_mismatch',
      'pdf_parse_failed',
      'docx_parse_failed',
    ]
    const sentences = codes.map((code) => documentErrorMessage(new ApiError('x', 415, code), tr, fallback))
    expect(new Set(sentences).size).toBe(codes.length)
  })

  it('gives a busy answer the wait Quarry asked for', () => {
    const busy = new ApiError('busy', 429, 'RATE_LIMITED', { retryAfterSeconds: 7 })
    expect(documentErrorMessage(busy, tr, fallback)).toContain('7 sekunder')
  })

  it('formats a file size for a person', () => {
    expect(documentSizeLabel(512)).toBe('512 B')
    expect(documentSizeLabel(1536)).toBe('1.5 KB')
    expect(documentSizeLabel(25 * 1024 * 1024)).toBe('25.0 MB')
  })
})
