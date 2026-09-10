// @vitest-environment jsdom

import { createRouter, memoryHistory } from '@solidjs/router'
import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { SpacesExpandedSidebarPanel } from './CoreSidebarSpacesPanel'
import { I18nProvider } from '@/shared/i18n'
import { publishSpaceThreads, publishSpaceUnread, resetLiveWorkForTests, retractSpaceThreads } from '@/features/spaces/lib/space-live-work'

const spacesClient = vi.hoisted(() => ({
  getSpaceThreads: vi.fn(),
  listSpaces: vi.fn(),
}))

vi.mock('@/shared/api/spaces-client', () => ({
  getSpaceThreads: spacesClient.getSpaceThreads,
  listSpaces: spacesClient.listSpaces,
}))

const personalSpace = {
  space_ref: 'space personal',
  name: 'Personal Space',
  kind: 'personal',
  lifecycle: 'active',
}

function renderSpacesSidebar() {
  const TestRouter = createRouter({
    routes: [{ path: '/spaces/:spaceId', component: () => <SpacesExpandedSidebarPanel onCollapse={() => undefined} /> }],
    history: memoryHistory('/spaces/space%20personal'),
    explicitLinks: true,
  })
  return render(() => (
    <I18nProvider>
      <TestRouter>{(props) => <>{props.children}</>}</TestRouter>
    </I18nProvider>
  ))
}

