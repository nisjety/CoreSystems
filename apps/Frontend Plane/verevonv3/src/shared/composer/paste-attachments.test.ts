import { afterEach, describe, expect, it, vi } from 'vitest'

import {
  pastedImageFiles,
  pastedPlainText,
  pastedTextAttachmentFile,
  pastedTextAttachmentName,
} from './paste-attachments'

// The helpers only read `clipboardData`, so a plain object stands in for the DOM paste event.
function paste(clipboardData: unknown): ClipboardEvent {
  return { clipboardData } as unknown as ClipboardEvent
}

afterEach(() => {
  vi.useRealTimers()
})

describe('pastedImageFiles', () => {
  it('keeps only the image files of a paste', () => {
    const png = new File(['a'], 'skjerm.png', { type: 'image/png' })
    const pdf = new File(['b'], 'rapport.pdf', { type: 'application/pdf' })
    const jpg = new File(['c'], 'foto.jpg', { type: 'image/jpeg' })
    const txt = new File(['d'], 'notat.txt', { type: 'text/plain' })
    const kept = pastedImageFiles(paste({ files: [png, pdf, jpg, txt] }))
    expect(kept.map((file) => file.name)).toEqual(['skjerm.png', 'foto.jpg'])
  })

  it('is empty when the paste carries no files', () => {
    expect(pastedImageFiles(paste(null))).toEqual([])
    expect(pastedImageFiles(paste({ files: [] }))).toEqual([])
  })
})

describe('pastedPlainText', () => {
  it('reads the text/plain part of a paste, and is empty without data', () => {
    const getData = vi.fn((type: string) => (type === 'text/plain' ? 'hei verden' : ''))
    expect(pastedPlainText(paste({ getData }))).toBe('hei verden')
    expect(getData).toHaveBeenCalledWith('text/plain')
    expect(pastedPlainText(paste({ getData: () => '' }))).toBe('')
    expect(pastedPlainText(paste(null))).toBe('')
  })
})

describe('pastedTextAttachmentName', () => {
  it('uses the first 60 characters of the text, then an ellipsis, under the label', () => {
    expect(pastedTextAttachmentName('x'.repeat(100), 'Limt inn tekst')).toBe(`Limt inn tekst: ${'x'.repeat(60)}….txt`)
  })

  it('names a short text whole, with no ellipsis', () => {
    expect(pastedTextAttachmentName('kort tekst', 'Limt inn tekst')).toBe('Limt inn tekst: kort tekst.txt')
  })

  it('falls back to a timestamped name for blank text', () => {
    vi.useFakeTimers()
    vi.setSystemTime(new Date(2026, 10, 18, 9, 5, 7))
    expect(pastedTextAttachmentName('   \n  ', 'Limt inn tekst')).toBe(
      `Limt inn tekst ${new Date().toLocaleTimeString()}.txt`,
    )
  })
})

describe('pastedTextAttachmentFile', () => {
  it('wraps the text as a text/plain file named like the attachment', () => {
    const file = pastedTextAttachmentFile('hei', 'Limt inn tekst')
    expect(file.type).toBe('text/plain')
    expect(file.name).toBe('Limt inn tekst: hei.txt')
    expect(file.size).toBe(3)
  })
})
