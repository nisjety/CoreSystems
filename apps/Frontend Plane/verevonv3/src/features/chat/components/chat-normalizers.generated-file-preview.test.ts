import { describe, expect, it } from 'vitest'
import { normalizeGeneratedFile, normalizeGeneratedFilePreview } from './chat-normalizers'

// The gateway may attach a rendered PDF copy of a generated Office file to the
// `attachment` frame (WS19-S5). The SPA keeps it only as a PDF `data:` URI with
// its page facts, and only PNG `data:` thumbnails, so the viewer never loads a URL.

const PREVIEW = {
  mime: 'application/pdf',
  url: 'data:application/pdf;base64,JVBERi0=',
  pages: 3,
  fonts_substituted: true,
  thumbs: ['data:image/png;base64,iVBO', 'https://example.test/x.png'],
}

describe('normalizeGeneratedFilePreview', () => {
  it('keeps a well-formed preview and drops non-PNG data thumbnails', () => {
    expect(normalizeGeneratedFilePreview(PREVIEW)).toEqual({
      mime: 'application/pdf',
      url: 'data:application/pdf;base64,JVBERi0=',
      pages: 3,
      fontsSubstituted: true,
      thumbs: ['data:image/png;base64,iVBO'],
    })
  })

  it('refuses a preview that is not a PDF data URI or lacks its page facts', () => {
    expect(normalizeGeneratedFilePreview({ ...PREVIEW, url: 'https://example.test/x.pdf' })).toBeUndefined()
    expect(normalizeGeneratedFilePreview({ ...PREVIEW, mime: 'text/html' })).toBeUndefined()
    expect(normalizeGeneratedFilePreview({ ...PREVIEW, pages: '3' })).toBeUndefined()
    expect(normalizeGeneratedFilePreview({ ...PREVIEW, fonts_substituted: undefined })).toBeUndefined()
    expect(normalizeGeneratedFilePreview(null)).toBeUndefined()
  })

  it('rides on the generated file, and a file without one has no preview key', () => {
    const base = { id: 'f1', name: 'r.docx', mime: 'application/x', size: 2, url: 'data:application/x;base64,AA' }
    expect(normalizeGeneratedFile({ ...base, preview: PREVIEW })?.preview?.pages).toBe(3)
    expect(normalizeGeneratedFile(base)).not.toHaveProperty('preview')
  })
})
