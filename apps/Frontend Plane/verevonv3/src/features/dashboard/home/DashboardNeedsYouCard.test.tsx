// @vitest-environment jsdom
import { cleanup, render, screen, waitFor, within } from '@solidjs/testing-library'
import { createRoot, createSignal, createStore, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import type { AttentionItem, AttentionItemsPage, AttentionNeedsYou } from '@/shared/api/spaces-attention-items-client'
import { ApiError } from '@/shared/api/http'

// WS11-S7: the "Trenger deg" card on Home. The inbox client, the roster and the shell counts are replaced,
// so each test states what the server said and checks what the card draws.

const inbox = vi.hoisted(() => ({
  listAttentionItems: vi.fn(),
  getSpaceRoster: vi.fn(),
}))
/** The shell's flag and counts; a test points them at signals it can move. */
const shell = vi.hoisted(() => ({ available: (): boolean => true, totals: (): unknown => undefined }))

vi.mock('@/shared/api/spaces-attention-items-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/spaces-attention-items-client')>()),
  listAttentionItems: inbox.listAttentionItems,
}))
vi.mock('@/shared/api/spaces-client', () => ({ getSpaceRoster: inbox.getSpaceRoster }))
vi.mock('@/features/spaces/lib/space-attention', () => ({
  spaceAttentionAvailable: () => shell.available(),
  spaceAttentionTotals: () => shell.totals(),
}))
vi.mock('@/shared/i18n', () => ({ useI18n: () => ({ tr: (_no: string, en: string) => en, locale: () => 'en' }) }))
vi.mock('@/shared/session/session-store', () => ({ getSession: () => identity }))

import { resetRosterNamesForTests } from '@/features/attention/lib/attention-roster'
import { DashboardNeedsYouCard } from './DashboardNeedsYouCard'

const [identity, setIdentity] = createStore({ user: { id: 'user-a' }, activeOrg: { id: 'org-a' } })
const NOW = Date.parse('2026-11-18T09:00:00Z')

function item(overrides: Partial<AttentionItem> & Pick<AttentionItem, 'itemId' | 'kind'>): AttentionItem {
  return {
    spaceRef: 'room-a',
    spaceName: 'Room Alpha',
    threadId: 'thread-1',
    messageId: null,
    runId: null,
    actor: { kind: 'user', subjectId: 'user-1', agentRef: null },
    count: 1,
    state: 'open',
    snoozedUntil: null,
    createdAt: NOW,
    updatedAt: NOW,
    headline: null,
    headlineState: 'ok',
    ...overrides,
  }
}

function page(overrides: Partial<AttentionItemsPage> = {}): AttentionItemsPage {
  return { items: [], needsYou: [], mode: 'live', gaps: [], nextCursor: null, ...overrides }
}

const approval: AttentionNeedsYou = {
  approvalId: 'approval-1',
  spaceRef: 'room-a',
  spaceName: 'Room Alpha',
  threadId: 'thread-1',
  runId: 'run-1',
  requestedAt: NOW,
  headline: null,
  headlineState: 'ok',
}

/** One macrotask, so a settled promise has run its continuation before the card is read. */
const settle = () => new Promise<void>((done) => setTimeout(done, 0))

const card = () => document.querySelector<HTMLElement>('.home-needs-you')

beforeEach(() => {
  inbox.listAttentionItems.mockReset()
  inbox.getSpaceRoster.mockReset()
  inbox.getSpaceRoster.mockResolvedValue([])
  shell.available = () => true
  shell.totals = () => undefined
  resetRosterNamesForTests()
  setIdentity((state) => {
    state.user.id = 'user-a'
    state.activeOrg.id = 'org-a'
  })
  flush()
})

afterEach(() => cleanup())

