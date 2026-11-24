// @vitest-environment jsdom

import { flush } from 'solid-js'
import { cleanup, screen, waitFor, within } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import AttentionPage from './AttentionPage'
import type { AttentionItem, AttentionItemsPage } from '@/shared/api/spaces-attention-items-client'
import { ApiError } from '@/shared/api/http'
import { I18nProvider } from '@/shared/i18n'
import { fireEvent } from '@/test/fire-event'
import { render } from '@/test/render'

// WS11-S7: the home inbox. The inbox client and the cross-room store are replaced, so each test states
// what the server said and checks what the page draws.

const inbox = vi.hoisted(() => ({
  listAttentionItems: vi.fn(),
  setAttentionItemState: vi.fn(),
  refreshSpaceAttention: vi.fn(),
  getThreadFollow: vi.fn(),
  setThreadFollow: vi.fn(),
}))

vi.mock('@/shared/api/spaces-attention-items-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/spaces-attention-items-client')>()),
  listAttentionItems: inbox.listAttentionItems,
  setAttentionItemState: inbox.setAttentionItemState,
  getThreadFollow: inbox.getThreadFollow,
  setThreadFollow: inbox.setThreadFollow,
}))
vi.mock('@/features/spaces/lib/space-attention', () => ({
  refreshSpaceAttention: inbox.refreshSpaceAttention,
}))

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

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((done) => {
    resolve = done
  })
  return { promise, resolve }
}

/** One macrotask, so a settled promise has run its continuation before the page is read. */
const settle = () => new Promise<void>((done) => setTimeout(done, 0))

function renderPage() {
  return render(() => (
    <I18nProvider>
      <AttentionPage />
    </I18nProvider>
  ))
}

function tabs() {
  return screen.getByRole('group', { name: 'Vis' })
}

/** The kind word of each row in one lane, in the order they are drawn. */
function kindsIn(lane: string): (string | null)[] {
  return Array.from(document.querySelectorAll(`section[data-lane="${lane}"] li`), (row) => row.getAttribute('data-kind'))
}

beforeEach(() => {
  inbox.listAttentionItems.mockReset()
  inbox.setAttentionItemState.mockReset()
  inbox.refreshSpaceAttention.mockReset()
  inbox.getThreadFollow.mockReset()
  inbox.setThreadFollow.mockReset()
})

afterEach(() => cleanup())

