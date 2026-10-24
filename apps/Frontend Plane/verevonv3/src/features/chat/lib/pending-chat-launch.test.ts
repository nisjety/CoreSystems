// @vitest-environment jsdom

import { beforeEach, afterEach, describe, expect, it, vi } from 'vitest'

vi.mock('@/shared/lib/blob-data', () => ({
  objectUrlToDataUrl: vi.fn((url: string) => Promise.resolve(`data:application/octet-stream;base64,converted-${url}`)),
}))

import { objectUrlToDataUrl } from '@/shared/lib/blob-data'
import { consumePendingChatLaunch, writePendingChatLaunch, MAX_PENDING_LAUNCH_CHARS } from './pending-chat-launch'

// The smallest IndexedDB the launch store uses: put, get and clear in one
// transaction, completing on the next task. `data` outlives a module reload.
function fakeIndexedDb(data: Map<string, unknown>, opts: { blocked?: boolean } = {}) {
  const open = vi.fn(() => {
    const request: { result?: unknown; error?: unknown; onsuccess?: () => void; onerror?: () => void; onupgradeneeded?: () => void } = {}
    const db = {
      close: vi.fn(),
      createObjectStore: () => undefined,
      transaction: () => {
        const tx: { oncomplete?: () => void } = {}
        setTimeout(() => tx.oncomplete?.(), 0)
        return {
          get oncomplete() { return tx.oncomplete },
          set oncomplete(handler) { tx.oncomplete = handler },
          objectStore: () => ({
            clear: () => { data.clear() },
            put: (value: unknown, key: string) => { data.set(key, value) },
            get: (key: string) => {
              const read: { result?: unknown; onsuccess?: () => void } = {}
              setTimeout(() => { read.result = data.get(key); read.onsuccess?.() }, 0)
              return read
            },
          }),
        }
      },
    }
    setTimeout(() => {
      if (opts.blocked) { request.error = new Error('blocked'); request.onerror?.(); return }
      request.result = db
      request.onupgradeneeded?.()
      request.onsuccess?.()
    }, 0)
    return request
  })
  return { open }
}

const settle = () => new Promise((resolve) => setTimeout(resolve, 5))
const bigFile = (id: string) => ({
  id, name: `${id}.xlsx`, size: 3_000_000, type: 'application/octet-stream',
  url: `data:application/octet-stream;base64,${'A'.repeat(4_000_000)}`,
})
let idbData: Map<string, unknown>

afterEach(() => { vi.restoreAllMocks(); vi.unstubAllGlobals(); consumePendingChatLaunch() })

beforeEach(() => {
  window.localStorage.clear()
  window.sessionStorage.clear()
  vi.mocked(objectUrlToDataUrl).mockClear()
  idbData = new Map()
  vi.stubGlobal('indexedDB', fakeIndexedDb(idbData))
})

describe('pending chat launch routing', () => {
  it('preserves a subscription route across dashboard-to-chat navigation', async () => {
    await writePendingChatLaunch({
      text: 'hello from the subscription',
      model: 'gpt-5.6-luna',
      provider: 'openai-codex-subscription',
      subscriptionConnectionId: ' conn_123 ',
      minPrivacyTier: 'global',
      effort: 'deep',
      tone: 'concise',
    })

    expect(consumePendingChatLaunch()).toMatchObject({
      text: 'hello from the subscription',
      model: 'gpt-5.6-luna',
      provider: 'openai-codex-subscription',
      subscriptionConnectionId: 'conn_123',
      minPrivacyTier: 'global',
      effort: 'deep',
      tone: 'concise',
    })
  })

  it('carries a canonical effort level through the launch and drops an unknown one', async () => {
    await writePendingChatLaunch({ text: 'think hard', effort: 'xhigh' })
    expect(consumePendingChatLaunch()).toMatchObject({ effort: 'xhigh' })

    window.sessionStorage.setItem(
      'verevon.chat.pendingLaunch',
      JSON.stringify({ text: 'stale client', effort: 'ultra' }),
    )
    expect(consumePendingChatLaunch()).toBeNull()
  })

  // DashboardComposer revokes every attachment's blob: URL right after this
  // write resolves (resetComposerDraft -> clearFiles, called synchronously
  // before the navigate to /chat). A non-image attachment (.txt/.pdf/.docx/
  // .csv/.json/.html upload, or a paste-to-text-attachment) must be converted
  // to a data: URL exactly like an image, or the chat page reads back a blob:
  // URL whose underlying blob is already gone.
  it('converts a non-image attachment blob: URL to a data: URL, not just images', async () => {
    await writePendingChatLaunch({
      text: 'see attached',
      attachments: [
        {
          id: 'a1',
          name: 'notes.txt',
          size: 12,
          type: 'text/plain',
          url: 'blob:http://localhost/text-attachment',
        },
      ],
    })

    expect(objectUrlToDataUrl).toHaveBeenCalledWith('blob:http://localhost/text-attachment')
    const launch = consumePendingChatLaunch()
    expect(launch?.attachments?.[0]?.url).toBe(
      'data:application/octet-stream;base64,converted-blob:http://localhost/text-attachment',
    )
  })

  it('rejects an unreadable attachment without accepting an incomplete launch', async () => {
    vi.mocked(objectUrlToDataUrl).mockRejectedValueOnce(new Error('blob already revoked'))

    await expect(writePendingChatLaunch({
      text: 'see attached',
      attachments: [
        {
          id: 'a1',
          name: 'notes.txt',
          size: 12,
          type: 'text/plain',
          url: 'blob:http://localhost/text-attachment',
        },
      ],
    })).rejects.toMatchObject({ reason: 'unreadable', filename: 'notes.txt' })

    const launch = consumePendingChatLaunch()
    expect(launch).toBeNull()
  })

  it('rejects full or disabled storage so the caller can retain the draft', async () => {
    vi.spyOn(window.sessionStorage, 'setItem').mockImplementation(() => { throw new DOMException('Full', 'QuotaExceededError') })
    await expect(writePendingChatLaunch({ text: 'keep my draft' })).rejects.toMatchObject({ reason: 'storage' })
    expect(consumePendingChatLaunch()).toBeNull()
  })

  it('bounds the payload without silently truncating it', async () => {
    await expect(writePendingChatLaunch({ text: 'x'.repeat(MAX_PENDING_LAUNCH_CHARS) })).rejects.toMatchObject({ reason: 'too_large' })
    expect(consumePendingChatLaunch()).toBeNull()
  })

  it('keeps a normal launch in the current tab only and consumes it once', async () => {
    await writePendingChatLaunch({ text: 'tab-local draft' })
    expect(localStorage.length).toBe(0)
    expect(sessionStorage.length).toBe(1)
    expect(consumePendingChatLaunch()?.text).toBe('tab-local draft')
    expect(consumePendingChatLaunch()).toBeNull()
  })

  it('never persists a temporary launch, even when browser storage is disabled', async () => {
    const write = vi.spyOn(window.sessionStorage, 'setItem').mockImplementation(() => { throw new Error('disabled') })
    await writePendingChatLaunch({ text: 'private draft', zdr: true })
    expect(write).not.toHaveBeenCalled()
    expect(consumePendingChatLaunch()).toMatchObject({ text: 'private draft', zdr: true })
    expect(consumePendingChatLaunch()).toBeNull()
  })
})

