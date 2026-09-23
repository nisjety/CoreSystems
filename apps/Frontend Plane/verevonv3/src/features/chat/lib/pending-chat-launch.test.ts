// @vitest-environment jsdom

import { beforeEach, afterEach, describe, expect, it, vi } from 'vitest'

vi.mock('@/shared/lib/blob-data', () => ({
  objectUrlToDataUrl: vi.fn((url: string) => Promise.resolve(`data:application/octet-stream;base64,converted-${url}`)),
}))

import { objectUrlToDataUrl } from '@/shared/lib/blob-data'
import { consumePendingChatLaunch, writePendingChatLaunch, MAX_PENDING_LAUNCH_CHARS } from './pending-chat-launch'

afterEach(() => { vi.restoreAllMocks(); consumePendingChatLaunch() })

beforeEach(() => {
  window.localStorage.clear()
  window.sessionStorage.clear()
  vi.mocked(objectUrlToDataUrl).mockClear()
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
