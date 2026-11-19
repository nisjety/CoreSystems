import { cleanup, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { clearSpaceRoomFeatures, publishSpaceRoomFeatures } from '../lib/space-room-features'

// The room timeline with its windowed store (WS15-S8) and the parts of WS15-S1 and WS15-S11 that a reader sees
// between the posts: the lazy transcript read (a post's tail is read only when it comes near the viewport), "Load
// earlier", the pinned shelf, the earlier-turns row, the system rows and the scroll-anchoring attributes. The
// network reads are mocked; the store is the real one. IntersectionObserver is a fake the test drives by hand.
// No I18nProvider wraps these renders, so the copy is the Norwegian default.

const spacesClient = vi.hoisted(() => ({
  getSpaceTimeline: vi.fn(),
  getSpaceThreadTranscript: vi.fn(),
  loadSpaceTranscript: vi.fn(),
  searchSpaceMessages: vi.fn(() => new Promise(() => {})),
  updateSpaceThreadPresentation: vi.fn(),
}))
vi.mock('@/shared/api/spaces-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/spaces-client')>()),
  ...spacesClient,
}))
vi.mock('@/features/chat/components/ChatMessages', () => ({
  ChatMarkdown: (props: { content: string }) => <p>{props.content}</p>,
}))
vi.mock('./SpaceApprovalPanel', () => ({
  SpaceApprovalPanel: () => <div data-testid="approval-panel" />,
}))

const { SpaceRoomTimeline } = await import('./SpaceRoomTimeline')
const { createSpaceTimelineStore } = await import('../lib/space-timeline-store')

type Observed = { readonly target: Element; readonly isIntersecting: boolean }

/** The observer the room's visibility watcher creates. The test reports what came near. */
class FakeIntersectionObserver {
  static last: FakeIntersectionObserver | undefined
  readonly targets = new Set<Element>()
  private readonly listener: (entries: Observed[]) => void
  constructor(listener: (entries: Observed[]) => void) {
    this.listener = listener
    FakeIntersectionObserver.last = this
  }
  observe(target: Element): void {
    this.targets.add(target)
  }
  unobserve(target: Element): void {
    this.targets.delete(target)
  }
  disconnect(): void {
    this.targets.clear()
  }
  report(target: Element, isIntersecting: boolean): void {
    this.listener([{ target, isIntersecting }])
  }
}

/** Reports that a post came near (or went away) the viewport. The store's writes apply at the next flush. */
function nearPost(threadId: string, isNear = true): void {
  const post = document.getElementById(`post-${threadId}`)
  if (!post) throw new Error(`no post ${threadId}`)
  FakeIntersectionObserver.last?.report(post, isNear)
  flush()
}

/** Waits until the check passes. The store's reads land outside Solid's event flush, so each try flushes first. */
async function eventually(check: () => void): Promise<void> {
  await waitFor(() => {
    flush()
    check()
  })
}

const roster = [
  { subject_type: 'user', subject_id: 'u-kari', role: 'owner', revision: 1, display_name: 'Kari Nordmann' },
  { subject_type: 'user', subject_id: 'u-ola', role: 'editor', revision: 1, display_name: 'Ola Hansen' },
]
const agent = { agent_ref: 'a1', subject_id: 'agent-1', name: 'Lagerhjelpen', status: 'active' }

/** A post, ordered by its minute of the day (the store and the room both read the sequence and the time). */
const t = (id: string, minute: number, extra: Record<string, unknown> = {}) => ({
  thread_id: id,
  space_id: 'room-1',
  title: `Sak ${id}`,
  owner_subject_id: 'u-ola',
  latest_run_status: 'completed',
  updated_at: new Date(Date.UTC(2026, 10, 18, 9, minute)).toISOString(),
  last_message_seq: minute,
  ...extra,
})

const iso = (minute: number) => new Date(Date.UTC(2026, 10, 18, 9, minute)).toISOString()
/** A room change as the store holds it after the timeline client has read it (`SpaceSystemEntry`). */
const sys = (id: string, at: string, extra: Record<string, unknown> = {}) => ({
  kind: 'system',
  id,
  at,
  code: 'member_added',
  actorId: 'u-kari',
  subjectId: 'u-ola',
  ...extra,
})

/** One answer of the windowed timeline endpoint, the first page by default. */
function timelineOf(
  threads: readonly unknown[],
  page: { readonly nextBefore?: string | null; readonly totalPosts?: number | null } = {},
  extra: Record<string, unknown> = {},
) {
  return {
    page: {
      windowed: true,
      hasMore: false,
      nextBefore: page.nextBefore ?? null,
      nextAfter: null,
      newest: 'c-newest',
      totalPosts: page.totalPosts === undefined ? threads.length : page.totalPosts,
    },
    systemEntries: [],
    systemTruncated: false,
    threads,
    pinned: [],
    refreshed: [],
    ...extra,
  }
}

