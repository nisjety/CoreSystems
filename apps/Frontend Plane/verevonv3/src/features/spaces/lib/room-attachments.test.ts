import { afterEach, describe, expect, it, vi } from 'vitest'

import { zdrRefusalPresentation } from '@/shared/api/zdr-refusals'
import type { RoomAttachment } from './room-attachments'

// No declared attachment policy: the constants in attachment-policy.ts are the limits under test.
const chatClient = vi.hoisted(() => ({ getAttachmentPolicy: vi.fn(() => undefined) }))
vi.mock('@/shared/api/chat-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/chat-client')>()),
  ...chatClient,
}))

const {
  pasteBecomesAttachment,
  pastedTextFile,
  prepareRoomTurn,
  ROOM_PASTE_ATTACHMENT_THRESHOLD,
  splitRoomMessage,
  toRoomAttachment,
  vetRoomAttachments,
} = await import('./room-attachments')

const ask = { subscriptionBacked: false, planMode: false, zdr: false, currentImageCount: 0, currentTotalBytes: 0 }
/** The reader's language for these tests is Norwegian, the product default. */
const nb = (noText: string) => noText

const PNG_BASE64 = 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg=='
const PNG_URL = `data:image/png;base64,${PNG_BASE64}`

const chip = (name: string, type: string, url: string, size = 12): RoomAttachment => ({
  id: `chip-${name}`,
  name,
  size,
  type,
  url,
})

/** Serves each document body by its URL. Any other request reads as an empty 200. */
function serveBodies(bodies: Record<string, string>) {
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string) => ({ ok: true, text: async () => bodies[url] ?? '' })),
  )
}

afterEach(() => {
  vi.unstubAllGlobals()
  vi.clearAllMocks()
})

describe('vetRoomAttachments (WS14-S8)', () => {
  it('caps a turn at ten files, counting the files already on it, and says how many were left out', () => {
    const files = Array.from(
      { length: 12 },
      (_, index) => new File([`notat ${index + 1}`], `notat-${index + 1}.txt`, { type: 'text/plain' }),
    )
    const fresh = vetRoomAttachments(files, ask, 0, nb)
    expect(fresh.accepted.map((file) => file.name)).toEqual(files.slice(0, 10).map((file) => file.name))
    expect(fresh.messages).toEqual(['Maks. 10 vedlegg per melding. 2 ble ikke lagt ved.'])

    const packed = vetRoomAttachments(files, ask, 8, nb)
    expect(packed.accepted).toHaveLength(2)
    expect(packed.messages).toEqual(['Maks. 10 vedlegg per melding. 10 ble ikke lagt ved.'])
  })

  it('refuses an image under zero retention with the typed zdr_feature_not_supported copy and never accepts it', () => {
    const image = new File(['png'], 'skjerm.png', { type: 'image/png' })
    const notat = new File(['Notat'], 'notat.md', { type: 'text/markdown' })
    const verdict = vetRoomAttachments([image, notat], { ...ask, zdr: true }, 0, nb)
    // A text document is not an image: it still goes into the message under zero retention.
    expect(verdict.accepted).toEqual([notat])
    const typed = zdrRefusalPresentation('zdr_feature_not_supported', 'attachments', nb)
    expect(verdict.messages).toEqual([[typed.message, typed.next].filter(Boolean).join(' ')])
    expect(verdict.messages[0]).toContain('Zero Data Retention')
    expect(verdict.accepted).not.toContain(image)
    expect(window.localStorage.length).toBe(0)
  })
})

describe('room paste and chips (WS14-S8)', () => {
  it('turns a paste over 4000 characters into a .txt chip and leaves a 4000-character paste in the box', () => {
    expect(ROOM_PASTE_ATTACHMENT_THRESHOLD).toBe(4000)
    expect(pasteBecomesAttachment('x'.repeat(4000))).toBe(false)

    const pasted = `Kari skrev dette: ${'lang tekst '.repeat(400)}`
    expect(pasted.length).toBeGreaterThan(4000)
    expect(pasteBecomesAttachment(pasted)).toBe(true)

    const file = pastedTextFile(pasted, 'Innlimt tekst')
    expect(file.type).toBe('text/plain')
    expect(file.name.startsWith('Innlimt tekst: Kari skrev dette: lang tekst')).toBe(true)
    expect(file.name.endsWith('….txt')).toBe(true)
    expect(toRoomAttachment(file, 0)).toMatchObject({ name: file.name, size: pasted.length, type: 'text/plain' })
  })
})

describe('prepareRoomTurn and splitRoomMessage (WS14-S8)', () => {
  it('appends the VEDLEGG fence to the words and returns the image as a stream attachment', async () => {
    serveBodies({ 'https://room.test/notat.md': '# Notat\nFørste linje.' })
    const doc = chip('notat.md', 'text/markdown', 'https://room.test/notat.md', 24)
    const image = chip('skjerm.png', 'image/png', PNG_URL, 70)

    const turn = await prepareRoomTurn('Hva står her?', [doc, image])

    expect(turn.content).toBe(
      'Hva står her?\n\n--- VEDLEGG: notat.md ---\n# Notat\nFørste linje.\n--- SLUTT PÅ VEDLEGG: notat.md ---',
    )
    expect(turn.attachments).toEqual([{ kind: 'image', data_base64: PNG_BASE64, mime_type: 'image/png' }])
  })

  it('splitRoomMessage returns the words and each file as prepareRoomTurn wrote them', async () => {
    serveBodies({ 'https://room.test/a.txt': 'Første\r\nlinje', 'https://room.test/b.md': '## B' })
    const turn = await prepareRoomTurn('Sammenlign disse', [
      chip('a.txt', 'text/plain', 'https://room.test/a.txt'),
      chip('b.md', 'text/markdown', 'https://room.test/b.md'),
    ])

    expect(splitRoomMessage(turn.content)).toEqual({
      text: 'Sammenlign disse',
      files: [
        { name: 'a.txt', content: 'Første\nlinje' },
        { name: 'b.md', content: '## B' },
      ],
    })
    expect(splitRoomMessage('Bare tekst')).toEqual({ text: 'Bare tekst', files: [] })
  })
})
