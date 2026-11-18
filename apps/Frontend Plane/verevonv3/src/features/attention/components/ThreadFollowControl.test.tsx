// @vitest-environment jsdom

import { flush } from 'solid-js'
import { cleanup, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { ThreadFollowControl } from './ThreadFollowControl'
import { ApiError } from '@/shared/api/http'
import { I18nProvider } from '@/shared/i18n'
import { fireEvent } from '@/test/fire-event'
import { render } from '@/test/render'

// WS11-S7: follow or mute one post. The room's flag and the follow client are replaced, so each test
// states what the room and the server said and checks what the control draws.

const room = vi.hoisted(() => ({ attention: true }))
const follow = vi.hoisted(() => ({ getThreadFollow: vi.fn(), setThreadFollow: vi.fn() }))

vi.mock('@/features/spaces/lib/space-room-features', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/features/spaces/lib/space-room-features')>()),
  spaceRoomFeature: (name: string) => name === 'attention' && room.attention,
}))
vi.mock('@/shared/api/spaces-attention-items-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/spaces-attention-items-client')>()),
  getThreadFollow: follow.getThreadFollow,
  setThreadFollow: follow.setThreadFollow,
}))

/** One macrotask, so a settled promise has run its continuation before the control is read. */
const settle = () => new Promise<void>((done) => setTimeout(done, 0))

function renderControl() {
  return render(() => (
    <I18nProvider>
      <ThreadFollowControl spaceRef="room-a" threadId="thread-1" />
    </I18nProvider>
  ))
}

beforeEach(() => {
  room.attention = true
  follow.getThreadFollow.mockReset()
  follow.setThreadFollow.mockReset()
})

afterEach(() => cleanup())

describe('ThreadFollowControl', () => {
  it('draws nothing while the room has not turned attention on, and does not read the follow', () => {
    room.attention = false

    const { container } = renderControl()

    expect(container.querySelector('.verevon-thread-follow')).toBeNull()
    expect(follow.getThreadFollow).not.toHaveBeenCalled()
  })

  it('draws nothing while the read is unknown, rather than guessing that the viewer is not following', async () => {
    follow.getThreadFollow.mockRejectedValue(new TypeError('Failed to fetch'))

    const { container } = renderControl()
    await waitFor(() => expect(follow.getThreadFollow).toHaveBeenCalledWith('room-a', 'thread-1'))
    await settle()
    flush()

    expect(container.querySelector('.verevon-thread-follow')).toBeNull()
    expect(follow.setThreadFollow).not.toHaveBeenCalled()
  })

  it('follows the thread, and pressing Following again clears the follow', async () => {
    follow.getThreadFollow.mockResolvedValue('none')
    follow.setThreadFollow.mockResolvedValueOnce('following').mockResolvedValueOnce('none')
    renderControl()

    fireEvent.click(await screen.findByRole('button', { name: 'Følg' }))
    const following = await screen.findByRole('button', { name: 'Følger' })
    expect(following.getAttribute('aria-pressed')).toBe('true')
    expect(follow.setThreadFollow).toHaveBeenLastCalledWith('room-a', 'thread-1', 'following')

    fireEvent.click(following)
    expect(await screen.findByRole('button', { name: 'Følg' })).toBeTruthy()
    expect(follow.setThreadFollow).toHaveBeenLastCalledWith('room-a', 'thread-1', 'none')
  })

  it('mutes the thread, and pressing Muted again clears the mute', async () => {
    follow.getThreadFollow.mockResolvedValue('none')
    follow.setThreadFollow.mockResolvedValueOnce('muted').mockResolvedValueOnce('none')
    renderControl()

    fireEvent.click(await screen.findByRole('button', { name: 'Demp' }))
    const muted = await screen.findByRole('button', { name: 'Dempet' })
    expect(muted.getAttribute('aria-pressed')).toBe('true')
    expect(follow.setThreadFollow).toHaveBeenLastCalledWith('room-a', 'thread-1', 'muted')

    fireEvent.click(muted)
    expect(await screen.findByRole('button', { name: 'Demp' })).toBeTruthy()
    expect(follow.setThreadFollow).toHaveBeenLastCalledWith('room-a', 'thread-1', 'none')
  })

  it('replaces a mute with a follow', async () => {
    follow.getThreadFollow.mockResolvedValue('muted')
    follow.setThreadFollow.mockResolvedValueOnce('following')
    renderControl()

    fireEvent.click(await screen.findByRole('button', { name: 'Følg' }))

    expect(await screen.findByRole('button', { name: 'Følger' })).toBeTruthy()
    expect(follow.setThreadFollow).toHaveBeenLastCalledWith('room-a', 'thread-1', 'following')
    expect(screen.getByRole('button', { name: 'Demp' }).getAttribute('aria-pressed')).toBe('false')
  })

  it('says the refusal in words when a follow is refused under zero data retention, and leaves the state as it was', async () => {
    follow.getThreadFollow.mockResolvedValue('none')
    // The ApiError the transport throws for the gateway's typed 412 on a durable write.
    follow.setThreadFollow.mockRejectedValue(
      new ApiError('refused under zero data retention', 412, 'zdr_durable_mutation_forbidden'),
    )
    renderControl()

    fireEvent.click(await screen.findByRole('button', { name: 'Følg' }))

    expect(
      await screen.findByText('Legitimasjon med Zero Data Retention kan ikke opprette, endre eller slette varig data.'),
    ).toBeTruthy()
    expect(screen.queryByText('refused under zero data retention')).toBeNull()
    const followButton = screen.getByRole('button', { name: 'Følg' })
    expect(followButton.getAttribute('aria-pressed')).toBe('false')
    expect(followButton.hasAttribute('disabled')).toBe(false)
  })
})
