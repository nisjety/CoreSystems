// @vitest-environment jsdom

import { Route, Router } from '@solidjs/router'
import { cleanup, fireEvent, render, screen } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { SpacesExpandedSidebarPanel } from './CoreSidebarSpacesPanel'
import { I18nProvider } from '@/shared/i18n'

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
  window.history.replaceState({}, '', '/spaces/space%20personal')
  return render(() => (
    <I18nProvider>
      <Router root={(props) => <>{props.children}</>}>
        <Route path="/spaces/:spaceId" component={() => <SpacesExpandedSidebarPanel onCollapse={() => undefined} />} />
      </Router>
    </I18nProvider>
  ))
}

describe('SpacesExpandedSidebarPanel', () => {
  beforeEach(() => {
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

    expect(await screen.findByRole('link', { name: 'Personal Space' }).then((link) => link.getAttribute('href'))).toBe(
      '/spaces/space%20personal',
    )
    expect(screen.getByText(/other spaces|andre rom/i)).toBeTruthy()
    expect(screen.getByRole('link', { name: 'Launch room' }).getAttribute('href')).toBe('/spaces/space_shared')
    expect(screen.getByText(/all conversations|alle samtaler/i)).toBeTruthy()
    expect(screen.getByRole('link', { name: /(?:Open|Åpne) Launch plan.*Prepare the release.*(?:Working|Jobber)/ }).getAttribute('href')).toBe(
      '/chat?thread_id=thread%20%2F%20launch',
    )
    expect(screen.queryByRole('link', { name: /Åpne rommet|open room/i })).toBeNull()
  })

  it('filters the personal room and conversation projection with the core sidebar search', async () => {
    renderSpacesSidebar()
    await screen.findByRole('link', { name: 'Personal Space' })

    fireEvent.input(screen.getByRole('textbox', { name: /search rooms and conversations|søk i rom og samtaler/i }), { target: { value: 'retro' } })

    expect(screen.queryByRole('link', { name: 'Personal Space' })).toBeNull()
    expect(screen.queryByRole('link', { name: /^(?:Open|Åpne) Launch plan/ })).toBeNull()
    expect(screen.getByRole('link', { name: /^(?:Open|Åpne) Retro notes/ })).toBeTruthy()
  })

  it('keeps the room visible and reports a failed conversation projection instead of showing an empty state', async () => {
    spacesClient.getSpaceThreads.mockRejectedValue(new Error('projection unavailable'))
    renderSpacesSidebar()

    expect(await screen.findByRole('link', { name: 'Personal Space' })).toBeTruthy()
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
})
