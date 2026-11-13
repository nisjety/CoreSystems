// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from 'vitest'
import { prepareChatAttachments } from './prepare-chat-attachments'
import { extractChatDocument, getAttachmentPolicy } from '@/shared/api/chat-client'
import { LocalizedError, translateApiError } from '@/shared/i18n/errors'

vi.mock('@/shared/api/chat-client', () => ({ extractChatDocument: vi.fn(), getAttachmentPolicy: vi.fn(() => undefined) }))
const file = (name: string, text: string) => ({ id: name, name, size: text.length, type: 'text/plain', url: `data:text/plain,${encodeURIComponent(text)}` })
afterEach(() => { vi.unstubAllGlobals(); vi.clearAllMocks() })
describe('complete attachment preparation before submission', () => {
  it('reads the per-file limit from the declared policy and names it in the refusal', async () => {
    const big = { ...file('big.csv', 'content'), size: 2_000_000 }
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: true, text: async () => 'content' }))
    await expect(prepareChatAttachments([big])).rejects.toThrow('under 1 MB')
    vi.mocked(getAttachmentPolicy).mockReturnValueOnce({ maxFileBytes: 4_000_000 } as ReturnType<typeof getAttachmentPolicy>)
    expect((await prepareChatAttachments([big]))[0]?.extractedText).toBe('content')
    vi.mocked(getAttachmentPolicy).mockReturnValueOnce({ maxFileBytes: 1_500_000 } as ReturnType<typeof getAttachmentPolicy>)
    // The Norwegian sentence keeps the Norwegian decimal comma; the English `message` writes 1.5.
    const limited: unknown = await prepareChatAttachments([big]).then(() => undefined, (error: unknown) => error)
    expect(limited).toBeInstanceOf(LocalizedError)
    expect((limited as LocalizedError).copy.no).toContain('under 1,5 MB')
    expect((limited as LocalizedError).message).toContain('under 1.5 MB')
  })
  it('preserves both same-named files in order', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValueOnce({ ok: true, text: async () => 'first' }).mockResolvedValueOnce({ ok: true, text: async () => 'second' }))
    expect((await prepareChatAttachments([file('notes.md', 'first'), file('notes.md', 'second')])).map(a => a.extractedText)).toEqual(['first', 'second'])
  })
  it('rejects a partial batch when the second file cannot be read', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValueOnce({ ok: true, text: async () => 'first' }).mockResolvedValueOnce({ ok: false }))
    await expect(prepareChatAttachments([file('first.md', 'first'), file('missing.csv', '')])).rejects.toThrow('missing.csv')
  })
  it('refuses an unreadable file as a LocalizedError that names the file in both languages', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: false }))
    const caught: unknown = await prepareChatAttachments([file('missing.md', 'first')]).then(() => undefined, (error: unknown) => error)
    expect(caught).toBeInstanceOf(LocalizedError)
    const refusal = caught as LocalizedError
    expect(refusal.copy.no).toContain('Kunne ikke lese hele «missing.md». Utkastet er beholdt.')
    expect(refusal.copy.en).toContain('Could not read all of “missing.md”. Your draft has been kept.')
    expect(refusal.copy.en).not.toMatch(/Kunne ikke|Utkastet|Prøv igjen/)
    // What a send site shows: the refusal's own words in the reader's language, never its generic fallback.
    const fallback = { no: 'Kunne ikke sende. Prøv igjen.', en: 'Could not send. Please retry.' }
    expect(translateApiError(refusal, (noText) => noText, fallback)).toBe(refusal.copy.no)
    expect(translateApiError(refusal, (_noText, enText) => enText, fallback)).toBe(refusal.copy.en)
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
