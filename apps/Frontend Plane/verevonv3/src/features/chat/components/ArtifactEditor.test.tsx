// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import * as chatClient from '@/shared/api/chat-client'
import { ApiError } from '@/shared/api/api-error'
import { ArtifactEditor, type ArtifactEditorHead } from './ArtifactEditor'

vi.mock('@/shared/api/chat-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-client')>()
  return { ...actual, saveArtifactRevision: vi.fn() }
})

afterEach(() => {
  cleanup()
  vi.mocked(chatClient.saveArtifactRevision).mockReset()
})

function saved(version: number, content: string): chatClient.SavedArtifactRevision {
  return {
    id: `rev-${version}`,
    artifactId: 'report',
    version,
    kind: 'document',
    title: 'Rapport',
    content,
    editorKind: 'user',
    createdAt: '2026-11-04T10:00:00Z',
  }
}

function mount(overrides: {
  onSaved?: (revision: chatClient.SavedArtifactRevision) => void
  onClose?: () => void
  newest?: () => ArtifactEditorHead | undefined
} = {}) {
  return render(() => (
    <ArtifactEditor
      threadId="thread-a"
      artifactId="report"
      title="Rapport"
      content="Første utkast"
      headVersion={2}
      onSaved={overrides.onSaved ?? (() => {})}
      onClose={overrides.onClose ?? (() => {})}
      newest={overrides.newest}
    />
  ))
}

function type(text: string) {
  fireEvent.input(screen.getByRole('textbox', { name: 'Rediger Rapport' }), { target: { value: text } })
  flush()
}

