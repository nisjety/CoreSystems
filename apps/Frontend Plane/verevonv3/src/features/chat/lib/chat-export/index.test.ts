import { afterEach, describe, expect, it, vi } from 'vitest'

vi.mock('./download', () => ({ downloadBytes: vi.fn() }))

import { downloadBytes } from './download'
import { answerExportSize, exportChatAnswer } from './index'

const downloadBytesMock = vi.mocked(downloadBytes)

afterEach(() => {
  downloadBytesMock.mockClear()
})

describe('answerExportSize', () => {
  it('counts a plain paragraph verbatim', () => {
    expect(answerExportSize('Hei verden')).toBe('Hei verden'.length)
  })

  it('strips markdown syntax before counting', () => {
    // "**Total**" -> "Total" (5), not the 9 raw characters including asterisks.
    expect(answerExportSize('**Total**')).toBe(5)
  })

  it('is 0 for an empty answer', () => {
    expect(answerExportSize('')).toBe(0)
  })

  it('grows past 600 for a long answer, matching the visible export button threshold', () => {
    const long = Array.from({ length: 20 }, (_, i) => `Dette er avsnitt ${i} med litt tekst i seg selv.`).join('\n\n')
    expect(answerExportSize(long)).toBeGreaterThan(600)
  })
})

describe('exportChatAnswer', () => {
  it('builds and downloads a docx with a filename derived from the title and date', () => {
    exportChatAnswer('docx', {
      content: '# Rapport\n\nInnhold her.',
      title: 'Kvartalsrapport Q3',
      createdAt: '2026-09-28T00:00:00Z',
    })
    expect(downloadBytesMock).toHaveBeenCalledTimes(1)
    const [bytes, fileName, mime] = downloadBytesMock.mock.calls[0] as [Uint8Array, string, string]
    expect(fileName).toBe('kvartalsrapport-q3-2026-09-28.docx')
    expect(mime).toBe('application/vnd.openxmlformats-officedocument.wordprocessingml.document')
    expect(bytes.byteLength).toBeGreaterThan(0)
  })

  it('builds an xlsx and a pptx too, with matching MIME types', () => {
    exportChatAnswer('xlsx', { content: 'Tekst', title: 'X' })
    exportChatAnswer('pptx', { content: 'Tekst', title: 'X' })
    const [, , xlsxMime] = downloadBytesMock.mock.calls[0] as [Uint8Array, string, string]
    const [, , pptxMime] = downloadBytesMock.mock.calls[1] as [Uint8Array, string, string]
    expect(xlsxMime).toBe('application/vnd.openxmlformats-officedocument.spreadsheetml.sheet')
    expect(pptxMime).toBe('application/vnd.openxmlformats-officedocument.presentationml.presentation')
  })

  it('throws for a pdf export with no element, rather than silently doing nothing', () => {
    expect(() => exportChatAnswer('pdf', { content: 'x', title: 'Test' })).toThrow()
    expect(downloadBytesMock).not.toHaveBeenCalled()
  })

  it('names the file after the heading the answer opens with when no title is given', () => {
    exportChatAnswer('docx', { content: '## Kvartalsrapport for **Q3**\n\nDette er et sammendrag.', createdAt: '2026-09-28T00:00:00Z' })
    const [, fileName] = downloadBytesMock.mock.calls[0] as [Uint8Array, string, string]
    expect(fileName).toBe('kvartalsrapport-for-q3-2026-09-28.docx')
  })

  it('uses a markdown-free preview of the first paragraph when the answer has no heading', () => {
    exportChatAnswer('docx', { content: 'Her er **sammendraget** av kvartalet.\n\n- Punkt', createdAt: '2026-09-28T00:00:00Z' })
    const [, fileName] = downloadBytesMock.mock.calls[0] as [Uint8Array, string, string]
    expect(fileName).toBe('her-er-sammendraget-av-kvartalet-2026-09-28.docx')
  })

  it('defaults the title to "Svar" (Norwegian) when the answer has no prose to name it by', () => {
    exportChatAnswer('xlsx', { content: '| a | b |\n|---|---|\n| 1 | 2 |' })
    const [, fileName] = downloadBytesMock.mock.calls[0] as [Uint8Array, string, string]
    expect(fileName.startsWith('svar-')).toBe(true)
  })

  it('defaults the title to "Answer" in English locale', () => {
    exportChatAnswer('xlsx', { content: '| a | b |\n|---|---|\n| 1 | 2 |', locale: 'en' })
    const [, fileName] = downloadBytesMock.mock.calls[0] as [Uint8Array, string, string]
    expect(fileName.startsWith('answer-')).toBe(true)
  })
})
