import { Route, Router } from '@solidjs/router'
import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import SpacePage from './SpacePage'

const spacesClient = vi.hoisted(() => ({
  getSpaceContext: vi.fn(),
  getSpaceThreads: vi.fn(),
  getPersonalSpaceDeletionReceipt: vi.fn(),
  listSpaces: vi.fn(),
  requestPersonalSpaceDeletion: vi.fn(),
}))

vi.mock('@/shared/api/spaces-client', () => ({
  getSpaceContext: spacesClient.getSpaceContext,
  getSpaceThreads: spacesClient.getSpaceThreads,
  getPersonalSpaceDeletionReceipt: spacesClient.getPersonalSpaceDeletionReceipt,
  listSpaces: spacesClient.listSpaces,
  requestPersonalSpaceDeletion: spacesClient.requestPersonalSpaceDeletion,
}))

vi.mock('@/features/chat/lib/chat-thread-history', () => ({
  selectChatThread: vi.fn(),
}))

const personalContext = {
  space: { space_ref: 'space_personal_1', name: 'Personal Space', kind: 'personal', lifecycle: 'active' },
  membership: {
    space_ref: 'space_personal_1', org_id: 'org_1', subject_id: 'user_1', kind: 'personal', role: 'owner',
    revisions: { authority: 1, membership: 1, privacy: 1, recipient_audience: 1, entitlement: 1 },
  },
}

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason?: unknown) => void
  const promise = new Promise<T>((promiseResolve, promiseReject) => {
    resolve = promiseResolve
    reject = promiseReject
  })
  return { promise, resolve, reject }
}

function renderSpacePage() {
  window.history.replaceState({}, '', '/spaces/space_personal_1')
  return render(() => (
    <Router root={(props) => <>{props.children}</>}>
      <Route path="/spaces/:spaceId" component={SpacePage} />
    </Router>
  ))
}

