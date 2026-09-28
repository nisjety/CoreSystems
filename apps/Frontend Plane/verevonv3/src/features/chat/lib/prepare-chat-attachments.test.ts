// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from 'vitest'
import { prepareChatAttachments } from './prepare-chat-attachments'
import { extractChatDocument } from '@/shared/api/chat-client'

vi.mock('@/shared/api/chat-client', () => ({ extractChatDocument: vi.fn() }))
const file = (name: string, text: string) => ({ id: name, name, size: text.length, type: 'text/plain', url: `data:text/plain,${encodeURIComponent(text)}` })
afterEach(() => { vi.unstubAllGlobals(); vi.clearAllMocks() })
describe('complete attachment preparation before submission', () => {
  it('preserves both same-named files in order', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValueOnce({ ok: true, text: async () => 'first' }).mockResolvedValueOnce({ ok: true, text: async () => 'second' }))
    expect((await prepareChatAttachments([file('notes.md', 'first'), file('notes.md', 'second')])).map(a => a.extractedText)).toEqual(['first', 'second'])
  })
  it('rejects a partial batch when the second file cannot be read', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValueOnce({ ok: true, text: async () => 'first' }).mockResolvedValueOnce({ ok: false }))
    await expect(prepareChatAttachments([file('first.md', 'first'), file('missing.csv', '')])).rejects.toThrow('missing.csv')
  })
  it('rejects over-budget text instead of indexing it after the answer starts', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: true, text: async () => 'x'.repeat(60_001) }))
    await expect(prepareChatAttachments([file('large.csv', 'content')])).rejects.toThrow('large.csv')
  })
  it.each(['pdf', 'docx', 'xlsx', 'pptx'])('waits for ephemeral %s extraction', async (extension) => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: true }))
    vi.mocked(extractChatDocument).mockResolvedValue({ content: 'Complete source text' })
    const input = { ...file(`source.${extension}`, ''), type: 'application/octet-stream', url: 'data:application/octet-stream;base64,SGVsbG8=' }
    expect((await prepareChatAttachments([input]))[0]?.extractedText).toBe('Complete source text')
    expect(extractChatDocument).toHaveBeenCalledWith(input.name, 'SGVsbG8=', input.type)
  })
  it('does not accept a PDF when extraction fails', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: true }))
    vi.mocked(extractChatDocument).mockRejectedValue(new Error('service unavailable'))
    await expect(prepareChatAttachments([{ ...file('source.pdf', ''), url: 'data:application/pdf;base64,SGVsbG8=' }])).rejects.toThrow('source.pdf')
  })
  it('reuses an already-extracted connector-picker attachment instead of re-extracting it', async () => {
    // A file picked from OneDrive/Google Drive already ran through
    // extractChatDocument at pick time (connector-picker spec §2.4); the
    // submit-time pass must carry that text forward, not spend a second
    // network round trip re-reading a file that never changed.
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: true }))
    const preExtracted = {
      ...file('Rapport.pdf', ''),
      type: 'application/pdf',
      url: 'data:application/pdf;base64,SGVsbG8=',
      extractedText: 'Already read at pick time',
    }
    const result = await prepareChatAttachments([preExtracted])
    expect(result[0]?.extractedText).toBe('Already read at pick time')
    expect(extractChatDocument).not.toHaveBeenCalled()
  })
  it('still extracts a PDF with no pre-populated text, same as a local upload', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: true }))
    vi.mocked(extractChatDocument).mockResolvedValue({ content: 'Freshly extracted text' })
    const notYetExtracted = { ...file('Rapport.pdf', ''), type: 'application/pdf', url: 'data:application/pdf;base64,SGVsbG8=' }
    const result = await prepareChatAttachments([notYetExtracted])
    expect(result[0]?.extractedText).toBe('Freshly extracted text')
    expect(extractChatDocument).toHaveBeenCalledWith(notYetExtracted.name, 'SGVsbG8=', notYetExtracted.type)
  })
})