describe('AttentionPage', () => {
  it('groups the rows into lanes by kind, with a failed run above a finished one', async () => {
    inbox.listAttentionItems.mockResolvedValue(
      page({
        items: [
          item({ itemId: 'item-mention', kind: 'mention' }),
          item({ itemId: 'item-reply', kind: 'reply' }),
          item({ itemId: 'item-done', kind: 'agent_done' }),
          item({ itemId: 'item-failed', kind: 'agent_failed' }),
          item({ itemId: 'item-invite', kind: 'invite' }),
          item({ itemId: 'item-reminder', kind: 'reminder' }),
        ],
      }),
    )

    renderPage()
    await screen.findByText('Påminnelse i Room Alpha')

    expect(kindsIn('mentions')).toEqual(['mention'])
    expect(kindsIn('threads')).toEqual(['reply'])
    expect(kindsIn('agents')).toEqual(['agent_failed', 'agent_done'])
    expect(kindsIn('invites')).toEqual(['invite'])
    expect(kindsIn('reminders')).toEqual(['reminder'])
    expect(document.querySelector('section[data-lane="needs_you"]')).toBeNull()
  })

  it('draws the needs-you lane from the live approvals, and says a title is not available when it is withheld', async () => {
    inbox.listAttentionItems.mockResolvedValue(
      page({
        needsYou: [
          {
            approvalId: 'approval-1',
            spaceRef: 'room-a',
            spaceName: 'Room Alpha',
            threadId: 'thread-1',
            runId: 'run-1',
            requestedAt: NOW,
            headline: null,
            headlineState: 'restricted',
          },
        ],
      }),
    )

    renderPage()
    const lane = await waitFor(() => {
      const found = document.querySelector<HTMLElement>('section[data-lane="needs_you"]')
      expect(found).not.toBeNull()
      return found as HTMLElement
    })

    expect(lane.querySelector('.verevon-attention__count')?.textContent).toBe('1')
    expect(lane.textContent).toContain('En agent venter på godkjenning i Room Alpha')
    expect(lane.textContent).toContain('Ikke tilgjengelig for deg')
    expect(kindsIn('needs_you')).toEqual(['needs_you'])
  })

  it('asks for the tab the viewer picked, and draws only the rows of that tab', async () => {
    inbox.listAttentionItems
      .mockResolvedValueOnce(page({ items: [item({ itemId: 'item-open', kind: 'mention' })] }))
      .mockResolvedValueOnce(page({ items: [item({ itemId: 'item-snoozed', kind: 'reply', state: 'snoozed' })] }))

    renderPage()
    await screen.findByText('Noen nevnte deg i Room Alpha')

    fireEvent.click(within(tabs()).getByRole('button', { name: 'Utsatt' }))

    expect(await screen.findByText('Nye svar i en tråd du følger i Room Alpha')).toBeTruthy()
    expect(inbox.listAttentionItems).toHaveBeenLastCalledWith({ state: 'snoozed', cursor: null })
    expect(screen.queryByText('Noen nevnte deg i Room Alpha')).toBeNull()
    expect(within(tabs()).getByRole('button', { name: 'Utsatt' }).getAttribute('aria-pressed')).toBe('true')
  })

  it('drops a late answer for a tab the viewer has already left', async () => {
    const late = deferred<AttentionItemsPage>()
    inbox.listAttentionItems
      .mockReturnValueOnce(late.promise)
      .mockResolvedValueOnce(page({ items: [item({ itemId: 'item-done', kind: 'reminder', state: 'done' })] }))

    renderPage()
    fireEvent.click(within(tabs()).getByRole('button', { name: 'Ferdig' }))
    await screen.findByText('Påminnelse i Room Alpha')

    late.resolve(page({ items: [item({ itemId: 'item-open', kind: 'mention' })] }))
    await settle()
    flush()

    expect(screen.queryByText('Noen nevnte deg i Room Alpha')).toBeNull()
    expect(screen.getByText('Påminnelse i Room Alpha')).toBeTruthy()
    expect(inbox.listAttentionItems).toHaveBeenNthCalledWith(1, { state: 'open', cursor: null })
    expect(inbox.listAttentionItems).toHaveBeenNthCalledWith(2, { state: 'done', cursor: null })
  })

  it('removes a row once it is marked done, and says nothing needs the viewer when the last row goes', async () => {
    inbox.listAttentionItems.mockResolvedValue(page({ items: [item({ itemId: 'item-1', kind: 'mention' })] }))
    inbox.setAttentionItemState.mockResolvedValue({ itemId: 'item-1', state: 'done', snoozedUntil: null })

    renderPage()
    const row = (await screen.findByText('Noen nevnte deg i Room Alpha')).closest('li') as HTMLElement
    fireEvent.click(within(row).getByRole('button', { name: 'Ferdig' }))

    await waitFor(() => expect(screen.queryByText('Noen nevnte deg i Room Alpha')).toBeNull())
    expect(screen.getByText('Ingenting trenger deg akkurat nå.')).toBeTruthy()
    expect(inbox.setAttentionItemState).toHaveBeenCalledWith('item-1', { state: 'done' })
    expect(inbox.refreshSpaceAttention).toHaveBeenCalledTimes(1)
  })

  it('says the inbox is not turned on when the gateway answers space_attention_disabled', async () => {
    inbox.listAttentionItems.mockRejectedValue(new ApiError('Not found', 404, 'space_attention_disabled'))

    renderPage()

    expect(await screen.findByText('Innboksen er ikke slått på ennå.')).toBeTruthy()
    expect(document.querySelector('section[data-lane]')).toBeNull()
  })

  it('shows more rows with the cursor the last page gave, and appends them', async () => {
    inbox.listAttentionItems
      .mockResolvedValueOnce(page({ items: [item({ itemId: 'item-1', kind: 'mention' })], nextCursor: 'cursor-2' }))
      .mockResolvedValueOnce(page({ items: [item({ itemId: 'item-2', kind: 'invite', spaceName: 'Room Beta' })] }))

    renderPage()
    await screen.findByText('Noen nevnte deg i Room Alpha')
    fireEvent.click(screen.getByRole('button', { name: 'Vis flere' }))

    expect(await screen.findByText('Du er invitert til Room Beta')).toBeTruthy()
    expect(inbox.listAttentionItems).toHaveBeenLastCalledWith({ state: 'open', cursor: 'cursor-2' })
    expect(screen.getByText('Noen nevnte deg i Room Alpha')).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Vis flere' })).toBeNull()
  })

  it('follows the post of a row from the inbox, reading the follow only once the choice is opened (C-PRIO-05)', async () => {
    inbox.listAttentionItems.mockResolvedValue(page({ items: [item({ itemId: 'item-1', kind: 'reply' })] }))
    inbox.getThreadFollow.mockResolvedValue('none')
    inbox.setThreadFollow.mockResolvedValue('following')

    renderPage()
    const row = (await screen.findByText('Nye svar i en tråd du følger i Room Alpha')).closest('li') as HTMLElement
    expect(inbox.getThreadFollow).not.toHaveBeenCalled()

    const toggle = within(row).getByRole('button', { name: 'Varsler' })
    fireEvent.click(toggle)
    expect(toggle.getAttribute('aria-expanded')).toBe('true')
    fireEvent.click(await within(row).findByRole('button', { name: 'Følg' }))

    const following = await within(row).findByRole('button', { name: 'Følger' })
    expect(following.getAttribute('aria-pressed')).toBe('true')
    expect(inbox.getThreadFollow).toHaveBeenCalledWith('room-a', 'thread-1')
    expect(inbox.setThreadFollow).toHaveBeenCalledWith('room-a', 'thread-1', 'following')
  })

  it('says so in words when the follow of an opened row cannot be read, and offers no buttons', async () => {
    inbox.listAttentionItems.mockResolvedValue(page({ items: [item({ itemId: 'item-1', kind: 'reply' })] }))
    inbox.getThreadFollow.mockRejectedValue(new Error('unavailable'))

    renderPage()
    const row = (await screen.findByText('Nye svar i en tråd du følger i Room Alpha')).closest('li') as HTMLElement
    fireEvent.click(within(row).getByRole('button', { name: 'Varsler' }))

    expect(await within(row).findByText('Varslene for denne tråden kan ikke leses nå.')).toBeTruthy()
    expect(within(row).queryByRole('button', { name: 'Følg' })).toBeNull()
    expect(within(row).queryByRole('button', { name: 'Demp' })).toBeNull()
    expect(inbox.setThreadFollow).not.toHaveBeenCalled()
  })

  it('offers no follow choice on a row that is not about a post', async () => {
    inbox.listAttentionItems.mockResolvedValue(page({ items: [item({ itemId: 'item-1', kind: 'invite', threadId: null })] }))

    renderPage()
    const row = (await screen.findByText('Du er invitert til Room Alpha')).closest('li') as HTMLElement

    expect(within(row).queryByRole('button', { name: 'Varsler' })).toBeNull()
    expect(inbox.getThreadFollow).not.toHaveBeenCalled()
  })
})