describe('ArtifactEditor', () => {
  it('save_sends_expected_version: saves the typed text against the version it opened', async () => {
    vi.mocked(chatClient.saveArtifactRevision).mockResolvedValue(saved(3, 'Mitt utkast'))
    const onSaved = vi.fn()
    mount({ onSaved })

    const save = screen.getByRole('button', { name: 'Lagre' }) as HTMLButtonElement
    expect(save.disabled).toBe(true)
    type('Mitt utkast')
    expect(save.disabled).toBe(false)
    fireEvent.click(save)

    await waitFor(() => expect(onSaved).toHaveBeenCalledTimes(1))
    expect(chatClient.saveArtifactRevision).toHaveBeenCalledWith({
      threadId: 'thread-a',
      artifactId: 'report',
      content: 'Mitt utkast',
      expectedVersion: 2,
    })
    expect(onSaved.mock.calls[0]?.[0]).toMatchObject({ version: 3, content: 'Mitt utkast' })
  })

  it('conflict_banner_offers_reload_and_overwrite: overwrite only after an explicit click, against the new head', async () => {
    vi.mocked(chatClient.saveArtifactRevision)
      .mockRejectedValueOnce(new ApiError('moved', 409, 'revision_mismatch', { details: { head_version: 4 } }))
      .mockResolvedValueOnce(saved(5, 'Mitt utkast'))
    const onSaved = vi.fn()
    mount({ onSaved })

    type('Mitt utkast')
    fireEvent.click(screen.getByRole('button', { name: 'Lagre' }))
    await waitFor(() => expect(screen.getByRole('alert').textContent).toContain('versjon 4'))
    expect(chatClient.saveArtifactRevision).toHaveBeenCalledTimes(1)
    expect(onSaved).not.toHaveBeenCalled()
    expect(screen.getByRole('button', { name: 'Last inn nyeste' })).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: 'Overskriv' }))
    await waitFor(() => expect(onSaved).toHaveBeenCalledTimes(1))
    expect(vi.mocked(chatClient.saveArtifactRevision).mock.calls[1]?.[0]).toMatchObject({
      content: 'Mitt utkast',
      expectedVersion: 4,
    })
  })

  it('conflict_banner_offers_reload_and_overwrite: load newest replaces the text with the head the panel holds', async () => {
    vi.mocked(chatClient.saveArtifactRevision)
      .mockRejectedValueOnce(new ApiError('moved', 409, 'revision_mismatch', { details: { head_version: 4 } }))
    mount({ newest: () => ({ content: 'Verevons versjon 4', title: 'Rapport', version: 4 }) })

    type('Mitt utkast')
    fireEvent.click(screen.getByRole('button', { name: 'Lagre' }))
    await waitFor(() => expect(screen.getByRole('button', { name: 'Last inn nyeste' })).toBeTruthy())
    fireEvent.click(screen.getByRole('button', { name: 'Last inn nyeste' }))
    flush()

    const textarea = screen.getByRole('textbox', { name: 'Rediger Rapport' }) as HTMLTextAreaElement
    expect(textarea.value).toBe('Verevons versjon 4')
    expect(screen.queryByRole('button', { name: 'Overskriv' })).toBeNull()
    // Nothing typed since loading: there is nothing to save.
    expect((screen.getByRole('button', { name: 'Lagre' }) as HTMLButtonElement).disabled).toBe(true)
  })

  it('conflict_banner_offers_reload_and_overwrite: an older copy in the panel is not offered as the newest', async () => {
    vi.mocked(chatClient.saveArtifactRevision)
      .mockRejectedValueOnce(new ApiError('moved', 409, 'revision_mismatch', { details: { head_version: 4 } }))
    mount({ newest: () => ({ content: 'Første utkast', title: 'Rapport', version: 2 }) })

    type('Mitt utkast')
    fireEvent.click(screen.getByRole('button', { name: 'Lagre' }))
    await waitFor(() => expect(screen.getByRole('button', { name: 'Last inn nyeste' })).toBeTruthy())
    fireEvent.click(screen.getByRole('button', { name: 'Last inn nyeste' }))
    flush()

    expect((screen.getByRole('textbox', { name: 'Rediger Rapport' }) as HTMLTextAreaElement).value).toBe('Mitt utkast')
    expect(document.body.textContent).toContain('Last inn samtalen på nytt')
  })

  it('dirty_guard_on_close: cancel with unsaved text asks before it discards', () => {
    const onClose = vi.fn()
    mount({ onClose })

    type('Mitt utkast')
    fireEvent.click(screen.getByRole('button', { name: 'Avbryt' }))
    flush()
    expect(onClose).not.toHaveBeenCalled()
    expect(screen.getByRole('alertdialog', { name: 'Forkaste endringene?' })).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: 'Fortsett å redigere' }))
    flush()
    expect(onClose).not.toHaveBeenCalled()
    expect((screen.getByRole('textbox', { name: 'Rediger Rapport' }) as HTMLTextAreaElement).value).toBe('Mitt utkast')

    fireEvent.click(screen.getByRole('button', { name: 'Avbryt' }))
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'Forkast' }))
    expect(onClose).toHaveBeenCalledTimes(1)
  })

  it('dirty_guard_on_close: an untouched editor closes at once and leaving the page is only guarded while dirty', () => {
    const onClose = vi.fn()
    mount({ onClose })
    flush()

    const clean = new Event('beforeunload', { cancelable: true })
    window.dispatchEvent(clean)
    expect(clean.defaultPrevented).toBe(false)

    type('Mitt utkast')
    const dirty = new Event('beforeunload', { cancelable: true })
    window.dispatchEvent(dirty)
    expect(dirty.defaultPrevented).toBe(true)

    type('Første utkast')
    fireEvent.click(screen.getByRole('button', { name: 'Avbryt' }))
    expect(onClose).toHaveBeenCalledTimes(1)
  })

  it('shows a refusal in the person\'s words and never the server message', async () => {
    vi.mocked(chatClient.saveArtifactRevision)
      .mockRejectedValueOnce(new ApiError('internal detail', 409, 'artifact_edit_run_active'))
    mount()

    type('Mitt utkast')
    fireEvent.click(screen.getByRole('button', { name: 'Lagre' }))
    await waitFor(() => expect(screen.getByRole('alert').textContent).toContain('Verevon jobber fortsatt'))
    expect(document.body.textContent).not.toContain('internal detail')
  })
})