describe('pending chat launch attachments beyond the sessionStorage envelope', () => {
  it('carries two 3 MB files through the handoff without putting their bytes in sessionStorage', async () => {
    await writePendingChatLaunch({ text: 'two big files', attachments: [bigFile('a'), bigFile('b')] })

    const envelope = window.sessionStorage.getItem('verevon.chat.pendingLaunch') ?? ''
    expect(envelope.length).toBeLessThan(MAX_PENDING_LAUNCH_CHARS)
    expect(envelope).not.toContain('AAAA')
    expect(idbData.size).toBe(2)

    const launch = consumePendingChatLaunch()
    expect(launch?.attachments?.map((attachment) => attachment.url.length)).toEqual([4_000_000 + 37, 4_000_000 + 37])
    await settle()
    expect(idbData.size).toBe(0)
    expect(window.sessionStorage.length).toBe(0)
  })

  it('restores the bytes from IndexedDB after a reload, and the sync consume leaves them for it', async () => {
    await writePendingChatLaunch({ text: 'survive a reload', attachments: [bigFile('a')] })
    vi.resetModules()
    const reloaded = await import('./pending-chat-launch')

    expect(reloaded.consumePendingChatLaunch()).toBeNull()
    expect(window.sessionStorage.length).toBe(1)

    const launch = await reloaded.consumePendingChatLaunchAsync()
    expect(launch?.text).toBe('survive a reload')
    expect(launch?.attachments?.[0]?.url).toBe(bigFile('a').url)
    await settle()
    expect(idbData.size).toBe(0)
    expect(window.sessionStorage.length).toBe(0)
  })

  it('drops a reloaded launch whose attachment bytes are gone instead of sending it without the file', async () => {
    await writePendingChatLaunch({ text: 'lost file', attachments: [bigFile('a')] })
    vi.resetModules()
    const reloaded = await import('./pending-chat-launch')
    idbData.clear()

    expect(await reloaded.consumePendingChatLaunchAsync()).toBeNull()
    expect(window.sessionStorage.length).toBe(0)
  })

  it('refuses when IndexedDB is blocked and leaves no half-written launch', async () => {
    vi.stubGlobal('indexedDB', fakeIndexedDb(idbData, { blocked: true }))
    await expect(writePendingChatLaunch({ text: 'private window', attachments: [bigFile('a')] }))
      .rejects.toMatchObject({ reason: 'storage' })
    expect(window.sessionStorage.length).toBe(0)
    expect(consumePendingChatLaunch()).toBeNull()
  })

  it('never opens IndexedDB for a temporary launch with attachments', async () => {
    const indexedDb = fakeIndexedDb(idbData)
    vi.stubGlobal('indexedDB', indexedDb)
    await writePendingChatLaunch({ text: 'private files', zdr: true, attachments: [bigFile('a')] })
    expect(indexedDb.open).not.toHaveBeenCalled()
    expect(window.sessionStorage.length).toBe(0)
    expect(consumePendingChatLaunch()?.attachments?.[0]?.url).toBe(bigFile('a').url)
    await settle()
    expect(indexedDb.open).not.toHaveBeenCalled()
  })

  it('still bounds the text envelope when attachments ride IndexedDB', async () => {
    await expect(writePendingChatLaunch({ text: 'x'.repeat(MAX_PENDING_LAUNCH_CHARS), attachments: [bigFile('a')] }))
      .rejects.toMatchObject({ reason: 'too_large' })
    expect(idbData.size).toBe(0)
  })
})
