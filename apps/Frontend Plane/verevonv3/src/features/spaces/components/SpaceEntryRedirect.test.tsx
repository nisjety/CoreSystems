import { createRouter, memoryHistory, useLocation } from '@solidjs/router'
import { cleanup, fireEvent, render, screen } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { ApiError } from '@/shared/api/http'

import SpaceEntryRedirect from './SpaceEntryRedirect'

// The resolver and the rollout read sit behind the spaces client; the component's own flag check
// (`VITE_SPACE_ENTRY_LINKS_UI`, read through spaceEntryLinksEnabled) is real.
const spacesClient = vi.hoisted(() => ({
  resolveSpaceEntry: vi.fn(),
  getSpaceFeatures: vi.fn(),
}))
vi.mock('@/shared/api/spaces-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/spaces-client')>()),
  resolveSpaceEntry: spacesClient.resolveSpaceEntry,
  getSpaceFeatures: spacesClient.getSpaceFeatures,
}))

// Where the router has landed (pathname, query and hash), recorded by a probe on the room route.
const landed = vi.hoisted(() => [] as string[])

function LandingProbe() {
  const location = useLocation()
  landed.push(`${location.pathname}${location.search}${location.hash}`)
  return <p data-testid="landed">room</p>
}

const ENTRY = {
  kind: 'message' as const,
  spaceRef: 'space_room_1',
  threadId: 'thread_9',
  messageId: 'msg_9',
}
const LANDING = '/spaces/space_room_1?thread_id=thread_9&m=msg_9#chat'
const NEUTRAL = 'Denne meldingen er ikke tilgjengelig for deg.'

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((done) => {
    resolve = done
  })
  return { promise, resolve }
}

/** Renders the redirect on its own route, beside the room route it replaces itself with. */
function renderEntry(path: string) {
  const history = memoryHistory(path)
  const TestRouter = createRouter({
    routes: [
      { path: '/spaces/:spaceId/m/:messageId', component: SpaceEntryRedirect },
      { path: '/spaces/:spaceId', component: LandingProbe },
    ],
    history,
    explicitLinks: true,
  })
  render(() => <TestRouter>{(props) => <>{props.children}</>}</TestRouter>)
  return history
}

beforeEach(() => {
  landed.length = 0
  spacesClient.resolveSpaceEntry.mockReset()
  spacesClient.getSpaceFeatures.mockReset()
  spacesClient.getSpaceFeatures.mockResolvedValue({ entry_links: true })
})

afterEach(() => {
  cleanup()
  vi.unstubAllEnvs()
})

