// @vitest-environment jsdom

import { createRouter, memoryHistory } from '@solidjs/router'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { SpacesExpandedSidebarPanel } from './CoreSidebarSpacesPanel'
import { AnnouncerRegions, resetAnnouncerForTests } from '@/shared/a11y/announcer'
import { emptySidebarDoc } from '@/shared/actions/space-sidebar'
import { I18nProvider } from '@/shared/i18n'
import { ApiError } from '@/shared/api/http'
import { setRoomDirectoryListed } from '@/shared/api/spaces-client'
import { removeClientValuesWithPrefix } from '@/shared/session/client-storage'
import { clearSession, markSessionOnboardingComplete, setSessionUser } from '@/shared/session/session-store'
import { SPACE_SIDEBAR_DEBOUNCE_MS } from '@/features/spaces/lib/space-sidebar-store'
import { notifySpacesListChanged } from '@/features/spaces/lib/space-list-changed'
import { publishSpaceThreads, publishSpaceUnread, resetLiveWorkForTests, retractSpaceThreads } from '@/features/spaces/lib/space-live-work'

const spacesClient = vi.hoisted(() => ({
  getSpaceThreads: vi.fn(),
  listSpaces: vi.fn(),
  // WS12-S3: whether the listing said direct messages are on ("New message" button).
  spaceDmEnabled: vi.fn(),
}))

vi.mock('@/shared/api/spaces-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/spaces-client')>()),
  searchSpaceMessages: () => new Promise(() => {}),
  getSpaceThreads: spacesClient.getSpaceThreads,
  listSpaces: spacesClient.listSpaces,
  spaceDmEnabled: spacesClient.spaceDmEnabled,
}))

// WS16-S9: the personal layout is read and saved through this client; each test states the answer.
const sidebarClient = vi.hoisted(() => ({
  getSpaceSidebar: vi.fn(),
  putSpaceSidebar: vi.fn(),
}))
vi.mock('@/shared/api/spaces-sidebar-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/spaces-sidebar-client')>()),
  getSpaceSidebar: sidebarClient.getSpaceSidebar,
  putSpaceSidebar: sidebarClient.putSpaceSidebar,
}))

// DEPTH-GW1-1: the panel's attention reader starts the shared poller while it mounts, and that start
// writes a signal inside the component, which Solid 2 refuses (REACTIVE_WRITE_IN_OWNED_SCOPE) and
// then halts the reactive system for the rest of the file. These tests are about the room list, so
// the attention store is replaced by one that never answers (the sections that need it stay off).
// WS11-S4: the one exception is `attention.rooms`, which a test fills to say what the server counted
// for a room; it is empty in every other test, so those rows draw no badge.
const attention = vi.hoisted(() => ({ rooms: {} as Record<string, unknown> }))
vi.mock('@/features/spaces/lib/space-attention', () => ({
  retainSpaceAttention: () => () => undefined,
  refreshSpaceAttention: () => undefined,
  spaceAttentionRoom: (spaceRef: string) => attention.rooms[spaceRef],
  spaceAttentionTotals: () => undefined,
  spaceAttentionStale: () => false,
  spaceAttentionAvailable: () => false,
}))

const personalSpace = {
  space_ref: 'space personal',
  name: 'Personal Space',
  kind: 'personal',
  lifecycle: 'active',
}

