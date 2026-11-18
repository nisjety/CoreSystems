import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { ApiError } from '@/shared/api/http'

const spacesClient = vi.hoisted(() => ({
  createSpacePost: vi.fn(),
}))
vi.mock('@/shared/api/spaces-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/spaces-client')>()),
  ...spacesClient,
}))

const { SpaceShareToRoomDialog } = await import('./SpaceShareToRoomDialog')

const ROOM = 'room-1'
const RECEIPT = { threadId: 'thr_room_1', messageId: 'msg_1', sequence: 1, deduplicated: false }

// Deterministic, UUID-shaped send ids. The dialog mints one id per send, and a new one only when the text changes.
let uuidCount = 0
const UUID_SHAPED = /^00000000-0000-4000-8000-\d{12}$/

/** The `clientSendId` of each post the dialog has sent, in order. */
const sentSendIds = () => spacesClient.createSpacePost.mock.calls.map((call) => call[1].clientSendId)

function renderDialog(overrides: { initialText?: string; maxChars?: number } = {}) {
  const onPosted = vi.fn()
  const onClose = vi.fn()
  render(() => (
    <SpaceShareToRoomDialog
      spaceRef={ROOM}
      initialText={overrides.initialText ?? 'Hei Kari'}
      maxChars={overrides.maxChars}
      onClose={onClose}
      onPosted={onPosted}
    />
  ))
  return { onPosted, onClose }
}

const textbox = () => screen.getByRole('textbox') as HTMLTextAreaElement
const postButton = () => screen.getByRole('button', { name: 'Legg ut i rommet' }) as HTMLButtonElement

function typeText(value: string) {
  fireEvent.input(textbox(), { target: { value } })
  flush()
}

beforeEach(() => {
  spacesClient.createSpacePost.mockReset()
  spacesClient.createSpacePost.mockResolvedValue(RECEIPT)
  uuidCount = 0
  vi.stubGlobal('crypto', { randomUUID: () => `00000000-0000-4000-8000-${String(++uuidCount).padStart(12, '0')}` })
})

afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
})

describe('SpaceShareToRoomDialog (WS17-S6)', () => {
  it('posts nothing until the post button is pressed', async () => {
    renderDialog({ initialText: 'Hei Kari' })
    flush()
    expect(spacesClient.createSpacePost).not.toHaveBeenCalled()

    typeText('Hei Kari, takk')
    expect(spacesClient.createSpacePost).not.toHaveBeenCalled()

    fireEvent.click(postButton())
    flush()
    await waitFor(() => expect(spacesClient.createSpacePost).toHaveBeenCalledTimes(1))
    expect(spacesClient.createSpacePost).toHaveBeenCalledWith(ROOM, {
      content: 'Hei Kari, takk',
      clientSendId: expect.any(String),
    })
  })

  it('disables the post button for empty, blank or over-limit text', () => {
    renderDialog({ initialText: '', maxChars: 12 })
    flush()
    expect(postButton().disabled).toBe(true)

    typeText('   ')
    expect(postButton().disabled).toBe(true)

    typeText('Hei Kari, takk!')
    expect(postButton().disabled).toBe(true)
    expect(textbox().getAttribute('aria-invalid')).toBe('true')
    expect(screen.getByText(/For lang for rommet/)).toBeTruthy()

    typeText('Hei')
    expect(postButton().disabled).toBe(false)
    expect(textbox().getAttribute('aria-invalid')).toBeNull()
    expect(spacesClient.createSpacePost).not.toHaveBeenCalled()
  })

  it('a retry of the same text reuses its clientSendId', async () => {
    spacesClient.createSpacePost
      .mockRejectedValueOnce(new ApiError('unavailable', 503, null))
      .mockResolvedValueOnce(RECEIPT)
    const { onPosted, onClose } = renderDialog({ initialText: 'Hei Kari' })

    fireEvent.click(postButton())
    flush()
    expect(await screen.findByRole('alert')).toBeTruthy()
    expect(postButton().disabled).toBe(false)

    fireEvent.click(postButton())
    flush()
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1))
    const ids = sentSendIds()
    expect(ids).toHaveLength(2)
    expect(ids[0]).toMatch(UUID_SHAPED)
    expect(ids[1]).toBe(ids[0])
    expect(spacesClient.createSpacePost.mock.calls[1]?.[1]?.content).toBe('Hei Kari')
    expect(onPosted).toHaveBeenCalledTimes(1)
  })

  it('changed text is a new send with a new clientSendId', async () => {
    spacesClient.createSpacePost
      .mockRejectedValueOnce(new ApiError('unavailable', 503, null))
      .mockResolvedValueOnce(RECEIPT)
    const { onClose } = renderDialog({ initialText: 'Hei Kari' })

    fireEvent.click(postButton())
    flush()
    await screen.findByRole('alert')

    typeText('Hei Kari, takk')
    fireEvent.click(postButton())
    flush()
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1))
    const ids = sentSendIds()
    expect(ids).toHaveLength(2)
    expect(ids[0]).toMatch(UUID_SHAPED)
    expect(ids[1]).toMatch(UUID_SHAPED)
    expect(ids[1]).not.toBe(ids[0])
    expect(spacesClient.createSpacePost.mock.calls[1]?.[1]?.content).toBe('Hei Kari, takk')
  })

  it('a success calls onPosted and then onClose', async () => {
    const { onPosted, onClose } = renderDialog({ initialText: 'Hei Kari' })

    fireEvent.click(postButton())
    flush()
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1))
    expect(onPosted).toHaveBeenCalledTimes(1)
    const postedAt = onPosted.mock.invocationCallOrder[0] ?? Number.NaN
    const closedAt = onClose.mock.invocationCallOrder[0] ?? Number.NaN
    expect(postedAt).toBeLessThan(closedAt)
    expect(screen.queryByRole('alert')).toBeNull()
  })

  it('a 403 space_post_not_authorized shows the denial copy and keeps the text', async () => {
    spacesClient.createSpacePost.mockRejectedValue(new ApiError('forbidden', 403, 'space_post_not_authorized'))
    const { onPosted, onClose } = renderDialog({ initialText: 'Hei Kari' })

    fireEvent.click(postButton())
    flush()
    const alert = await screen.findByRole('alert')
    expect(alert.textContent).toContain('Rollen din i dette rommet tillater ikke å skrive her')
    expect(textbox().value).toBe('Hei Kari')
    expect(postButton().disabled).toBe(false)
    expect(onPosted).not.toHaveBeenCalled()
    expect(onClose).not.toHaveBeenCalled()
  })
})