/** A store that has loaded its first page, as the room's page does before it renders the timeline. */
async function loadedStore(answer: ReturnType<typeof timelineOf>) {
  spacesClient.getSpaceTimeline.mockResolvedValueOnce(answer)
  const store = createSpaceTimelineStore('room-1')
  await store.loadFirst()
  return store
}

function renderRoom(store: Awaited<ReturnType<typeof loadedStore>>, extra: Record<string, unknown> = {}) {
  const view = render(() => (
    <SpaceRoomTimeline
      spaceRef="room-1"
      spaceName="Lager"
      threads={() => store.threads() as never}
      roster={() => roster as never}
      agents={() => [agent] as never}
      timeline={store}
      {...(extra as object)}
    />
  ))
  // The watcher registers each post in an effect; the effects must have run before a test reports nearness.
  flush()
  return view
}

const postIds = () => [...document.querySelectorAll('.verevon-room-timeline > li')].map((item) => item.id || 'other')

beforeEach(() => {
  vi.stubGlobal('IntersectionObserver', FakeIntersectionObserver)
  FakeIntersectionObserver.last = undefined
  spacesClient.getSpaceTimeline.mockReset()
  spacesClient.getSpaceThreadTranscript.mockReset()
  spacesClient.loadSpaceTranscript.mockReset().mockResolvedValue({ state: 'ok', threadId: 'x', turns: [] })
  spacesClient.updateSpaceThreadPresentation.mockReset()
})

afterEach(() => {
  clearSpaceRoomFeatures()
  vi.unstubAllGlobals()
  cleanup()
})

describe('SpaceRoomTimeline: the windowed store reads a post when it comes near (WS15-S8)', () => {
  it('reads a post’s transcript tail only once the post is near the viewport', async () => {
    const store = await loadedStore(timelineOf([t('new-a', 10), t('new-b', 20)]))
    spacesClient.loadSpaceTranscript.mockResolvedValue({
      state: 'ok',
      threadId: 'new-a',
      turns: [{ role: 'user', content: 'Hei fra A', authorSubjectId: 'u-kari', messageId: 'm1' }],
      window: { omittedBefore: 0, hasMoreBefore: false },
    })
    renderRoom(store)
    flush()
    expect(spacesClient.loadSpaceTranscript).not.toHaveBeenCalled()

    nearPost('new-a')
    await eventually(() => expect(screen.getByText('Hei fra A')).toBeTruthy())
    expect(spacesClient.loadSpaceTranscript).toHaveBeenCalledTimes(1)
    expect(spacesClient.loadSpaceTranscript).toHaveBeenCalledWith('room-1', 'new-a', { tail: 12, includeFirst: true })
  })

  it('shows the turns the server left out as a row under the first turn, and reads the next window when it is pressed', async () => {
    const store = await loadedStore(timelineOf([t('a', 5)]))
    spacesClient.loadSpaceTranscript
      .mockResolvedValueOnce({
        state: 'ok',
        threadId: 'a',
        turns: [
          { role: 'user', content: 'Første', authorSubjectId: 'u-ola', messageId: 'm1' },
          { role: 'user', content: 'Nyeste', authorSubjectId: 'u-kari', messageId: 'm40' },
        ],
        window: { omittedBefore: 38, hasMoreBefore: true },
      })
      .mockResolvedValueOnce({
        state: 'ok',
        threadId: 'a',
        turns: [{ role: 'user', content: 'Litt eldre', authorSubjectId: 'u-ola', messageId: 'm20' }],
        window: { omittedBefore: 19, hasMoreBefore: true },
      })
    renderRoom(store)
    nearPost('a')
    await eventually(() => expect(screen.getByRole('button', { name: '38 tidligere meldinger' })).toBeTruthy())
    screen.getByRole('button', { name: '38 tidligere meldinger' }).click()
    await waitFor(() => expect(spacesClient.loadSpaceTranscript).toHaveBeenCalledTimes(2))
    expect(spacesClient.loadSpaceTranscript).toHaveBeenLastCalledWith('room-1', 'a', { tail: 12, before: 'm40' })
    await eventually(() => expect(screen.getByText('Litt eldre')).toBeTruthy())
  })
})

