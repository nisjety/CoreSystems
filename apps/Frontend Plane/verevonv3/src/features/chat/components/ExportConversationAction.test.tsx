import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { flush } from 'solid-js'
import { ExportConversationAction, threadExportEnabled } from './ExportConversationAction'
import { THREAD_EXPORT_FORMAT } from '../replay/thread-export-schema'
import { applyServerRetention, __resetRetentionForTests } from '@/features/chat/lib/chat-retention'

const { exportThread } = vi.hoisted(() => ({ exportThread: vi.fn() }))
const { downloadBytes } = vi.hoisted(() => ({ downloadBytes: vi.fn() }))
vi.mock('@/shared/api/chat-client', () => ({ exportThread }))
vi.mock('@/features/chat/lib/chat-export/download', () => ({ downloadBytes }))
vi.mock('@/shared/i18n', () => ({ useI18n: () => ({ tr: (_no: string, en: string) => en, locale: () => 'en' }) }))

// A redacted v1 export with one turn that the replay can show.
const replayFile = JSON.stringify({
  format: THREAD_EXPORT_FORMAT,
  version: 1,
  exported_at: '2026-11-05T10:00:00Z',
  profile: 'redacted',
  thread: { id: 'thread-1', title: 'Plan', created_at: '2026-11-04T09:00:00Z' },
  messages: [
    {
      id: 'm-1',
      role: 'user',
      content: 'Hello, world',
      created_at: '2026-11-04T09:00:01Z',
      artifacts: [],
      citations: [],
      metadata: {},
    },
  ],
  runs: [],
  truncated: false,
})

afterEach(() => {
  cleanup()
  exportThread.mockReset()
  downloadBytes.mockReset()
  vi.unstubAllEnvs()
  __resetRetentionForTests()
})

function renderMenu(props: { temporaryChat?: boolean; spaceScoped?: boolean } = {}) {
  return render(() => <ExportConversationAction threadId="thread-1" {...props} />)
}

// Solid applies a signal's change to the DOM on the next flush, so each interaction is flushed before the test reads it.
function openMenu() {
  fireEvent.click(screen.getByRole('button', { name: 'Export conversation' }))
  flush()
}

function choose(name: string) {
  fireEvent.click(screen.getByRole('radio', { name }))
  flush()
}

describe('ExportConversationAction', () => {
  it('is off unless the build turns it on, with the strict literal true', () => {
    expect(threadExportEnabled()).toBe(false)
    vi.stubEnv('VITE_THREAD_EXPORT', 'yes')
    expect(threadExportEnabled()).toBe(false)
    vi.stubEnv('VITE_THREAD_EXPORT', 'true')
    expect(threadExportEnabled()).toBe(true)
  })

  it('keeps the format and the content unchosen, and the download off, until the person picks both', () => {
    renderMenu()
    openMenu()

    expect((screen.getByRole('radio', { name: 'JSON' }) as HTMLInputElement).checked).toBe(false)
    expect((screen.getByRole('radio', { name: 'Redacted' }) as HTMLInputElement).checked).toBe(false)
    expect((screen.getByRole('button', { name: 'Download' }) as HTMLButtonElement).disabled).toBe(true)
  })

  it('downloads the chosen format and content under the name the server gave the file', async () => {
    exportThread.mockResolvedValue({
      body: '# Plan',
      mediaType: 'text/markdown; charset=utf-8',
      fileName: 'verevon-thread-thread-1.md',
    })
    renderMenu()
    openMenu()

    choose('Markdown')
    choose('Redacted')
    fireEvent.click(screen.getByRole('button', { name: 'Download' }))

    await waitFor(() => expect(downloadBytes).toHaveBeenCalledTimes(1))
    expect(exportThread).toHaveBeenCalledWith('thread-1', 'markdown', 'redacted')
    expect(downloadBytes).toHaveBeenCalledWith(
      expect.any(Uint8Array),
      'verevon-thread-thread-1.md',
      'text/markdown; charset=utf-8',
    )
  })

  it('says why a thread may not leave in full, in place, from the typed reason', async () => {
    exportThread.mockRejectedValue(
      Object.assign(new Error('refused'), { code: 'thread_not_exportable', details: { reason: 'non_persistent_run' } }),
    )
    renderMenu()
    openMenu()

    choose('JSON')
    choose('Full')
    fireEvent.click(screen.getByRole('button', { name: 'Download' }))

    const alert = await screen.findByRole('alert')
    expect(alert.textContent).toContain('cannot be exported in full')
    expect(downloadBytes).not.toHaveBeenCalled()
  })

  it('names the feature a zero-retention refusal is about', async () => {
    exportThread.mockRejectedValue(
      Object.assign(new Error('refused'), { code: 'zdr_feature_not_supported', feature: 'conversation_export' }),
    )
    renderMenu()
    openMenu()

    choose('JSON')
    choose('Full')
    fireEvent.click(screen.getByRole('button', { name: 'Download' }))

    const alert = await screen.findByRole('alert')
    expect(alert.textContent).toContain('conversation export and replay')
  })

  it('goes away once the deployment answers that exporting is off', async () => {
    exportThread.mockRejectedValue(Object.assign(new Error('off'), { code: 'thread_export_disabled' }))
    renderMenu()
    openMenu()

    choose('JSON')
    choose('Full')
    fireEvent.click(screen.getByRole('button', { name: 'Download' }))

    await waitFor(() => expect(screen.queryByRole('button', { name: 'Export conversation' })).toBeNull())
  })

  it('is not offered for a temporary chat', () => {
    renderMenu({ temporaryChat: true })

    expect(screen.queryByRole('button', { name: 'Export conversation' })).toBeNull()
  })

  it('is not offered for a thread scoped to a Space', () => {
    renderMenu({ spaceScoped: true })

    expect(screen.queryByRole('button', { name: 'Export conversation' })).toBeNull()
  })

  it('is not offered to a zero-retention caller', () => {
    applyServerRetention(true)
    renderMenu()

    expect(screen.queryByRole('button', { name: 'Export conversation' })).toBeNull()
  })

  it('opens the redacted replay of the conversation', async () => {
    exportThread.mockResolvedValue({ body: replayFile, mediaType: 'application/json; charset=utf-8', fileName: 'x.json' })
    renderMenu()
    openMenu()

    fireEvent.click(screen.getByRole('button', { name: 'Replay conversation' }))

    const dialog = await screen.findByRole('dialog')
    expect(exportThread).toHaveBeenCalledWith('thread-1', 'json', 'redacted')
    expect(dialog.textContent).toContain('Hello, world')
  })

  it('refuses an export version it does not know, and shows none of its contents', async () => {
    exportThread.mockResolvedValue({
      body: JSON.stringify({ ...JSON.parse(replayFile), version: 2 }),
      mediaType: 'application/json; charset=utf-8',
      fileName: 'x.json',
    })
    renderMenu()
    openMenu()

    fireEvent.click(screen.getByRole('button', { name: 'Replay conversation' }))

    const alert = await screen.findByRole('alert')
    expect(alert.textContent).toContain('cannot be shown here')
    expect(screen.queryByRole('dialog')).toBeNull()
  })

  it('closes the replay on Escape', async () => {
    exportThread.mockResolvedValue({ body: replayFile, mediaType: 'application/json; charset=utf-8', fileName: 'x.json' })
    renderMenu()
    openMenu()
    fireEvent.click(screen.getByRole('button', { name: 'Replay conversation' }))
    await screen.findByRole('dialog')
    // The Escape listener is attached by an effect after the viewer mounts.
    flush()

    fireEvent.keyDown(document, { key: 'Escape' })

    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
  })
})
