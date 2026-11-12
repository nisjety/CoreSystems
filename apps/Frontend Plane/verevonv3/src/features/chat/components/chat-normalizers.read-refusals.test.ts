import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { type AttachmentPolicy, getAttachmentPolicy } from '@/shared/api/chat-client'
import { LocalizedError, translateApiError } from '@/shared/i18n/errors'

import { inlineTextAttachments, toStreamAttachments } from './chat-normalizers'

// A file, image or document the turn cannot read whole is refused with a LocalizedError. The send
// sites that run translateApiError (the Home composer and the Space room) then show the sentence
// that says what to do, in the reader's language. A plain Error would show the caller's generic
// fallback instead, and its Norwegian text to an English reader.

vi.mock('@/shared/api/chat-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/chat-client')>()),
  getAttachmentPolicy: vi.fn(() => undefined),
}))

const norwegian = (noText: string) => noText
const english = (_noText: string, enText: string) => enText
const FALLBACK = { no: 'Kunne ikke sende. Prøv igjen.', en: 'Could not send. Please retry.' }

const notes = { id: 'n1', name: 'notes.md', size: 12, type: 'text/markdown', url: 'blob:notes' }
const chart = { id: 'i1', name: 'chart.png', size: 4, type: 'image/png', url: 'blob:chart' }
const sales = { id: 'a1', name: 'sales.csv', size: 8, type: 'text/csv', url: '' }
// "hi" as base64: two bytes once decoded, so the declared total below can be passed on the second image.
const pixel = (name: string) => ({ id: name, name, size: 2, type: 'image/png', url: 'data:image/png;base64,aGk=' })

const policy = (overrides: Partial<AttachmentPolicy> = {}): AttachmentPolicy => ({
  imageMimes: ['image/png'],
  maxImages: 4,
  maxImageBytes: 1_000_000,
  maxFiles: 3,
  maxFileBytes: 1_000_000,
  maxTotalBytes: 3_145_728,
  modes: { vision: true, sandboxFiles: false },
  ...overrides,
})

/** The rejection a caller receives. Fails the test when the call does not refuse. */
async function refusalOf(run: () => Promise<unknown>): Promise<LocalizedError> {
  const caught: unknown = await run().then(
    () => {
      throw new Error('expected a refusal')
    },
    (error: unknown) => error,
  )
  expect(caught).toBeInstanceOf(LocalizedError)
  return caught as LocalizedError
}

beforeEach(() => {
  vi.mocked(getAttachmentPolicy).mockReset()
})

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('a document the turn cannot read whole', () => {
  it('is refused with its own sentence in both languages, not the generic fallback', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: false }))
    const error = await refusalOf(() => inlineTextAttachments([notes]))
    expect(error.copy).toEqual({
      no: 'Kunne ikke lese hele «notes.md». Utkastet er beholdt; prøv igjen med et lesbart, mindre vedlegg.',
      en: 'Could not read all of “notes.md”. Your draft has been kept; try again with a readable, smaller attachment.',
    })
    expect(translateApiError(error, norwegian, FALLBACK)).toBe(error.copy.no)
    expect(translateApiError(error, english, FALLBACK)).toBe(error.copy.en)
  })

  it('refuses a document over the per-file ceiling with the same sentence', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: true, text: async () => 'x'.repeat(60_001) }))
    const error = await refusalOf(() => inlineTextAttachments([notes]))
    expect(error.copy.no).toContain('«notes.md»')
    expect(error.copy.en).toContain('“notes.md”')
  })
})

describe('an image or a file the turn cannot read', () => {
  it('names the image in both languages when its bytes cannot be fetched', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: false }))
    const error = await refusalOf(() => toStreamAttachments([chart]))
    expect(error.copy).toEqual({
      no: 'Kunne ikke lese bildet «chart.png». Prøv igjen eller fjern bildet.',
      en: 'Could not read the image “chart.png”. Try again or remove the image.',
    })
    expect(translateApiError(error, english, FALLBACK)).toBe(error.copy.en)
  })

  it('names a document the server stages when it has no bytes to send', async () => {
    vi.mocked(getAttachmentPolicy).mockReturnValue(policy({ modes: { vision: true, sandboxFiles: true } }))
    const error = await refusalOf(() => toStreamAttachments([sales]))
    expect(error.copy).toEqual({
      no: 'Kunne ikke lese «sales.csv». Prøv igjen eller fjern filen.',
      en: 'Could not read “sales.csv”. Try again or remove the file.',
    })
    expect(translateApiError(error, norwegian, FALLBACK)).toBe(error.copy.no)
  })

  it('refuses the turn when its images pass the declared total, naming the file that went over', async () => {
    vi.mocked(getAttachmentPolicy).mockReturnValue(policy({ maxTotalBytes: 3 }))
    const error = await refusalOf(() => toStreamAttachments([pixel('first.png'), pixel('second.png')]))
    expect(error.copy).toEqual({
      no: 'Vedleggene er for store til å sendes i én melding («second.png» passerte grensen). Fjern noen og prøv igjen.',
      en: 'The attachments are too large to send in one message (“second.png” went over the limit). Remove some and try again.',
    })
    expect(translateApiError(error, english, FALLBACK)).toBe(error.copy.en)
  })

  it('still sends a readable image unchanged', async () => {
    await expect(toStreamAttachments([pixel('first.png')])).resolves.toEqual([
      { kind: 'image', data_base64: 'aGk=', mime_type: 'image/png' },
    ])
  })
})