describe('SpacePage', () => {
  beforeEach(() => {
    spacesClient.getSpaceContext.mockReset()
    spacesClient.getSpaceThreads.mockReset()
    spacesClient.getPersonalSpaceDeletionReceipt.mockReset()
    spacesClient.listSpaces.mockReset()
    spacesClient.requestPersonalSpaceDeletion.mockReset()
    spacesClient.getSpaceThreads.mockResolvedValue({ ...personalContext, threads: [] })
    spacesClient.listSpaces.mockResolvedValue([personalContext.space])
  })

  afterEach(() => vi.restoreAllMocks())

  it('fails closed on the next membership recheck instead of keeping stale Space content visible', async () => {
    spacesClient.getSpaceContext
      .mockResolvedValueOnce(personalContext)
      .mockRejectedValueOnce(new Error('membership revoked'))
    let recheck: (() => void) | undefined
    vi.spyOn(window, 'setInterval').mockImplementation(((handler: TimerHandler, timeout?: number) => {
      if (timeout === 30_000) recheck = handler as () => void
      return 1 as unknown as number
    }) as typeof window.setInterval)

    renderSpacePage()
    expect(await screen.findByRole('heading', { name: 'Personal Space' })).toBeTruthy()

    expect(recheck).toBeTypeOf('function')
    recheck!()
    await waitFor(() => expect(spacesClient.getSpaceContext).toHaveBeenCalledTimes(2))

    expect(await screen.findByRole('heading', { name: 'Space unavailable' })).toBeTruthy()
    expect(screen.queryByRole('heading', { name: 'Personal Space' })).toBeNull()
    expect(screen.queryByRole('link', { name: 'Chat' })).toBeNull()
  })

  it('composes the room projection into the cockpit with a visible work pulse', async () => {
    spacesClient.getSpaceContext.mockResolvedValue(personalContext)
    spacesClient.getSpaceThreads.mockResolvedValue({
      ...personalContext,
      threads: [
        {
          thread_id: 'thread_running',
          space_id: 'space_personal_1',
          title: 'Prepare launch brief',
          preview: 'Collecting the latest release evidence.',
          latest_run_status: 'running',
          latest_run_updated_at: '2026-08-14T10:00:00Z',
        },
      ],
    })

    renderSpacePage()

    expect(await screen.findByRole('heading', { name: 'Personal Space' })).toBeTruthy()
    expect(screen.getByRole('tab', { name: 'Samtaler' })).toBeTruthy()
    expect(screen.getByRole('heading', { name: 'Room pulse' })).toBeTruthy()
    expect(screen.getByText('Verevon is working')).toBeTruthy()
    expect(screen.getByRole('link', { name: /Verevon is working.*Prepare launch brief/ }).getAttribute('href')).toBe(
      '/chat?thread_id=thread_running',
    )
    expect(screen.queryByRole('link', { name: 'Open Agent Studio' })).toBeNull()
  })

  it('keeps the Space composer link encoded without rendering a duplicate Space overview rail', async () => {
    spacesClient.getSpaceContext.mockResolvedValue({
      ...personalContext,
      space: { ...personalContext.space, space_ref: 'space / personal' },
      membership: { ...personalContext.membership, space_ref: 'space / personal' },
    })
    spacesClient.getSpaceThreads.mockResolvedValue({
      ...personalContext,
      space: { ...personalContext.space, space_ref: 'space / personal' },
      membership: { ...personalContext.membership, space_ref: 'space / personal' },
      threads: [
        {
          thread_id: 'thread / launch',
          space_id: 'space / personal',
          title: 'Launch plan',
          preview: 'Latest release preparation.',
          latest_run_status: 'running',
        },
        {
          thread_id: 'thread-retro',
          space_id: 'space / personal',
          title: 'Retro notes',
          preview: 'Capture the learnings.',
          latest_run_status: 'completed',
        },
      ],
    })

    window.history.replaceState({}, '', '/spaces/space%20%2F%20personal')
    render(() => (
      <Router root={(props) => <>{props.children}</>}>
        <Route path="/spaces/:spaceId" component={SpacePage} />
      </Router>
    ))

    expect(await screen.findByRole('link', { name: /^Launch plan.*Latest release preparation.*Working/ }).then((link) => link.getAttribute('href'))).toBe(
      '/chat?thread_id=thread%20%2F%20launch',
    )
    expect(screen.getByRole('link', { name: 'Message Personal Space' }).getAttribute('href')).toBe(
      '/chat?space_ref=space%20%2F%20personal',
    )
    expect(screen.getByRole('link', { name: /^Retro notes.*Capture the learnings.*Completed/ })).toBeTruthy()
    expect(screen.queryByRole('complementary', { name: 'Space overview' })).toBeNull()
  })

  it('gives a new Space a Grok-inspired Agent Studio entry point without duplicating Chat', async () => {
    spacesClient.getSpaceContext.mockResolvedValue(personalContext)
    spacesClient.getSpaceThreads.mockResolvedValue({ ...personalContext, threads: [] })
    renderSpacePage()

    expect(await screen.findByRole('heading', { name: 'Explore bots in Agent Studio' })).toBeTruthy()
    expect(screen.getByText('Personal room')).toBeTruthy()
    expect(screen.queryByText('Shared workroom')).toBeNull()
    expect(screen.queryByRole('link', { name: 'Chat' })).toBeNull()
    expect(screen.getByRole('link', { name: 'Start a conversation' })).toBeTruthy()
    expect(screen.getByRole('link', { name: 'Open Agent Studio' }).getAttribute('href')).toBe(
      '/agents?agent=chatbot&view=playground',
    )
    expect(screen.getByRole('link', { name: 'Message Personal Space' }).getAttribute('href')).toBe(
      '/chat?space_ref=space_personal_1',
    )

    fireEvent.click(screen.getByRole('tab', { name: 'Medlemmer' }))
    expect(screen.getByText(/People and bots connected to this Space will appear only when/)).toBeTruthy()
  })

  it('does not mistake a failed conversation projection for a fresh Space', async () => {
    spacesClient.getSpaceContext.mockResolvedValue(personalContext)
    spacesClient.getSpaceThreads.mockRejectedValue(new Error('projection unavailable'))
    renderSpacePage()

    expect(await screen.findByText(/Space conversation activity is temporarily unavailable/)).toBeTruthy()
    expect(screen.queryByRole('heading', { name: 'Start the conversation in Personal Space' })).toBeNull()
    expect(screen.getByRole('heading', { name: 'Conversation record unavailable' })).toBeTruthy()
    expect(screen.queryByRole('link', { name: 'Open Agent Studio' })).toBeNull()
  })

  it('keeps a newer thread projection available when an older request fails late', async () => {
    const staleThreads = deferred<Awaited<ReturnType<typeof spacesClient.getSpaceThreads>>>()
    spacesClient.getSpaceContext.mockResolvedValue(personalContext)
    spacesClient.getSpaceThreads
      .mockReturnValueOnce(staleThreads.promise)
      .mockResolvedValueOnce({ ...personalContext, threads: [] })
    let recheck: (() => void) | undefined
    vi.spyOn(window, 'setInterval').mockImplementation(((handler: TimerHandler, timeout?: number) => {
      if (timeout === 30_000) recheck = handler as () => void
      return 1 as unknown as number
    }) as typeof window.setInterval)

    renderSpacePage()
    expect(await screen.findByRole('heading', { name: 'Personal Space' })).toBeTruthy()
    recheck!()

    expect(await screen.findByRole('link', { name: 'Open Agent Studio' })).toBeTruthy()
    staleThreads.reject(new Error('stale projection failed'))

    await waitFor(() => expect(screen.queryByRole('heading', { name: 'Conversation record unavailable' })).toBeNull())
    expect(screen.getByRole('link', { name: 'Open Agent Studio' })).toBeTruthy()
  })
})