describe('SpaceRoomTimeline: Load earlier and the pinned shelf (WS15-S8)', () => {
  it('offers Load earlier while older posts are unloaded, and reads the page before the oldest loaded post', async () => {
    const store = await loadedStore(timelineOf([t('new-a', 30), t('new-b', 31)], { nextBefore: 'cursor-1', totalPosts: 5 }))
    spacesClient.getSpaceTimeline.mockResolvedValueOnce(timelineOf([t('old-a', 10), t('old-b', 20)], { nextBefore: null, totalPosts: 5 }))
    renderRoom(store)
    const earlier = await screen.findByRole('button', { name: 'Last inn tidligere innlegg (3)' })
    earlier.click()
    await eventually(() => expect(document.getElementById('post-old-a')).not.toBeNull())
    expect(spacesClient.getSpaceTimeline).toHaveBeenLastCalledWith('room-1', { limit: 50, before: 'cursor-1' })
    expect(postIds()).toEqual(['post-old-a', 'post-old-b', 'post-new-a', 'post-new-b'])
    expect(screen.queryByRole('button', { name: /Last inn tidligere innlegg/ })).toBeNull()
  })

  it('pages older posts when the reader has scrolled up and the top of the room comes near', async () => {
    const store = await loadedStore(timelineOf([t('new-a', 30)], { nextBefore: 'cursor-1', totalPosts: 3 }))
    spacesClient.getSpaceTimeline.mockResolvedValueOnce(timelineOf([t('old-a', 10)], { nextBefore: null, totalPosts: 3 }))
    renderRoom(store)
    const list = document.querySelector('ol.verevon-room-timeline') as HTMLOListElement
    // jsdom has no layout: the reader's scroll is the position moving up between two scroll events.
    Object.defineProperty(list, 'scrollTop', { configurable: true, writable: true, value: 120 })
    list.dispatchEvent(new Event('scroll'))
    list.scrollTop = 40
    list.dispatchEvent(new Event('scroll'))
    const sentinel = document.querySelector('.verevon-room-earlier-item') as HTMLElement
    FakeIntersectionObserver.last?.report(sentinel, true)
    flush()
    await eventually(() => expect(document.getElementById('post-old-a')).not.toBeNull())
    expect(spacesClient.getSpaceTimeline).toHaveBeenLastCalledWith('room-1', { limit: 50, before: 'cursor-1' })
  })

  it('does not page older posts on its own while the reader has not scrolled up', async () => {
    const store = await loadedStore(timelineOf([t('new-a', 30)], { nextBefore: 'cursor-1', totalPosts: 3 }))
    renderRoom(store)
    const sentinel = document.querySelector('.verevon-room-earlier-item') as HTMLElement
    FakeIntersectionObserver.last?.report(sentinel, true)
    flush()
    await new Promise((resolve) => setTimeout(resolve, 0))
    expect(spacesClient.getSpaceTimeline).toHaveBeenCalledTimes(1)
    expect(document.getElementById('post-old-a')).toBeNull()
  })

  it('draws a pinned post that is older than the window on a shelf above the list, and opens it', async () => {
    const pinnedOld = t('old-pin', 1, { title: 'Festet gammel', pinned: true })
    const store = await loadedStore(timelineOf([t('new-a', 40)], { nextBefore: 'cursor-x', totalPosts: 2 }, { pinned: [pinnedOld] }))
    const open = vi.fn()
    renderRoom(store, { onOpenThread: open })
    const shelfButton = await screen.findByRole('button', { name: 'Festet gammel' })
    expect(shelfButton.closest('.verevon-room-shelf')?.getAttribute('aria-label')).toBe('Festede innlegg')
    shelfButton.click()
    expect(open).toHaveBeenCalledWith(expect.objectContaining({ thread_id: 'old-pin' }))
  })
})

describe('SpaceRoomTimeline: room changes between the posts (WS15-S11)', () => {
  it('draws a change before the first later post, and the changes after the last post at the end of the list', async () => {
    const store = await loadedStore(
      timelineOf([t('a', 10), t('b', 20)], {}, { systemEntries: [sys('s-1', iso(15)), sys('s-2', iso(45), { code: 'archived', actorId: 'u-kari' })] }),
    )
    renderRoom(store)
    await screen.findByText('Sak a')
    expect(postIds()).toEqual(['post-a', 'other', 'post-b', 'other'])
    const rows = [...document.querySelectorAll('.verevon-room-system-row')].map((row) => row.textContent)
    expect(rows[0]).toContain('Kari Nordmann la til Ola Hansen')
    expect(rows[1]).toContain('Kari Nordmann arkiverte rommet')
  })

  it('never anchors a change to a pinned post, even one that is later than the change', async () => {
    const store = await loadedStore(
      timelineOf([t('pin', 50), t('a', 30)], {}, { systemEntries: [sys('s-1', iso(20))] }),
    )
    renderRoom(store, { roomPins: () => [{ thread_id: 'pin', pinned_by: 'u-kari', pinned_at: 1 }] })
    await screen.findByText('Sak pin')
    // The pinned post leads the list; the change goes before the first unpinned post it is earlier than.
    expect(postIds()).toEqual(['post-pin', 'other', 'post-a'])
  })

  it('shows the newest three of a long run of changes behind a count, and all of them once it is pressed', async () => {
    const rows = [1, 2, 3, 4, 5].map((n) => sys(`s-${n}`, iso(n)))
    const store = await loadedStore(timelineOf([t('a', 30)], {}, { systemEntries: rows }))
    renderRoom(store)
    const more = await screen.findByRole('button', { name: '2 tidligere romendringer' })
    expect(document.querySelectorAll('.verevon-room-system-row')).toHaveLength(3)
    more.click()
    await waitFor(() => expect(document.querySelectorAll('.verevon-room-system-row')).toHaveLength(5))
  })
})

