import { cleanup, fireEvent, render, screen, within } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'

import type { SpaceMessagePolicyHints } from '@/shared/api/spaces-client'
import { createSpaceTurnLifecycle, type LifecycleTurn } from '../lib/space-message-lifecycle'

const { SpaceMessageToolbar } = await import('./SpaceMessageToolbar')

// No I18nProvider wraps these renders, so the copy is the default Norwegian.
const DELETE_CONSEQUENCE = 'Alle ser at meldingen er slettet. Teksten er ikke tilgjengelig lenger.'
const MODERATED_CONSEQUENCE = 'Alle ser at en leder fjernet meldingen. Teksten er ikke tilgjengelig lenger.'
const CONFIRM_DELETE = 'Bekreft sletting av meldingen'

afterEach(() => {
  cleanup()
})

function renderToolbar(options: { canEdit: boolean; canDelete: boolean; moderating?: boolean; busy?: boolean }) {
  const onEdit = vi.fn()
  const onDelete = vi.fn()
  render(() => (
    <SpaceMessageToolbar
      canEdit={options.canEdit}
      canDelete={options.canDelete}
      moderating={options.moderating}
      busy={options.busy}
      onEdit={onEdit}
      onDelete={onDelete}
    />
  ))
  return { onEdit, onDelete }
}

describe('SpaceMessageToolbar buttons per capability (WS12-S7)', () => {
  it('shows Edit and Delete in a named toolbar when both are allowed', () => {
    renderToolbar({ canEdit: true, canDelete: true })
    const toolbar = screen.getByRole('toolbar', { name: 'Meldingshandlinger' })
    expect(within(toolbar).getByRole('button', { name: 'Rediger' })).toBeTruthy()
    expect(within(toolbar).getByRole('button', { name: 'Slett' })).toBeTruthy()
  })

  it('an edit-only viewer sees Edit and no Delete', () => {
    renderToolbar({ canEdit: true, canDelete: false })
    expect(screen.getByRole('button', { name: 'Rediger' })).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Slett' })).toBeNull()
  })

  it('a delete-only viewer sees Delete and no Edit', () => {
    renderToolbar({ canEdit: false, canDelete: true })
    expect(screen.queryByRole('button', { name: 'Rediger' })).toBeNull()
    expect(screen.getByRole('button', { name: 'Slett' })).toBeTruthy()
  })

  it('draws no toolbar at all when neither is allowed', () => {
    renderToolbar({ canEdit: false, canDelete: false })
    expect(screen.queryByRole('toolbar')).toBeNull()
  })
})

describe('SpaceMessageToolbar actions (WS12-S7)', () => {
  it('Edit hands over through onEdit and deletes nothing', () => {
    const { onEdit, onDelete } = renderToolbar({ canEdit: true, canDelete: true })
    fireEvent.click(screen.getByRole('button', { name: 'Rediger' }))
    expect(onEdit).toHaveBeenCalledTimes(1)
    expect(onDelete).not.toHaveBeenCalled()
  })

  it('Delete asks in place first, and deletes only on the confirming press', async () => {
    const { onDelete } = renderToolbar({ canEdit: false, canDelete: true })
    fireEvent.click(screen.getByRole('button', { name: 'Slett' }))
    expect(await screen.findByText(DELETE_CONSEQUENCE)).toBeTruthy()
    expect(onDelete).not.toHaveBeenCalled()

    fireEvent.click(await screen.findByRole('button', { name: CONFIRM_DELETE }))
    expect(onDelete).toHaveBeenCalledTimes(1)
  })

  it("says plainly that a manager removed someone else's message, before the confirming press", async () => {
    const { onDelete } = renderToolbar({ canEdit: false, canDelete: true, moderating: true })
    fireEvent.click(screen.getByRole('button', { name: 'Slett' }))
    expect(await screen.findByText(MODERATED_CONSEQUENCE)).toBeTruthy()
    expect(onDelete).not.toHaveBeenCalled()
  })

  it('both buttons are disabled while a change is in flight', () => {
    renderToolbar({ canEdit: true, canDelete: true, busy: true })
    expect((screen.getByRole('button', { name: 'Rediger' }) as HTMLButtonElement).disabled).toBe(true)
    expect((screen.getByRole('button', { name: 'Slett' }) as HTMLButtonElement).disabled).toBe(true)
  })
})

