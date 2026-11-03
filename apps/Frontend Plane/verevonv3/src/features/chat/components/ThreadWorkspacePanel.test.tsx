import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { ThreadWorkspacePanel, threadWorkspaceEnabled } from './ThreadWorkspacePanel'

const { requestJson } = vi.hoisted(() => ({ requestJson: vi.fn() }))
vi.mock('@/shared/api/http', () => ({ requestJson }))
vi.mock('@/shared/i18n', () => ({ useI18n: () => ({ tr: (_no: string, en: string) => en }) }))

const workspacePath = '/api/v1/chat/threads/thr_1/workspace'

afterEach(() => {
  cleanup()
  requestJson.mockReset()
  vi.unstubAllEnvs()
})

describe('ThreadWorkspacePanel', () => {
  it('is off unless the build turns the thread workspace on', () => {
    expect(threadWorkspaceEnabled()).toBe(false)
    render(() => <ThreadWorkspacePanel threadId="thr_1" />)

    expect(screen.queryByRole('button', { name: 'Thread files' })).toBeNull()
    expect(requestJson).not.toHaveBeenCalled()
  })

  it('lists the files the thread keeps when it is opened', async () => {
    vi.stubEnv('VITE_CHAT_THREAD_WORKSPACE', 'true')
    requestJson.mockResolvedValue({
      thread_id: 'thr_1',
      revision: 2,
      files: [{ path: 'notes.txt', content_hash: `sha256:${'a'.repeat(64)}`, size_bytes: 12 }],
    })
    render(() => <ThreadWorkspacePanel threadId="thr_1" />)

    fireEvent.click(screen.getByRole('button', { name: 'Thread files' }))

    expect(await screen.findByText('notes.txt')).toBeTruthy()
    expect(screen.getByText('12 B')).toBeTruthy()
    expect(requestJson).toHaveBeenCalledWith(workspacePath)
  })

  // The reset erases files for good, so one press only asks for confirmation.
  it('erases the files only on the second press', async () => {
    vi.stubEnv('VITE_CHAT_THREAD_WORKSPACE', 'true')
    requestJson
      .mockResolvedValueOnce({
        thread_id: 'thr_1',
        revision: 1,
        files: [{ path: 'notes.txt', content_hash: `sha256:${'b'.repeat(64)}`, size_bytes: 5 }],
      })
      .mockResolvedValueOnce({ thread_id: 'thr_1', files_deleted: 1, object_prefix: 'cas/thread/org-1/thr_1/' })
    render(() => <ThreadWorkspacePanel threadId="thr_1" />)
    fireEvent.click(screen.getByRole('button', { name: 'Thread files' }))
    await screen.findByText('notes.txt')

    fireEvent.click(screen.getByRole('button', { name: 'Reset thread files' }))
    expect(requestJson).toHaveBeenCalledTimes(1)

    fireEvent.click(await screen.findByRole('button', { name: 'Confirm reset' }))
    await waitFor(() => expect(screen.getByText('Thread files were reset.')).toBeTruthy())
    expect(requestJson).toHaveBeenLastCalledWith(workspacePath, { method: 'DELETE' })
    expect(screen.queryByText('notes.txt')).toBeNull()
  })

  it('says so in place when the files cannot be loaded', async () => {
    vi.stubEnv('VITE_CHAT_THREAD_WORKSPACE', 'true')
    requestJson.mockRejectedValue(new Error('upstream down'))
    render(() => <ThreadWorkspacePanel threadId="thr_1" />)

    fireEvent.click(screen.getByRole('button', { name: 'Thread files' }))

    const alert = await screen.findByRole('alert')
    expect(alert.textContent).toContain('The files could not be loaded.')
  })
})
