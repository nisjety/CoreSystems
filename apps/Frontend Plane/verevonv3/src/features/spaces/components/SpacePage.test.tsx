import { Route, Router } from '@solidjs/router'
import { render, screen, waitFor } from '@solidjs/testing-library'
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
  })
})