// The toolbar draws what the lifecycle rules decide: each control comes from the rule that owns it.
const hints = (moderate: boolean): SpaceMessagePolicyHints => ({
  revise_entitled: true,
  moderate_entitled: moderate,
  original_retention: 'purge',
})

function rulesFor(role: string, moderate = false) {
  return createSpaceTurnLifecycle({
    spaceRef: () => 'space-1',
    threadId: () => 'thread-1',
    enabled: () => true,
    viewerSubjectId: () => 'kari',
    viewerRole: () => role,
    threadOwnerSubjectId: () => 'kari',
    policy: () => hints(moderate),
    onChanged: () => {},
  })
}

function renderFor(role: string, turn: LifecycleTurn, moderate = false) {
  const rules = rulesFor(role, moderate)
  return renderToolbar({
    canEdit: rules.canEdit(turn),
    canDelete: rules.canDelete(turn),
    moderating: turn.authorSubjectId !== 'kari',
  })
}

const ownTurn: LifecycleTurn = { role: 'user', messageId: 'm-1', authorSubjectId: 'kari' }
const othersTurn: LifecycleTurn = { role: 'user', messageId: 'm-2', authorSubjectId: 'ola' }

describe('SpaceMessageToolbar follows the lifecycle rules (WS12-S7)', () => {
  it('an editor gets Edit and Delete on their own message', () => {
    renderFor('editor', ownTurn)
    expect(screen.getByRole('button', { name: 'Rediger' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Slett' })).toBeTruthy()
  })

  it("a viewer gets no toolbar, on their own message or on anyone else's", () => {
    renderFor('viewer', ownTurn)
    expect(screen.queryByRole('toolbar')).toBeNull()
    cleanup()
    renderFor('viewer', othersTurn)
    expect(screen.queryByRole('toolbar')).toBeNull()
  })

  it("a manager who may moderate gets Delete alone on someone else's message, worded as a removal", async () => {
    renderFor('manager', othersTurn, true)
    expect(screen.queryByRole('button', { name: 'Rediger' })).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: 'Slett' }))
    expect(await screen.findByText(MODERATED_CONSEQUENCE)).toBeTruthy()
  })

  it('a deleted message offers nothing, even to its author', () => {
    renderFor('editor', { ...ownTurn, revisionState: 'deleted' })
    expect(screen.queryByRole('toolbar')).toBeNull()
  })
})

