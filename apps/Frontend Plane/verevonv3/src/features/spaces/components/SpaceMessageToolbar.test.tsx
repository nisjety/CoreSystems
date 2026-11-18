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