describe('Home "Needs you" card', () => {
  it('draws the approval and the open rows waiting for the viewer, named from the roster, with the inbox one link away', async () => {
    inbox.getSpaceRoster.mockResolvedValue([
      { subject_type: 'user', subject_id: 'user-1', role: 'editor', revision: 1, display_name: 'Ola Nordmann' },
    ])
    inbox.listAttentionItems.mockResolvedValue(
      page({
        needsYou: [approval],
        items: [item({ itemId: 'item-mention', kind: 'mention' }), item({ itemId: 'item-reply', kind: 'reply', threadId: null })],
      }),
    )

    render(() => <DashboardNeedsYouCard />)

    expect(await screen.findByRole('heading', { level: 2, name: 'Waiting for you' })).toBeTruthy()
    expect(await screen.findByText('Ola Nordmann mentioned you in Room Alpha')).toBeTruthy()
    expect(screen.getByText('An agent is waiting for your approval in Room Alpha')).toBeTruthy()
    expect(screen.getByText('New replies in a thread you follow in Room Alpha')).toBeTruthy()
    expect(inbox.listAttentionItems).toHaveBeenCalledWith({ state: 'open', limit: 6 })
    expect(screen.getByRole('link', { name: 'Open your inbox' }).getAttribute('href')).toBe('/attention')
  })

  it('draws nothing when the inbox is not turned on', async () => {
    inbox.listAttentionItems.mockRejectedValue(new ApiError('Not found', 404, 'space_attention_disabled'))

    render(() => <DashboardNeedsYouCard />)
    await settle()
    flush()

    expect(inbox.listAttentionItems).toHaveBeenCalledTimes(1)
    expect(card()).toBeNull()
  })

  it('does not ask at all while the shell says the surface is off', async () => {
    shell.available = () => false
    inbox.listAttentionItems.mockResolvedValue(page({ needsYou: [approval] }))

    render(() => <DashboardNeedsYouCard />)
    await settle()
    flush()

    expect(inbox.listAttentionItems).not.toHaveBeenCalled()
    expect(card()).toBeNull()
  })

  it('draws nothing when no row is open, and does not show an empty inbox as a result', async () => {
    inbox.listAttentionItems.mockResolvedValue(page())

    render(() => <DashboardNeedsYouCard />)
    await settle()
    flush()

    expect(card()).toBeNull()
  })

  it('says the inbox could not be read, in words and without a heading, rather than showing it as empty', async () => {
    inbox.listAttentionItems.mockRejectedValue(new Error('offline'))

    render(() => <DashboardNeedsYouCard />)

    expect(await screen.findByText('Needs you could not be loaded right now.')).toBeTruthy()
    expect(within(card() as HTMLElement).queryByRole('heading', { level: 2 })).toBeNull()
    expect(within(card() as HTMLElement).getByRole('link', { name: 'Open your inbox' })).toBeTruthy()
  })

  it('says a read was partial, and still draws the rows it has', async () => {
    inbox.listAttentionItems.mockResolvedValue(page({ items: [item({ itemId: 'item-invite', kind: 'invite' })], gaps: ['session_core'] }))

    render(() => <DashboardNeedsYouCard />)

    expect(await screen.findByText('You were invited to Room Alpha')).toBeTruthy()
    expect(screen.getByText('Something could not be read right now, so rows may be missing.')).toBeTruthy()
  })

  it('reads the inbox again only when the shell counts move, not on every poll', async () => {
    inbox.listAttentionItems.mockResolvedValue(page({ needsYou: [approval] }))
    const [totals, setTotals] = createRoot(() => createSignal<unknown>({ badge: 1, unreadRooms: 0, otherOpen: 0 }))
    shell.totals = totals

    render(() => <DashboardNeedsYouCard />)
    await screen.findByText('An agent is waiting for your approval in Room Alpha')
    expect(inbox.listAttentionItems).toHaveBeenCalledTimes(1)

    // A poll that brings the same counts asks nothing new.
    setTotals({ badge: 1, unreadRooms: 0, otherOpen: 0 })
    flush()
    await settle()
    expect(inbox.listAttentionItems).toHaveBeenCalledTimes(1)

    // A poll that moves the counts reads the inbox again.
    setTotals({ badge: 0, unreadRooms: 0, otherOpen: 0 })
    flush()
    await waitFor(() => expect(inbox.listAttentionItems).toHaveBeenCalledTimes(2))
  })
})
