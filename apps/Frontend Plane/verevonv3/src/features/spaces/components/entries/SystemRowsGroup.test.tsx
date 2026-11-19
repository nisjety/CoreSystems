import { cleanup, render, waitFor } from '@solidjs/testing-library'
import { afterEach, describe, expect, it } from 'vitest'

import type { SpaceRosterMember, SpaceSystemEntry } from '@/shared/api/spaces-client'
import { SYSTEM_ROWS_COLLAPSE_AFTER, SystemRowsGroup } from './SystemRowsGroup'

// WS15-S11: the room's membership changes that fall between two posts. Up to three show; a longer run shows its
// newest three behind a count and expands on click. Names come from the roster; an id the roster does not hold
// reads as a former member, and no raw id is ever drawn. The viewer is Kari, so her own name carries "(deg)".
// No I18nProvider wraps these renders, so the copy is the Norwegian default.

afterEach(() => cleanup())

const roster = [
  { subject_type: 'user', subject_id: 'u-kari', role: 'owner', revision: 1, display_name: 'Kari Nordmann' },
  { subject_type: 'user', subject_id: 'u-ola', role: 'editor', revision: 1, display_name: 'Ola Hansen' },
] as SpaceRosterMember[]

const at = (minute: number) => new Date(Date.UTC(2026, 10, 1, 9, minute)).toISOString()

function row(n: number, overrides: Partial<SpaceSystemEntry> = {}): SpaceSystemEntry {
  return { kind: 'system', id: `sys-${n}`, at: at(n), code: 'member_joined', subjectId: 'u-ola', ...overrides }
}

function renderGroup(entries: readonly SpaceSystemEntry[]) {
  return render(() => (
    <ol>
      <SystemRowsGroup entries={entries} roster={() => roster} viewerSubjectId={() => 'u-kari'} />
    </ol>
  ))
}

const rows = (): HTMLElement[] => [...document.querySelectorAll<HTMLElement>('.verevon-room-system-row')]
const moreButton = (): HTMLButtonElement | null => document.querySelector<HTMLButtonElement>('.verevon-room-system-more')

describe('SystemRowsGroup (WS15-S11)', () => {
  it('draws up to three rows with no collapse control', () => {
    renderGroup([row(1), row(2), row(3)])
    expect(rows()).toHaveLength(3)
    expect(moreButton()).toBeNull()
  })

  it('shows the newest three behind a count of the rest, and expands them all on click', async () => {
    const entries = [1, 2, 3, 4, 5].map((n) => row(n))
    const view = renderGroup(entries)
    expect(SYSTEM_ROWS_COLLAPSE_AFTER).toBe(3)
    expect(rows()).toHaveLength(3)
    expect(rows().map((item) => item.querySelector('time')?.getAttribute('datetime'))).toEqual([at(3), at(4), at(5)])
    expect(moreButton()?.textContent).toBe('2 tidligere romendringer')

    moreButton()?.click()
    await waitFor(() => expect(rows()).toHaveLength(5))
    expect(moreButton()).toBeNull()
    view.unmount()
  })

  it('names members from the roster, marks the viewer, and reads an id the roster does not hold as a former member', () => {
    renderGroup([
      row(1, { code: 'member_added', actorId: 'u-kari', subjectId: 'u-ola' }),
      row(2, { code: 'member_removed', actorId: 'gone-7', subjectId: 'u-ola' }),
    ])
    expect(rows()[0]?.textContent).toContain('Kari Nordmann (deg) la til Ola Hansen')
    expect(rows()[1]?.textContent).toContain('Tidligere medlem fjernet Ola Hansen')
    expect(document.body.textContent).not.toContain('gone-7')
  })

  it('says someone when the ledger recorded no actor, and a member when it recorded no subject', () => {
    renderGroup([
      row(1, { code: 'archived', actorId: undefined }),
      row(2, { code: 'member_joined', subjectId: undefined }),
    ])
    expect(rows()[0]?.textContent).toContain('Noen arkiverte rommet')
    expect(rows()[1]?.textContent).toContain('et medlem ble med i rommet')
  })

  it('reads a code this version does not know as a generic room change', () => {
    renderGroup([row(1, { code: 'something_new' })])
    expect(rows()[0]?.getAttribute('data-entry-code')).toBe('something_new')
    expect(rows()[0]?.textContent).toContain('Rommet ble endret')
    expect(document.body.textContent).not.toContain('something_new')
  })
})