function renderSpacesSidebar(path = '/spaces/space%20personal') {
  const TestRouter = createRouter({
    routes: [{ path: '/spaces/:spaceId', component: () => <SpacesExpandedSidebarPanel onCollapse={() => undefined} /> }],
    history: memoryHistory(path),
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
    attention.rooms = {}
    resetLiveWorkForTests()
    flush()
    spacesClient.spaceDmEnabled.mockReset().mockReturnValue(false)
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
    // inline timeline is the shared record's one reader. WS01-S7a: the link now names the
    // thread, so the room opens it in the side pane.
    expect(screen.getByRole('link', { name: /(?:Open|Åpne) Launch plan.*Prepare the release.*(?:Working|Jobber)/ }).getAttribute('href')).toBe(
      '/spaces/space%20personal?thread_id=thread%20%2F%20launch#chat',
    )
    expect(screen.getByRole('link', { name: /(?:Open|Åpne) Retro notes/ }).getAttribute('href')).toBe(
      '/spaces/space%20personal?thread_id=thread-retro#chat',
    )
    expect(screen.queryByRole('link', { name: /Åpne rommet|open room/i })).toBeNull()
  })

  it('draws the server count on a room with a count, and no badge on a room the answer does not name', async () => {
    attention.rooms = {
      space_shared: {
        spaceRef: 'space_shared',
        unreadThreads: 1,
        unreadCapped: false,
        mentions: 0,
        needsYou: 1,
        working: 0,
        lastActivityAt: null,
        lastReadAt: null,
        manualUnread: false,
        level: 'mentions',
        badge: { kind: 'count', n: 1 },
        gaps: [],
      },
    }
    renderSpacesSidebar()

    const shared = await screen.findByRole('link', { name: /Launch room/ })
    expect(shared.getAttribute('data-attention')).toBe('true')
    expect(within(shared).getByRole('img', { name: /1 (trenger deg|need you)/ }).textContent).toBe('1')
    expect(screen.getByRole('link', { name: 'Personlig rom' }).getAttribute('data-attention')).toBeNull()
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

  it('keeps an archived room listed last with a quiet Arkivert tag', async () => {
    spacesClient.listSpaces.mockResolvedValue([
      personalSpace,
      { space_ref: 'space_old', name: 'Old room', kind: 'room', lifecycle: 'active', archived: true },
      { space_ref: 'space_shared', name: 'Launch room', kind: 'room', lifecycle: 'active', archived: false },
    ])
    renderSpacesSidebar()

    const archived = await screen.findByRole('link', { name: /Old room/ })
    expect(archived.getAttribute('data-archived')).toBe('true')
    expect(archived.textContent).toContain('Arkivert')
    const live = screen.getByRole('link', { name: 'Launch room' })
    expect(live.getAttribute('data-archived')).toBeNull()
    // Archived sinks below the live room, whatever the server order.
    expect(live.compareDocumentPosition(archived) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
  })

  // DEPTH-WS04-S2-1: a rename saved on the room page (SpacePage's onSaved announces it) is read again here.
  it('reads the listing again when a room page announces a rename, and shows the new name', async () => {
    renderSpacesSidebar()
    expect(await screen.findByRole('link', { name: 'Launch room' })).toBeTruthy()
    const readsBefore = spacesClient.listSpaces.mock.calls.length
    spacesClient.listSpaces.mockResolvedValue([
      personalSpace,
      { space_ref: 'space_shared', name: 'Release room', kind: 'shared', lifecycle: 'active' },
    ])

    notifySpacesListChanged()

    expect(await screen.findByRole('link', { name: 'Release room' })).toBeTruthy()
    expect(screen.queryByRole('link', { name: 'Launch room' })).toBeNull()
    expect(spacesClient.listSpaces.mock.calls.length).toBeGreaterThan(readsBefore)
  })

  // WS12-S3: direct messages get their own group; the channels group never lists them.
  it('draws the DMs in a Direct messages group, keeps them out of Channels and skips a hidden one', async () => {
    spacesClient.listSpaces.mockResolvedValue([
      personalSpace,
      { space_ref: 'space_room', name: 'Launch room', kind: 'room', lifecycle: 'active' },
      { space_ref: 'space_dm', name: 'Direct message', kind: 'dm', lifecycle: 'active', participants: [{ subject_id: 'u-2', display_name: 'Annika' }] },
      { space_ref: 'space_gdm', name: 'Direct message', kind: 'group_dm', lifecycle: 'active', participants: [{ subject_id: 'u-3', display_name: 'Bjorn' }, { subject_id: 'u-4', display_name: 'Carla' }] },
      { space_ref: 'space_hidden', name: 'Direct message', kind: 'dm', lifecycle: 'active', hidden: true, participants: [{ subject_id: 'u-5', display_name: 'Hidden Hanna' }] },
    ])
    renderSpacesSidebar()

    expect(await screen.findByText(/^(?:Direktemeldinger|Direct messages)$/)).toBeTruthy()
    const dm = await screen.findByRole('link', { name: /Annika/ })
    expect(dm.getAttribute('href')).toBe('/spaces/space_dm')
    expect(screen.getByRole('link', { name: /Bjorn, Carla/ }).getAttribute('href')).toBe('/spaces/space_gdm')
    // Channels keeps the room and never lists a DM kind.
    expect(screen.getByRole('link', { name: 'Launch room' })).toBeTruthy()
    expect(document.querySelectorAll('a[href="/spaces/space_dm"]').length).toBe(1)
    expect(document.querySelectorAll('a[href="/spaces/space_gdm"]').length).toBe(1)
  })

  // DEPTH-GW1-3: a DM the caller hid matches the `dms` group no more (space-dm-group.ts) and the
  // channels group excludes the DM kinds, so buildSidebarModel's fallback puts it in Channels and
  // the "hidden" DM is drawn there. The plan: a hidden DM is not drawn until a new message brings it back.
  it.skip('does not draw a hidden DM anywhere (DEPTH-GW1-3: it falls through to Channels)', async () => {
    spacesClient.listSpaces.mockResolvedValue([
      personalSpace,
      { space_ref: 'space_hidden', name: 'Direct message', kind: 'dm', lifecycle: 'active', hidden: true, participants: [{ subject_id: 'u-5', display_name: 'Hidden Hanna' }] },
    ])
    renderSpacesSidebar()
    await screen.findByRole('link', { name: 'Personlig rom' })
    expect(screen.queryByRole('link', { name: /Hidden Hanna/ })).toBeNull()
  })

  it('draws no Direct messages group when the listing has no DM', async () => {
    renderSpacesSidebar()
    await screen.findByRole('link', { name: 'Personlig rom' })
    expect(screen.queryByText(/^(?:Direktemeldinger|Direct messages)$/)).toBeNull()
  })

  it('offers New message only when the listing said direct messages are on', async () => {
    spacesClient.spaceDmEnabled.mockReturnValue(false)
    const off = renderSpacesSidebar()
    await screen.findByRole('link', { name: 'Personlig rom' })
    expect(screen.queryByRole('button', { name: /Ny melding|New message/ })).toBeNull()
    off.unmount()
    cleanup()

    spacesClient.spaceDmEnabled.mockReturnValue(true)
    renderSpacesSidebar()
    await screen.findByRole('link', { name: 'Personlig rom' })
    expect(await screen.findByRole('button', { name: /Ny melding|New message/ })).toBeTruthy()
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

  // WS04-S8, WS11-S4 and WS16-S9: the signed-in panel. The layout is a real store on a stubbed sidebar
  // client, so each test states what the server answered and checks what the panel draws and saves.
  describe('with a personal sidebar layout and the room directory', () => {
    const SIDEBAR_CACHE_PREFIX = 'verevon.space-sidebar.v1:'
    const sharedRoom = { space_ref: 'space_shared', name: 'Launch room', kind: 'shared', lifecycle: 'active' }
    // A second channel room with a ref the sidebar document accepts (its refs take no spaces).
    const opsRoom = { space_ref: 'space_ops', name: 'Ops room', kind: 'room', lifecycle: 'active' }

    beforeEach(() => {
      setRoomDirectoryListed(false)
      removeClientValuesWithPrefix(SIDEBAR_CACHE_PREFIX)
      spacesClient.listSpaces.mockReset().mockResolvedValue([personalSpace, sharedRoom, opsRoom])
      sidebarClient.getSpaceSidebar.mockReset().mockReturnValue(new Promise(() => {}))
      sidebarClient.putSpaceSidebar.mockReset().mockImplementation(async (baseRevision: number, doc: unknown) => ({
        revision: baseRevision + 1,
        doc,
      }))
      resetAnnouncerForTests()
      HTMLDialogElement.prototype.showModal = function showModal() { this.setAttribute('open', '') }
      HTMLDialogElement.prototype.close = function close() { this.removeAttribute('open') }
      // The org is set only once the user is visible to the next read: Solid 2 applies store writes on flush.
      setSessionUser({ id: 'user_1', email: 'user@example.com', name: 'User', emailVerified: true })
      flush()
      markSessionOnboardingComplete({ id: 'org_1', name: 'Acme AS', role: 'owner' })
      flush()
    })

    afterEach(() => {
      cleanup()
      clearSession()
      setRoomDirectoryListed(false)
      removeClientValuesWithPrefix(SIDEBAR_CACHE_PREFIX)
      vi.unstubAllGlobals()
    })

    /** A stored layout with one custom section, `proj`, holding `refs` in manual order. */
    function withSection(refs: readonly string[] = []) {
      return {
        ...emptySidebarDoc(),
        sections: [{ id: 'proj', name: 'Prosjekter', collapsed: false, sort: 'manual' as const, refs: [...refs] }],
      }
    }

    /** The real listing call, with the gateway's answer stubbed: its `features` set the room-directory flag. */
    async function stubListing(features: { room_directory: boolean }) {
      const actual = await vi.importActual<typeof import('@/shared/api/spaces-client')>('@/shared/api/spaces-client')
      spacesClient.listSpaces.mockImplementation(actual.listSpaces)
      vi.stubGlobal('fetch', vi.fn(async () => new Response(
        JSON.stringify({ spaces: [personalSpace, sharedRoom], features }),
        { status: 200, headers: { 'Content-Type': 'application/json' } },
      )))
    }

    /** Resolves once the layout has loaded: rows are draggable and their menus are drawn. */
    async function waitForEditable() {
      await waitFor(() => expect(document.querySelector('[data-room-ref][draggable="true"]')).not.toBeNull())
    }

    /** Renders with a stored layout and the announcer, once the layout has loaded. */
    async function renderEditableSidebar(doc = emptySidebarDoc(), path?: string) {
      sidebarClient.getSpaceSidebar.mockResolvedValue({ mode: 'server', revision: 1, doc })
      const view = renderSpacesSidebar(path)
      render(() => <AnnouncerRegions />)
      await waitForEditable()
      return view
    }

    function sectionEl(id: string) {
      return document.querySelector<HTMLElement>(`section[data-section-id="${id}"]`)
    }

    /** The section with this id, once it is drawn. */
    function awaitSection(id: string) {
      return waitFor(() => {
        const section = sectionEl(id)
        expect(section).not.toBeNull()
        return section as HTMLElement
      })
    }

    function roomRow(ref: string) {
      return document.querySelector<HTMLElement>(`[data-room-ref="${ref}"]`)
    }

    /** The room refs drawn inside a section, top to bottom. */
    function roomOrder(section: HTMLElement | null) {
      return Array.from(section?.querySelectorAll<HTMLElement>('[data-room-ref]') ?? []).map((row) => row.dataset.roomRef)
    }

    /** The `more` menu of a room or a section, found by the label on its summary (the menu list repeats that label). */
    function menuOf(label: string) {
      return document.querySelector<HTMLElement>(`summary[aria-label="${label}"]`)?.closest('details') as HTMLElement
    }

    /** The `more` menu of one room's row. A group named like a room shares that room's menu label, so rows are used. */
    function rowMenu(ref: string) {
      return roomRow(ref)?.querySelector<HTMLElement>('details') as HTMLElement
    }

    /** A room link drawn inside a section, once it is drawn: a section can be drawn before its rooms. */
    function awaitLinkIn(sectionId: string, name: string) {
      return waitFor(() => within(sectionEl(sectionId) as HTMLElement).getByRole('link', { name }))
    }

    /** One item of a menu. A closed menu keeps its items in the DOM, so they are read with `hidden`. */
    function menuItem(label: string, name: RegExp) {
      return within(menuOf(label)).getByRole('menuitem', { name, hidden: true })
    }

    /** What the polite live regions say right now. */
    function politeText() {
      return Array.from(document.querySelectorAll('[data-announcer-region^="polite"]')).map((node) => node.textContent).join(' ')
    }

    /** A stand-in for the browser's DataTransfer: what a drag stores is what its drop reads back. */
    function dragTransfer() {
      const entries = new Map<string, string>()
      return {
        get types() { return [...entries.keys()] },
        effectAllowed: 'none',
        dropEffect: 'none',
        setData: (type: string, value: string) => entries.set(type, value),
        getData: (type: string) => entries.get(type) ?? '',
      }
    }

    /** One room's attention answer. The fields the panel does not read keep neutral values. */
    function attentionRoom(overrides: Record<string, unknown>) {
      return {
        unreadThreads: 0,
        unreadCapped: false,
        mentions: 0,
        needsYou: 0,
        working: 0,
        lastActivityAt: null,
        lastReadAt: null,
        manualUnread: false,
        level: 'mentions',
        badge: { kind: 'none', n: 0 },
        gaps: [],
        ...overrides,
      }
    }

    // WS04-S8: the Browse rooms link follows the listing's features.room_directory.
    it('offers Browse rooms only when the listing says room_directory', async () => {
      await stubListing({ room_directory: false })
      const off = renderSpacesSidebar()
      await screen.findByRole('link', { name: 'Personlig rom' })
      expect(screen.queryByRole('link', { name: /Finn rom|Browse rooms/ })).toBeNull()
      off.unmount()
      cleanup()

      await stubListing({ room_directory: true })
      renderSpacesSidebar()
      expect((await screen.findByRole('link', { name: /Finn rom|Browse rooms/ })).getAttribute('href')).toBe('/spaces/directory')
    })

    // WS04-S8: /spaces/directory is the directory page; it is never loaded as a Space.
    it('reads /spaces/directory as the room directory page, not as a Space to load', async () => {
      await stubListing({ room_directory: true })
      renderSpacesSidebar('/spaces/directory')
      const directory = await screen.findByRole('link', { name: /Finn rom|Browse rooms/ })
      expect(directory.className).toContain('core-sidebar-panel-link--active')
      await waitFor(() => expect(spacesClient.getSpaceThreads).toHaveBeenCalledWith('space personal'))
      expect(spacesClient.getSpaceThreads).not.toHaveBeenCalledWith('directory')
    })

    // WS11-S4: the server's badge is drawn for a room this browser has not got open, never for the open one.
    it('draws the server badge only on a room this browser has not got open', async () => {
      attention.rooms = {
        space_shared: attentionRoom({ needsYou: 2, mentions: 2, badge: { kind: 'count', n: 2 } }),
        'space personal': attentionRoom({ needsYou: 1, mentions: 1, badge: { kind: 'count', n: 1 } }),
      }
      publishSpaceThreads('space_shared', [{ thread_id: 't-open', space_id: 'space_shared', latest_run_status: 'completed' }])
      renderSpacesSidebar('/spaces/space_shared')

      const open = await screen.findByRole('link', { name: 'Launch room' })
      expect(open.getAttribute('data-attention')).toBeNull()
      expect(open.querySelector('.space-attention-badge')).toBeNull()
      const unopened = screen.getByRole('link', { name: /Personlig rom/ })
      expect(unopened.getAttribute('data-attention')).toBe('true')
      expect(within(unopened).getByRole('img', { name: /1 (trenger deg|need you)/ }).textContent).toBe('1')
    })

    // WS11-S4: the level the server last reported is the checked item; a room the answer does not name gets no level or mute item.
    it('marks the notify level with a checkmark, and draws no level or mute item for a room the answer does not name', async () => {
      attention.rooms = { space_shared: attentionRoom({ level: 'mentions' }) }
      await renderEditableSidebar()

      const shared = within(rowMenu('space_shared'))
      expect(shared.getByRole('menuitemcheckbox', { name: /Varsler: alle meldinger/, hidden: true }).getAttribute('aria-checked')).toBe('false')
      const mentions = shared.getByRole('menuitemcheckbox', { name: /Varsler: bare omtaler/, hidden: true })
      expect(mentions.getAttribute('aria-checked')).toBe('true')
      expect(mentions.textContent).toBe('✓ Varsler: bare omtaler')
      expect(shared.getByRole('menuitemcheckbox', { name: /Demp rom/, hidden: true }).getAttribute('aria-checked')).toBe('false')

      const personal = within(rowMenu('space personal'))
      expect(personal.queryAllByRole('menuitemcheckbox', { hidden: true })).toHaveLength(0)
      expect(personal.getAllByRole('menuitem', { hidden: true }).map((item) => item.textContent)).toEqual([
        'Stjernemerk',
        'Flytt til gruppe …',
        'Skjul',
      ])
    })

    // WS16-S9: Starred, then the custom sections, then the built-in groups; a group with no room is not drawn.
    it('draws the grouped layout: Starred, then the built-in groups in their order', async () => {
      await renderEditableSidebar({ ...emptySidebarDoc(), starred: ['space_ops'] })

      const starred = await awaitSection('starred')
      expect(within(starred).getByText('Stjernemerkede')).toBeTruthy()
      expect(within(starred).getByRole('link', { name: 'Ops room' })).toBeTruthy()
      expect(within(await awaitSection('group-channels')).getByRole('link', { name: 'Launch room' })).toBeTruthy()
      expect(within(await awaitSection('group-personal')).getByRole('link', { name: 'Personlig rom' })).toBeTruthy()
      expect(Array.from(document.querySelectorAll('section[data-section-id]')).map((node) => node.getAttribute('data-section-id')))
        .toEqual(['starred', 'group-channels', 'group-personal'])
    })

    it('stars a room from its menu, moves it into Starred and saves the layout', async () => {
      await renderEditableSidebar()
      fireEvent.click(menuItem('Valg for Launch room', /^Stjernemerk$/))
      flush()

      expect(await awaitLinkIn('starred', 'Launch room')).toBeTruthy()
      await waitFor(() => expect(politeText()).toContain('Launch room flyttet til Stjernemerkede'))
      await waitFor(() => expect(sidebarClient.putSpaceSidebar).toHaveBeenLastCalledWith(1, expect.objectContaining({ starred: ['space_shared'] })))
    })

    it('hides a room from its menu, and brings it back with Show hidden rooms', async () => {
      await renderEditableSidebar()
      fireEvent.click(menuItem('Valg for Launch room', /^Skjul$/))
      flush()

      expect(await waitFor(() => screen.getByRole('button', { name: /Vis skjulte rom \(1\)/ }))).toBeTruthy()
      expect(within(await awaitSection('group-channels')).queryByRole('link', { name: 'Launch room' })).toBeNull()
      fireEvent.click(screen.getByRole('button', { name: /Vis skjulte rom \(1\)/ }))
      flush()

      expect(await awaitLinkIn('hidden', 'Launch room')).toBeTruthy()
      expect(screen.getByRole('button', { name: /Skjul skjulte rom \(1\)/ })).toBeTruthy()
      await waitFor(() => expect(sidebarClient.putSpaceSidebar).toHaveBeenLastCalledWith(1, expect.objectContaining({ hidden: ['space_shared'] })))
    })

    it('drags a room onto a section and saves it there', async () => {
      await renderEditableSidebar(withSection())
      const transfer = dragTransfer()
      fireEvent.dragStart(roomRow('space_shared')!, { dataTransfer: transfer })
      flush()

      const section = await awaitSection('section-proj')
      fireEvent.dragOver(section, { dataTransfer: transfer })
      fireEvent.drop(section, { dataTransfer: transfer })
      flush()

      // The section is drawn again on the save, so the room is read from the section as it is now.
      expect(await awaitLinkIn('section-proj', 'Launch room')).toBeTruthy()
      expect(within(await awaitSection('group-channels')).queryByRole('link', { name: 'Launch room' })).toBeNull()
      await waitFor(() => expect(sidebarClient.putSpaceSidebar).toHaveBeenLastCalledWith(1, expect.objectContaining({
        sections: [expect.objectContaining({ id: 'proj', refs: ['space_shared'] })],
      })))
    })

    it('moves a room to a section from the Move to section dialog', async () => {
      await renderEditableSidebar(withSection())
      fireEvent.click(menuItem('Valg for Launch room', /^Flytt til gruppe/))
      flush()

      const dialog = await waitFor(() => {
        const open = document.querySelector<HTMLDialogElement>('dialog[aria-labelledby="space-move-dialog-title"]')
        expect(open?.hasAttribute('open')).toBe(true)
        return open as HTMLDialogElement
      })
      fireEvent.change(within(dialog).getByRole('combobox'), { target: { value: 'proj' } })
      // The choice is a signal write, applied on the next flush: the submit must read it.
      flush()
      fireEvent.click(within(dialog).getByRole('button', { name: 'Flytt' }))
      flush()

      await waitFor(() => expect(dialog.hasAttribute('open')).toBe(false))
      expect(await awaitLinkIn('section-proj', 'Launch room')).toBeTruthy()
      await waitFor(() => expect(sidebarClient.putSpaceSidebar).toHaveBeenLastCalledWith(1, expect.objectContaining({
        sections: [expect.objectContaining({ id: 'proj', refs: ['space_shared'] })],
      })))
    })

    // WS16-S9: Alt+Arrow moves a room in Starred or a manual section, says its new place and saves the order.
    it('moves a starred room up with Alt+ArrowUp and says its new place', async () => {
      await renderEditableSidebar({ ...emptySidebarDoc(), starred: ['space_shared', 'space_ops'] })
      fireEvent.keyDown(within(roomRow('space_ops')!).getByRole('link', { name: 'Ops room' }), { key: 'ArrowUp', altKey: true })
      flush()

      await waitFor(() => expect(roomOrder(sectionEl('starred'))).toEqual(['space_ops', 'space_shared']))
      await waitFor(() => expect(politeText()).toContain('Ops room: plass 1 av 2'))
      await waitFor(() => expect(sidebarClient.putSpaceSidebar).toHaveBeenLastCalledWith(1, expect.objectContaining({
        starred: ['space_ops', 'space_shared'],
      })))
    })

    it('moves a room down inside a manual section with Alt+ArrowDown and says its new place', async () => {
      await renderEditableSidebar(withSection(['space_shared', 'space_ops']))
      fireEvent.keyDown(within(roomRow('space_shared')!).getByRole('link', { name: 'Launch room' }), { key: 'ArrowDown', altKey: true })
      flush()

      await waitFor(() => expect(roomOrder(sectionEl('section-proj'))).toEqual(['space_ops', 'space_shared']))
      await waitFor(() => expect(politeText()).toContain('Launch room: plass 2 av 2'))
      await waitFor(() => expect(sidebarClient.putSpaceSidebar).toHaveBeenLastCalledWith(1, expect.objectContaining({
        sections: [expect.objectContaining({ refs: ['space_ops', 'space_shared'] })],
      })))
    })

    // WS16-S9: a search reaches hidden rooms too, and the Show hidden toggle waits until the search is cleared.
    it('includes a hidden room in a search, and offers no Show hidden toggle while searching', async () => {
      await renderEditableSidebar({ ...emptySidebarDoc(), hidden: ['space_shared'] })
      expect(screen.getByRole('button', { name: /Vis skjulte rom \(1\)/ })).toBeTruthy()

      fireEvent.input(screen.getByRole('textbox', { name: /søk i rom og samtaler|search rooms and conversations/i }), { target: { value: 'launch' } })
      flush()

      expect(await awaitLinkIn('hidden', 'Launch room')).toBeTruthy()
      expect(document.querySelector('.space-sidebar-hidden-toggle')).toBeNull()
    })

    // WS16-S9: a 404 from the gateway (the layout is off) selects the legacy panel: channels and personal rooms, no editing.
    it('falls back to the legacy panel when the layout answers 404', async () => {
      sidebarClient.getSpaceSidebar.mockRejectedValue(new ApiError('Not found', 404, 'space_sidebar_disabled'))
      renderSpacesSidebar()

      await waitFor(() => {
        expect(document.querySelector('.space-sidebar-section')).toBeNull()
        expect(within(screen.getByText('Kanaler').closest('section') as HTMLElement).getByRole('link', { name: 'Launch room' })).toBeTruthy()
      })
      expect(document.querySelector('[data-room-ref]')).toBeNull()
      expect(screen.queryByText('Stjernemerkede')).toBeNull()
      expect(screen.queryByRole('button', { name: /Ny gruppe/ })).toBeNull()
      expect(screen.queryByText('Lagret bare i denne nettleseren.')).toBeNull()
    })

    // WS16-S9: under Zero Data Retention the layout stays in this browser: the note says so, and nothing is sent.
    it('says the layout is saved in this browser only under Zero Data Retention, and never sends it', async () => {
      sidebarClient.getSpaceSidebar.mockResolvedValue({ mode: 'zdr_local', revision: 0, doc: null })
      renderSpacesSidebar()
      await waitForEditable()
      expect(screen.getByText('Lagret bare i denne nettleseren.')).toBeTruthy()

      fireEvent.click(menuItem('Valg for Launch room', /^Stjernemerk$/))
      flush()
      expect(await awaitLinkIn('starred', 'Launch room')).toBeTruthy()
      await new Promise((resolve) => setTimeout(resolve, SPACE_SIDEBAR_DEBOUNCE_MS + 100))
      expect(sidebarClient.putSpaceSidebar).not.toHaveBeenCalled()
    })
  })

})