describe('SpaceMessageToolbar copy link, quick reactions and reason picker (DEPTH-WS12-S7-1)', () => {
  const HREF = '/spaces/space-1?thread_id=thread-1&m=m-1#chat'

  it('Copy link writes the absolute canonical message link and says so', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined)
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true })
    render(() => (
      <SpaceMessageToolbar canEdit={false} canDelete={false} linkHref={HREF} onEdit={() => {}} onDelete={() => {}} />
    ))
    fireEvent.click(screen.getByRole('button', { name: 'Kopier lenke' }))
    expect(await screen.findByText('Lenken er kopiert.')).toBeTruthy()
    expect(writeText).toHaveBeenCalledWith(new URL(HREF, window.location.origin).href)
  })

  it('a failed copy is reported, not swallowed', async () => {
    const writeText = vi.fn().mockRejectedValue(new Error('denied'))
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true })
    render(() => (
      <SpaceMessageToolbar canEdit={false} canDelete={false} linkHref={HREF} onEdit={() => {}} onDelete={() => {}} />
    ))
    fireEvent.click(screen.getByRole('button', { name: 'Kopier lenke' }))
    expect(await screen.findByText('Kunne ikke kopiere lenken.')).toBeTruthy()
  })

  it('quick reactions add an unused emoji and remove one the viewer already used', () => {
    const onReact = vi.fn()
    render(() => (
      <SpaceMessageToolbar
        canEdit={false}
        canDelete={false}
        canReact
        reacted={['\u{1F44D}']}
        onReact={onReact}
        onEdit={() => {}}
        onDelete={() => {}}
      />
    ))
    const toolbar = screen.getByRole('toolbar')
    const thumbs = within(toolbar).getByRole('button', { name: 'Fjern reaksjonen \u{1F44D}' })
    expect(thumbs.getAttribute('aria-pressed')).toBe('true')
    fireEvent.click(thumbs)
    expect(onReact).toHaveBeenLastCalledWith('\u{1F44D}', false)
    fireEvent.click(within(toolbar).getByRole('button', { name: 'Reager med \u{2764}\u{FE0F}' }))
    expect(onReact).toHaveBeenLastCalledWith('\u{2764}\u{FE0F}', true)
  })

  it('offers no quick reactions when the viewer may not react', () => {
    render(() => (
      <SpaceMessageToolbar canEdit canDelete={false} canReact={false} onReact={() => {}} onEdit={() => {}} onDelete={() => {}} />
    ))
    expect(screen.queryByRole('button', { name: /Reager med/ })).toBeNull()
  })

  it('a moderator picks the reason, and the confirmed delete carries it', async () => {
    const onDelete = vi.fn()
    render(() => (
      <SpaceMessageToolbar canEdit={false} canDelete moderating onEdit={() => {}} onDelete={onDelete} />
    ))
    const picker = screen.getByRole('combobox', { name: 'Årsak til fjerning' }) as HTMLSelectElement
    expect(picker.value).toBe('other')
    fireEvent.change(picker, { target: { value: 'spam' } })
    fireEvent.click(screen.getByRole('button', { name: 'Slett' }))
    fireEvent.click(await screen.findByRole('button', { name: CONFIRM_DELETE }))
    expect(onDelete).toHaveBeenCalledWith('spam')
  })

  it('the author deleting their own message sends no reason and sees no picker', async () => {
    const onDelete = vi.fn()
    render(() => <SpaceMessageToolbar canEdit canDelete onEdit={() => {}} onDelete={onDelete} />)
    expect(screen.queryByRole('combobox')).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: 'Slett' }))
    fireEvent.click(await screen.findByRole('button', { name: CONFIRM_DELETE }))
    expect(onDelete).toHaveBeenCalledWith(undefined)
  })

  it('has one Tab stop and moves with the arrow keys, Home and End', async () => {
    render(() => (
      <SpaceMessageToolbar canEdit canDelete linkHref={HREF} onEdit={() => {}} onDelete={() => {}} />
    ))
    // The single Tab stop is set once the controls are in place (a microtask after render).
    await Promise.resolve()
    const copy = screen.getByRole('button', { name: 'Kopier lenke' })
    const edit = screen.getByRole('button', { name: 'Rediger' })
    const del = screen.getByRole('button', { name: 'Slett' })
    expect([copy.tabIndex, edit.tabIndex, del.tabIndex]).toEqual([0, -1, -1])
    copy.focus()
    fireEvent.keyDown(copy, { key: 'ArrowRight' })
    expect(document.activeElement).toBe(edit)
    expect([copy.tabIndex, edit.tabIndex]).toEqual([-1, 0])
    fireEvent.keyDown(edit, { key: 'End' })
    expect(document.activeElement).toBe(del)
    fireEvent.keyDown(del, { key: 'ArrowRight' })
    expect(document.activeElement).toBe(copy)
    fireEvent.keyDown(copy, { key: 'ArrowLeft' })
    expect(document.activeElement).toBe(del)
    fireEvent.keyDown(del, { key: 'Home' })
    expect(document.activeElement).toBe(copy)
  })
})

describe('createSpaceTurnLifecycle remove carries the moderation reason (DEPTH-WS12-S7-1)', () => {
  it("sends the picked reason for someone else's message and none for the author's own", async () => {
    const calls: string[] = []
    const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
      calls.push(String(input))
      return new Response(
        JSON.stringify({ data: { message_id: 'm', revision_state: 'deleted', revision_no: 1, revised_at: null, deduplicated: false } }),
        { status: 200, headers: { 'content-type': 'application/json' } },
      )
    })
    vi.stubGlobal('fetch', fetchMock)
    try {
      const rules = rulesFor('manager', true)
      await rules.remove(othersTurn, 'harassment')
      await rules.remove(ownTurn)
      expect(calls[0]).toContain('moderation_reason=harassment')
      expect(calls[1]).not.toContain('moderation_reason')
    } finally {
      vi.unstubAllGlobals()
    }
  })
})
