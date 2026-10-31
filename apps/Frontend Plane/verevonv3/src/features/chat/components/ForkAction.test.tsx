import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { ForkAction, threadForkEnabled } from './ForkAction'

const { forkChatThread } = vi.hoisted(() => ({ forkChatThread: vi.fn() }))
vi.mock('@/shared/api/chat-client', () => ({ forkChatThread }))
vi.mock('@/shared/i18n', () => ({ useI18n: () => ({ tr: (_no: string, en: string) => en }) }))

afterEach(() => {
  cleanup()
  forkChatThread.mockReset()
})

describe('ForkAction', () => {
  it('is off unless the build turns it on', () => {
    expect(threadForkEnabled()).toBe(false)
  })

  it('forks from the chosen turn and hands the new thread to the caller', async () => {
    forkChatThread.mockResolvedValue({ threadId: 'thread-fork' })
    const onForked = vi.fn()
    render(() => <ForkAction threadId="thread-source" messageId="turn-4" onForked={onForked} />)

    fireEvent.click(screen.getByRole('button', { name: 'Fork from here' }))

    await waitFor(() => expect(onForked).toHaveBeenCalledWith('thread-fork'))
    expect(forkChatThread).toHaveBeenCalledWith('thread-source', { fromMessageId: 'turn-4' })
  })

  it('goes away once the deployment answers that forking is off', async () => {
    forkChatThread.mockRejectedValue(Object.assign(new Error('off'), { code: 'history_fork_disabled' }))
    render(() => <ForkAction threadId="thread-source" messageId="turn-4" />)

    fireEvent.click(screen.getByRole('button', { name: 'Fork from here' }))

    await waitFor(() => expect(screen.queryByRole('button', { name: 'Fork from here' })).toBeNull())
  })

  it('says so in place when the copy is refused', async () => {
    forkChatThread.mockRejectedValue(Object.assign(new Error('refused'), { code: 'history_fork_refused' }))
    render(() => <ForkAction threadId="thread-source" messageId="turn-4" />)

    fireEvent.click(screen.getByRole('button', { name: 'Fork from here' }))

    const alert = await screen.findByRole('alert')
    expect(alert.textContent).toContain('This chat could not be forked.')
    expect(screen.getByRole('button', { name: 'Fork from here' })).toBeTruthy()
  })
})