describe('SpaceRoomTimeline: the room artifacts flag is read as the room publishes it (WS07-S11)', () => {
  it('takes the artifact chips away when the room turns the flag off after the timeline is drawn', async () => {
    publishSpaceRoomFeatures({ room_artifacts: true })
    spacesClient.loadSpaceTranscript.mockResolvedValue({
      state: 'ok',
      threadId: 't-art',
      turns: [
        {
          role: 'assistant',
          content: 'Her er planen.',
          agentName: 'Lagerhjelpen',
          messageId: 'm2',
          artifacts: [{ id: 'a1', kind: 'document', title: 'Innkjøpsplan', content: 'Dokumentets tekst', version: 2 }],
        },
      ],
    })
    render(() => (
      <SpaceRoomTimeline
        spaceRef="room-1"
        spaceName="Lager"
        threads={() => [t('t-art', 5)] as never}
        roster={() => roster as never}
        agents={() => [agent] as never}
      />
    ))
    await screen.findByRole('button', { name: /Åpne artefakt Innkjøpsplan, versjon 2/ })
    publishSpaceRoomFeatures({ room_artifacts: false })
    flush()
    await waitFor(() => expect(screen.queryByRole('button', { name: /Åpne artefakt/ })).toBeNull())
    // The words of the turn stay; only the surface for the artifact goes.
    expect(screen.getByText('Her er planen.')).toBeTruthy()
  })
})

describe('SpaceRoomTimeline: earlier turns and scroll anchoring (WS15-S1)', () => {
  it('shows the first turn and the newest 29, a row for the 15 between, and all 45 once the row is pressed', async () => {
    const turns = Array.from({ length: 45 }, (_, index) => ({
      role: 'user',
      content: `Melding ${index + 1}`,
      authorSubjectId: 'u-ola',
    }))
    spacesClient.loadSpaceTranscript.mockResolvedValue({ state: 'ok', threadId: 'a', turns })
    render(() => (
      <SpaceRoomTimeline
        spaceRef="room-1"
        spaceName="Lager"
        threads={() => [t('a', 5)] as never}
        roster={() => roster as never}
        agents={() => [agent] as never}
      />
    ))
    expect(await screen.findByText('Melding 45')).toBeTruthy()
    expect(document.querySelectorAll('.verevon-room-turn')).toHaveLength(30)
    expect(screen.queryByText('Melding 2')).toBeNull()
    screen.getByRole('button', { name: '15 tidligere meldinger' }).click()
    await waitFor(() => expect(document.querySelectorAll('.verevon-room-turn')).toHaveLength(45))
    expect(screen.queryByRole('button', { name: '15 tidligere meldinger' })).toBeNull()
    expect(screen.getByText('Melding 2')).toBeTruthy()
  })

  it('draws no anchoring attributes and no jump control while the flag is off', async () => {
    render(() => (
      <SpaceRoomTimeline
        spaceRef="room-1"
        spaceName="Lager"
        threads={() => [t('a', 5), t('b', 6), t('c', 7)] as never}
        roster={() => roster as never}
        agents={() => [agent] as never}
      />
    ))
    await screen.findByText('Sak a')
    expect(document.querySelector('[data-anchored]')).toBeNull()
    expect(document.querySelector('[aria-posinset]')).toBeNull()
    expect(document.querySelector('[aria-setsize]')).toBeNull()
    expect(document.querySelector('.verevon-room-jump')).toBeNull()
  })

  it('numbers each post within the room and marks the list as anchored while the flag is on', async () => {
    publishSpaceRoomFeatures({ timeline_anchor: true })
    render(() => (
      <SpaceRoomTimeline
        spaceRef="room-1"
        spaceName="Lager"
        threads={() => [t('a', 5), t('b', 6), t('c', 7)] as never}
        roster={() => roster as never}
        agents={() => [agent] as never}
      />
    ))
    await screen.findByText('Sak a')
    const posts = [...document.querySelectorAll('.verevon-room-post')]
    expect(posts.map((post) => post.getAttribute('aria-posinset'))).toEqual(['1', '2', '3'])
    expect(posts.every((post) => post.getAttribute('aria-setsize') === '3')).toBe(true)
    expect(document.querySelector('ol.verevon-room-timeline')?.hasAttribute('data-anchored')).toBe(true)
  })
})