describe('SpacesExpandedSidebarPanel', () => {
  beforeEach(() => {
    resetLiveWorkForTests()
    flush()
    spacesClient.listSpaces.mockReset().mockResolvedValue([
      personalSpace,
      { space_ref: 'space_shared', name: 'Launch room', kind: 'shared', lifecycle: 'active' },
    ])
    spacesClient.getSpaceThreads.mockReset().mockResolvedValue({
      space: personalSpace,
      membership: {
        space_ref: personalSpace.space_ref,
        org_id: 'org_1',
        subject_id: 'user_1',
        kind: 'personal',
        role: 'owner',
        revisions: { authority: 1, membership: 1, privacy: 1, recipient_audience: 1, entitlement: 1 },
      },
      threads: [
        { thread_id: 'thread / launch', space_id: personalSpace.space_ref, title: 'Launch plan', preview: 'Prepare the release.', latest_run_status: 'running' },
        { thread_id: 'thread-retro', space_id: personalSpace.space_ref, title: 'Retro notes', preview: 'Capture learnings.', latest_run_status: 'completed' },
      ],
    })
  })

  afterEach(cleanup)

  it('replaces the generic open-room item with a localized personal room and its real conversation projection', async () => {
    renderSpacesSidebar()

    expect(await screen.findByRole('link', { name: 'Personlig rom' }).then((link) => link.getAttribute('href'))).toBe(
      '/spaces/space%20personal',
    )
    expect(screen.getByText(/channels|kanaler/i)).toBeTruthy()
    expect(screen.getByRole('link', { name: 'Launch room' }).getAttribute('href')).toBe('/spaces/space_shared')
    expect(screen.getByText(/all conversations|alle samtaler/i)).toBeTruthy()
    // A Space conversation opens in its ROOM, never in /chat — the room's
    // inline timeline is the shared record's one reader.
    expect(screen.getByRole('link', { name: /(?:Open|Åpne) Launch plan.*Prepare the release.*(?:Working|Jobber)/ }).getAttribute('href')).toBe(
      '/spaces/space%20personal',
    )
    expect(screen.queryByRole('link', { name: /Åpne rommet|open room/i })).toBeNull()
  })

  it('filters the personal room and conversation projection with the core sidebar search', async () => {
    renderSpacesSidebar()
    await screen.findByRole('link', { name: 'Personlig rom' })

    fireEvent.input(screen.getByRole('textbox', { name: /search rooms and conversations|søk i rom og samtaler/i }), { target: { value: 'retro' } })
    // Solid 2 commits DOM work on a queued microtask instead of synchronously
    // inside the event, so drain the queue before reading the filtered DOM.
    flush()

    expect(screen.queryByRole('link', { name: 'Personlig rom' })).toBeNull()
    expect(screen.queryByRole('link', { name: /^(?:Open|Åpne) Launch plan/ })).toBeNull()
    expect(screen.getByRole('link', { name: /^(?:Open|Åpne) Retro notes/ })).toBeTruthy()
  })

  it('keeps the room visible and reports a failed conversation projection instead of showing an empty state', async () => {
    spacesClient.getSpaceThreads.mockRejectedValue(new Error('projection unavailable'))
    renderSpacesSidebar()

    expect(await screen.findByRole('link', { name: 'Personlig rom' })).toBeTruthy()
    expect((await screen.findByRole('alert')).textContent).toMatch(/conversations could not be loaded|samtaler kunne ikke lastes/i)
    expect(screen.queryByText(/no conversations in this space yet|ingen samtaler i dette rommet ennå/i)).toBeNull()
  })

  it('localizes an untitled conversation rather than showing an English fallback in the Norwegian sidebar', async () => {
    spacesClient.getSpaceThreads.mockResolvedValueOnce({
      space: personalSpace,
      membership: {
        space_ref: personalSpace.space_ref,
        org_id: 'org_1',
        subject_id: 'user_1',
        kind: 'personal',
        role: 'owner',
        revisions: { authority: 1, membership: 1, privacy: 1, recipient_audience: 1, entitlement: 1 },
      },
      threads: [{ thread_id: 'thread-untitled', space_id: personalSpace.space_ref }],
    })
    renderSpacesSidebar()

    expect(await screen.findByRole('link', { name: /Åpne samtale uten tittel/i })).toBeTruthy()
  })

  // Item 1b: Slack's channel dot, for agent work. The sidebar fetches nothing
  // extra for it — the Space page publishes the projection it already polls.
  it('shows a working dot on a room whose published projection has a live run', async () => {
    renderSpacesSidebar()
    await screen.findByText('Launch room')
    expect(screen.queryByRole('img', { name: 'Agentarbeid pågår' })).toBeNull()

    publishSpaceThreads('space_shared', [
      { thread_id: 't-live', space_id: 'space_shared', latest_run_status: 'running' },
    ])
    flush()

    const dot = await screen.findByRole('img', { name: 'Agentarbeid pågår' })
    expect(dot.closest('a')?.textContent).toContain('Launch room')
    expect(dot.closest('a')?.getAttribute('data-working')).toBe('true')

    // The page left the room; a projection nobody refreshes must not keep the
    // dot lit.
    retractSpaceThreads('space_shared')
    flush()
    await waitFor(() => expect(screen.queryByRole('img', { name: 'Agentarbeid pågår' })).toBeNull())
  })

  // Absence is not idleness. The mounted thread list says this room has a
  // running thread, but no page has published a live projection for it — so
  // the sidebar must not draw a dot it cannot vouch for, in either direction.
  it('draws no dot for a room it has not observed, however stale its mounted copy is', async () => {
    renderSpacesSidebar()
    await screen.findByText('Launch plan')
    expect(screen.queryByRole('img', { name: 'Agentarbeid pågår' })).toBeNull()
    expect(document.querySelector('.core-sidebar-panel-link[data-working]')).toBeNull()
  })

  // The thread row's "Jobber" label was true at mount and stale forever
  // after. A fresher published projection wins for the live-ness question.
  it('lets a published projection correct a thread row’s stale status', async () => {
    renderSpacesSidebar()
    const row = await screen.findByRole('link', { name: /Launch plan/ })
    expect(row.getAttribute('aria-label')).toContain('Jobber')

    // The run finished; the page's poll says so.
    publishSpaceThreads('space personal', [
      { thread_id: 'thread / launch', space_id: 'space personal', title: 'Launch plan', latest_run_status: 'completed' },
    ])
    flush()
    await waitFor(() => {
      const fresh = screen.getByRole('link', { name: /Launch plan/ })
      expect(fresh.getAttribute('aria-label')).toContain('Fullført')
      expect(fresh.getAttribute('aria-label')).not.toContain('Jobber')
    })
  })


  // Item 4b: a pinned thread swaps its glyph and says so in its label, so the
  // channel list reads like a channel list.
  it('marks a pinned thread row', async () => {
    spacesClient.getSpaceThreads.mockReset().mockResolvedValue({
      space: personalSpace,
      membership: { space_ref: personalSpace.space_ref, org_id: 'org_1', subject_id: 'user_1', kind: 'personal', role: 'owner',
        revisions: { authority: 1, membership: 1, privacy: 1, recipient_audience: 1, entitlement: 1 } },
      threads: [
        { thread_id: 'pinned', space_id: personalSpace.space_ref, title: 'Rutiner', pinned: true, latest_run_status: 'completed' },
        { thread_id: 'plain', space_id: personalSpace.space_ref, title: 'Løst', latest_run_status: 'completed' },
      ],
    })
    renderSpacesSidebar()
    const row = await screen.findByRole('link', { name: /Rutiner/ })
    expect(row.getAttribute('data-pinned')).toBe('true')
    expect(row.getAttribute('aria-label')).toContain('Festet')
    expect(row.querySelector('.core-sidebar-panel-link__icon--pinned')).toBeTruthy()
    const plain = screen.getByRole('link', { name: /Løst/ })
    expect(plain.getAttribute('data-pinned')).toBeNull()
    expect(plain.getAttribute('aria-label')).not.toContain('Festet')
  })

  // New-since-arrival comes from the page's published derivation, so the
  // sidebar badges exactly the rows the timeline does.
  it('badges a thread the page says is new since arrival, and lets a working dot win', async () => {
    renderSpacesSidebar()
    const retro = await screen.findByRole('link', { name: /Retro notes/ })
    expect(retro.getAttribute('data-unread')).toBeNull()

    publishSpaceThreads('space personal', [
      { thread_id: 'thread-retro', space_id: 'space personal', latest_run_status: 'completed' },
      { thread_id: 'thread / launch', space_id: 'space personal', latest_run_status: 'running' },
    ])
    publishSpaceUnread('space personal', new Set(['thread-retro', 'thread / launch']))
    flush()

    await waitFor(() => {
      const fresh = screen.getByRole('link', { name: /Retro notes/ })
      expect(fresh.getAttribute('data-unread')).toBe('true')
      expect(fresh.getAttribute('aria-label')).toContain('Ny siden sist')
      expect(fresh.querySelector('.core-sidebar-unread-dot')).toBeTruthy()
    })
    // The launch thread is both new and working; work in progress is the more
    // urgent fact, so only the working dot shows.
    const launch = screen.getByRole('link', { name: /Launch plan/ })
    expect(launch.querySelector('.core-sidebar-working-dot')).toBeTruthy()
    expect(launch.querySelector('.core-sidebar-unread-dot')).toBeNull()
  })

})