describe('SpaceEntryRedirect (WS13-S4b)', () => {
  it('shows the opening state while the message is being resolved', async () => {
    spacesClient.resolveSpaceEntry.mockReturnValue(new Promise(() => undefined))
    renderEntry('/spaces/space_room_1/m/msg_9')
    expect(await screen.findByText('Åpner meldingen…')).toBeTruthy()
    expect(screen.getByRole('status').getAttribute('aria-live')).toBe('polite')
  })

  it('replaces itself with the room, its thread and the message, once the resolver answers', async () => {
    spacesClient.resolveSpaceEntry.mockResolvedValue(ENTRY)
    const history = renderEntry('/spaces/space_room_1/m/msg_9')

    expect(await screen.findByTestId('landed')).toBeTruthy()
    expect(spacesClient.resolveSpaceEntry).toHaveBeenCalledWith('space_room_1', 'msg_9')
    expect(landed.at(-1)).toBe(LANDING)
    // Replace, not push: stepping back from the room does not return to the message link.
    history.back()
    expect(history.get()).toBe(LANDING)
  })

  it('shows the neutral view for a 403 without echoing the message id, and navigates nowhere', async () => {
    spacesClient.resolveSpaceEntry.mockRejectedValue(new ApiError('forbidden', 403, 'space_read_not_authorized'))
    renderEntry('/spaces/space_room_1/m/msg_9')

    expect(await screen.findByText(NEUTRAL)).toBeTruthy()
    expect(document.body.textContent ?? '').not.toContain('msg_9')
    expect(screen.queryByRole('button', { name: 'Prøv igjen' })).toBeNull()
    expect(landed).toEqual([])
  })

  it('shows the same neutral view for a 404, so a missing message reads like a forbidden one', async () => {
    spacesClient.resolveSpaceEntry.mockRejectedValue(new ApiError('missing', 404, 'entry_not_found'))
    renderEntry('/spaces/space_room_1/m/msg_9')

    expect(await screen.findByText(NEUTRAL)).toBeTruthy()
    expect(document.body.textContent ?? '').not.toContain('msg_9')
    expect(screen.queryByRole('button', { name: 'Prøv igjen' })).toBeNull()
    expect(landed).toEqual([])
  })

  it('treats a malformed message id as unavailable and does not ask the resolver', async () => {
    renderEntry('/spaces/space_room_1/m/bad.id')

    expect(await screen.findByText(NEUTRAL)).toBeTruthy()
    expect(spacesClient.resolveSpaceEntry).not.toHaveBeenCalled()
    expect(document.body.textContent ?? '').not.toContain('bad.id')
  })

  it('takes the neutral view when the entry-links flag is off and the gateway does not say it is on', async () => {
    vi.stubEnv('VITE_SPACE_ENTRY_LINKS_UI', 'false')
    spacesClient.getSpaceFeatures.mockResolvedValue({ entry_links: false })
    renderEntry('/spaces/space_room_1/m/msg_9')

    expect(await screen.findByText(NEUTRAL)).toBeTruthy()
    expect(spacesClient.resolveSpaceEntry).not.toHaveBeenCalled()
    expect(landed).toEqual([])
  })

  it('still resolves with the flag off when the gateway says entry links are on', async () => {
    vi.stubEnv('VITE_SPACE_ENTRY_LINKS_UI', 'false')
    spacesClient.getSpaceFeatures.mockResolvedValue({ entry_links: true })
    spacesClient.resolveSpaceEntry.mockResolvedValue(ENTRY)
    renderEntry('/spaces/space_room_1/m/msg_9')

    expect(await screen.findByTestId('landed')).toBeTruthy()
    expect(spacesClient.resolveSpaceEntry).toHaveBeenCalledWith('space_room_1', 'msg_9')
    expect(landed.at(-1)).toBe(LANDING)
  })

  it('offers a retry for a server error, and the retry resolves again and lands', async () => {
    spacesClient.resolveSpaceEntry
      .mockRejectedValueOnce(new ApiError('boom', 503, null))
      .mockResolvedValueOnce(ENTRY)
    renderEntry('/spaces/space_room_1/m/msg_9')

    expect(await screen.findByText('Meldingen kunne ikke åpnes akkurat nå')).toBeTruthy()
    expect(landed).toEqual([])
    fireEvent.click(screen.getByRole('button', { name: 'Prøv igjen' }))

    expect(await screen.findByTestId('landed')).toBeTruthy()
    expect(spacesClient.resolveSpaceEntry).toHaveBeenCalledTimes(2)
    expect(landed.at(-1)).toBe(LANDING)
  })

  it('offers a retry when the connection is lost (no response at all)', async () => {
    spacesClient.resolveSpaceEntry.mockRejectedValueOnce(new TypeError('Failed to fetch'))
    renderEntry('/spaces/space_room_1/m/msg_9')

    expect(await screen.findByText('Meldingen kunne ikke åpnes akkurat nå')).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Prøv igjen' })).toBeTruthy()
  })

  it('does not navigate when the page is left while the resolver is still answering', async () => {
    const pending = deferred<typeof ENTRY>()
    spacesClient.resolveSpaceEntry.mockReturnValue(pending.promise)
    const history = renderEntry('/spaces/space_room_1/m/msg_9')
    expect(await screen.findByText('Åpner meldingen…')).toBeTruthy()

    cleanup()
    pending.resolve(ENTRY)
    await new Promise((resolve) => setTimeout(resolve, 0))
    // The history itself is what a late navigate would rewrite, so it is the thing to check.
    expect(history.get()).toBe('/spaces/space_room_1/m/msg_9')
    expect(landed).toEqual([])
  })
})
