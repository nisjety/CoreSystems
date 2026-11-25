import { cleanup, render, screen } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { __resetRetentionForTests, applyServerRetention } from '@/features/chat/lib/chat-retention'
import { I18nProvider, localeStorageKey } from '@/shared/i18n'
import { clearSpaceRoomFeatures, publishSpaceRoomFeatures } from '../../lib/space-room-features'

// The rows of one thread, read-only: deleted and edited messages (WS12-S7), the per-turn files of a message
// (WS14-S8), the "earlier turns" row (WS15-S1), a person's post as markdown behind `human_markdown` (WS15-S4),
// and the typed entries a turn carries (WS15-S9, WS18-S7). ChatMarkdown is stubbed to a marker that holds the
// text it was given, so the tests can see exactly what the list hands to it. No I18nProvider wraps these
// renders, so the copy is the Norwegian default.

vi.mock('@/features/chat/components/ChatMessages', () => ({
  ChatMarkdown: (props: { content: string }) => <div class="chat-md-stub" data-md="true">{props.content}</div>,
}))

const { SpaceTurnList, normalizeTranscriptTurns } = await import('./SpaceTurnList')

const roster = [
  { subject_type: 'user', subject_id: 'u-kari', role: 'owner', revision: 1, display_name: 'Kari Nordmann' },
  { subject_type: 'user', subject_id: 'u-ola', role: 'editor', revision: 1, display_name: 'Ola Hansen' },
]

const at = (hour: number, minute: number) => new Date(2026, 9, 31, hour, minute).toISOString()

function renderTurns(turns: readonly unknown[], extra: Record<string, unknown> = {}) {
  return render(() => (
    <SpaceTurnList
      turns={() => turns as never}
      roster={() => roster as never}
      viewerSubjectId={() => 'u-kari'}
      agentName="Lagerhjelpen"
      {...(extra as object)}
    />
  ))
}

afterEach(() => {
  clearSpaceRoomFeatures()
  __resetRetentionForTests()
  cleanup()
})

describe('SpaceTurnList: deleted and edited messages (WS12-S7)', () => {
  it('draws a deleted member message as a tombstone with no text and no body', () => {
    renderTurns([{ role: 'user', content: '', authorSubjectId: 'u-ola', revisionState: 'deleted', revisedBySubjectId: 'u-ola' }])
    const tombstone = document.querySelector('[data-revision-state="deleted"]') as HTMLElement
    expect(tombstone.tagName).toBe('P')
    expect(tombstone.textContent).toBe('Meldingen er slettet.')
    expect(document.querySelector('.verevon-room-turn__plain')).toBeNull()
    expect(document.querySelector('.verevon-room-turn__markdown')).toBeNull()
  })

  it('says a manager removed a member message when someone else revised it', () => {
    renderTurns([{ role: 'user', content: '', authorSubjectId: 'u-ola', revisionState: 'deleted', revisedBySubjectId: 'u-kari' }])
    expect(document.querySelector('[data-revision-state="deleted"]')?.textContent).toBe('Meldingen ble fjernet av en leder.')
  })

  it('says a deleted answer was deleted, and that an agent had already answered it', () => {
    renderTurns([{ role: 'assistant', content: '', agentName: 'Lagerhjelpen', revisionState: 'deleted', agentFollowedUp: true }])
    expect(document.querySelector('[data-revision-state="deleted"]')?.textContent).toBe(
      'Dette svaret er slettet. En agent hadde allerede svart på den.',
    )
  })

  it('marks an edited message with its own note and the time of the change, and keeps its text', () => {
    renderTurns([
      { role: 'user', content: 'Rettet tekst', authorSubjectId: 'u-ola', revisionState: 'edited', revisedAt: at(15, 4) },
    ])
    const marker = document.querySelector('[data-revision-state="edited"]') as HTMLElement
    expect(marker.textContent).toBe('(redigert)')
    expect(marker.getAttribute('title')).toMatch(/15[.:]04/)
    expect(document.querySelector('.verevon-room-turn__plain')?.textContent).toBe('Rettet tekst')
    expect(document.querySelector('[data-revision-state="deleted"]')).toBeNull()
  })

  it('draws neither marker on an ordinary message', () => {
    renderTurns([{ role: 'user', content: 'Vanlig melding', authorSubjectId: 'u-ola' }])
    expect(document.querySelector('[data-revision-state]')).toBeNull()
    expect(document.querySelector('.verevon-room-turn__tombstone')).toBeNull()
  })
})

describe('normalizeTranscriptTurns: identified and revised turns (WS12-S7)', () => {
  it('keeps a deleted turn with no text as a tombstone with its id and revision, and drops an empty member turn', () => {
    const kept = normalizeTranscriptTurns([
      {
        role: 'user',
        content: '',
        messageId: 'm-9',
        authorSubjectId: 'u-ola',
        revisionState: 'deleted',
        revisedAt: at(9, 30),
        revisedBySubjectId: 'u-kari',
      },
      { role: 'user', content: '   ', authorSubjectId: 'u-ola' },
      { role: 'user', content: 'Ny tekst', messageId: 'm-10', revisionState: 'edited', revisedAt: 'ikke en tid' },
    ])
    expect(kept).toHaveLength(2)
    expect(kept[0]).toMatchObject({
      role: 'user',
      content: '',
      messageId: 'm-9',
      revisionState: 'deleted',
      revisedAt: at(9, 30),
      revisedBySubjectId: 'u-kari',
    })
    expect(kept[1]).toMatchObject({ content: 'Ny tekst', messageId: 'm-10', revisionState: 'edited' })
    // A revision time that is not a time is left out rather than invented.
    expect('revisedAt' in (kept[1] as object)).toBe(false)
  })
})

describe('SpaceTurnList: the files of a message (WS14-S8)', () => {
  const withFile = 'Se vedlegg\n\n--- VEDLEGG: rapport.txt ---\nHemmelig innhold i fila\n--- SLUTT PÅ VEDLEGG: rapport.txt ---'

  it('draws the words, then one chip per file by name, and never the fenced text', () => {
    renderTurns([{ role: 'user', content: withFile, authorSubjectId: 'u-ola' }])
    expect(document.querySelector('.verevon-room-turn__plain')?.textContent).toBe('Se vedlegg')
    const chips = document.querySelectorAll('.verevon-space-attachment-chip')
    expect(chips).toHaveLength(1)
    expect(chips[0]?.textContent).toBe('rapport.txt')
    expect(chips[0]?.getAttribute('title')).toBe('23 tegn')
    expect(document.body.textContent).not.toContain('Hemmelig innhold i fila')
  })

  it('keeps every file of a message, in the order they were sent', () => {
    const twoFiles = 'To filer\n\n--- VEDLEGG: a.txt ---\nA\n--- SLUTT PÅ VEDLEGG: a.txt ---\n\n--- VEDLEGG: b.csv ---\nB\n--- SLUTT PÅ VEDLEGG: b.csv ---'
    renderTurns([{ role: 'user', content: twoFiles, authorSubjectId: 'u-ola' }])
    expect([...document.querySelectorAll('.verevon-space-attachment-chip')].map((chip) => chip.textContent)).toEqual(['a.txt', 'b.csv'])
  })

  it('draws no chip list for a message that carries no file', () => {
    renderTurns([{ role: 'user', content: 'Bare ord', authorSubjectId: 'u-ola' }])
    expect(document.querySelector('.verevon-space-attachment-chips')).toBeNull()
  })
})

describe('SpaceTurnList: earlier turns (WS15-S1)', () => {
  const two = [
    { role: 'user', content: 'Første', authorSubjectId: 'u-ola', createdAt: at(10, 0) },
    { role: 'user', content: 'Andre', authorSubjectId: 'u-ola', createdAt: at(10, 1) },
  ]

  it('draws a row for the turns left out, under the first turn, and calls onExpand once when it is pressed', () => {
    const onExpand = vi.fn()
    renderTurns(two, { earlierTurns: () => ({ count: 4, onExpand }) })
    const gap = document.querySelector('.verevon-room-turns-gap__button') as HTMLButtonElement
    expect(gap.textContent).toBe('4 tidligere meldinger')
    const first = document.querySelector('.verevon-room-turn') as HTMLElement
    expect(first.nextElementSibling?.classList.contains('verevon-room-turns-gap')).toBe(true)
    gap.click()
    expect(onExpand).toHaveBeenCalledTimes(1)
  })

  it('does not group the turn after the gap under the first turn, even when it is by the same member', () => {
    renderTurns(two, { earlierTurns: () => ({ count: 4, onExpand: () => undefined }) })
    expect(document.querySelectorAll('.verevon-room-turn__author')).toHaveLength(2)
    expect(document.querySelector('.verevon-room-turn[data-grouped]')).toBeNull()
  })

  it('says one turn in the singular', () => {
    renderTurns([two[0]], { earlierTurns: () => ({ count: 1, onExpand: () => undefined }) })
    expect(document.querySelector('.verevon-room-turns-gap__button')?.textContent).toBe('1 tidligere melding')
  })

  it('draws no row when nothing is left out', () => {
    renderTurns([two[0]], { earlierTurns: () => undefined })
    expect(document.querySelector('.verevon-room-turns-gap')).toBeNull()
  })

  it('keeps the first turn and the newest turns when a transcript is over the cap, not the oldest thirty', () => {
    const turns = Array.from({ length: 40 }, (_, i) => ({ role: 'user', content: `Melding ${i + 1}`, authorSubjectId: 'u-ola' }))
    const kept = normalizeTranscriptTurns(turns, 30)
    expect(kept).toHaveLength(30)
    expect(kept[0]?.content).toBe('Melding 1')
    expect(kept.slice(1).map((turn) => turn.content)).toEqual(Array.from({ length: 29 }, (_, i) => `Melding ${i + 12}`))
  })
})

describe('SpaceTurnList: typed entries (WS15-S9, WS18-S7)', () => {
  it('draws an agent-card turn as its card, with its text only inside the card and never as markdown', () => {
    renderTurns([
      {
        role: 'assistant',
        content: 'Importen er ferdig.',
        agentName: 'Lagerhjelpen',
        entry: { kind: 'agent_card', version: 1, agent_card: { title: 'Import', tone: 'done', fields: [{ label: 'Rader', value: '1 204' }] } },
      },
    ])
    const card = document.querySelector('[data-entry-kind="agent_card"]') as HTMLElement
    expect(card.querySelector('.verevon-room-entry--card__title')?.textContent).toBe('Import')
    expect(card.querySelector('.verevon-room-entry--card__body')?.textContent).toBe('Importen er ferdig.')
    expect(document.querySelector('[data-md]')).toBeNull()
    expect(document.querySelector('.verevon-room-turn__markdown')).toBeNull()
  })

  it('draws a turn with no entry as its markdown body, as before', () => {
    renderTurns([{ role: 'assistant', content: 'Vanlig svar', agentName: 'Lagerhjelpen' }])
    expect(document.querySelector('.verevon-room-turn__markdown [data-md]')?.textContent).toBe('Vanlig svar')
    expect(document.querySelector('[data-entry-kind]')).toBeNull()
  })

  it('draws a kind with no card as the neutral card, beside the text the turn still has', () => {
    renderTurns([{ role: 'assistant', content: 'Svar med et ukjent element', agentName: 'Lagerhjelpen', entry: { kind: 'poll', version: 1 } }])
    expect(document.querySelector('.verevon-room-entry--unknown')).not.toBeNull()
    expect(document.querySelector('.verevon-room-turn__markdown [data-md]')?.textContent).toBe('Svar med et ukjent element')
  })
})

describe('SpaceTurnList: a person’s post as markdown (WS15-S4)', () => {
  const text = 'Her er **fet** tekst\n\n- ett\n- to'

  it('draws the plain pre-wrapped line while human_markdown is off', () => {
    renderTurns([{ role: 'user', content: text, authorSubjectId: 'u-ola' }])
    expect(document.querySelector('.verevon-room-turn__plain')?.textContent).toBe(text)
    expect(document.querySelector('.verevon-room-turn__markdown')).toBeNull()
  })

  it('draws the words through ChatMarkdown with human_markdown on, and never the attachment fence', () => {
    publishSpaceRoomFeatures({ human_markdown: true })
    renderTurns([
      {
        role: 'user',
        content: `${text}\n\n--- VEDLEGG: a.txt ---\nx\n--- SLUTT PÅ VEDLEGG: a.txt ---`,
        authorSubjectId: 'u-ola',
      },
    ])
    expect(document.querySelector('.verevon-room-turn__plain')).toBeNull()
    expect(document.querySelector('.verevon-room-turn__markdown [data-md]')?.textContent).toBe(text)
    expect(document.body.textContent).not.toContain('SLUTT PÅ VEDLEGG')
    expect(document.querySelector('.verevon-space-attachment-chip')?.textContent).toBe('a.txt')
  })

  it('picks out mentions on the markdown path too, with the mention picker on (DEPTH-WS08-S6-2)', async () => {
    publishSpaceRoomFeatures({ human_markdown: true, mention_picker: true })
    renderTurns([{ role: 'user', content: 'Takk @Kari Nordmann og @Ola Hansen', authorSubjectId: 'u-ola' }])
    await Promise.resolve()
    const marks = [...document.querySelectorAll('.verevon-room-turn__markdown mark.verevon-space-mention')]
    expect(marks.map((mark) => mark.textContent)).toEqual(['@Kari Nordmann', '@Ola Hansen'])
    // The reader (u-kari) is emphasized as self.
    expect(marks.map((mark) => (mark as HTMLElement).dataset.self ?? null)).toEqual(['true', null])
  })

  it('leaves the markdown path unmarked while the mention picker is off', async () => {
    publishSpaceRoomFeatures({ human_markdown: true })
    renderTurns([{ role: 'user', content: 'Takk @Kari Nordmann', authorSubjectId: 'u-ola' }])
    await Promise.resolve()
    expect(document.querySelector('.verevon-room-turn__markdown [data-md]')?.textContent).toBe('Takk @Kari Nordmann')
    expect(document.querySelector('mark')).toBeNull()
  })
})

// WS16-S1: a turn's time, in the header and in the gutter of a grouped row, and the time of an edit, read in the
// reader's locale. The second and third turns group under the first (same author, within five minutes), so their
// times sit in the gutter; the third is edited, and its "(edited)" note carries the time of the change.
describe('SpaceTurnList: turn times follow the reader’s locale (WS16-S1)', () => {
  const turns = () => [
    { role: 'user', content: 'Første', authorSubjectId: 'u-ola', createdAt: at(14, 32) },
    { role: 'user', content: 'Andre', authorSubjectId: 'u-ola', createdAt: at(14, 33) },
    {
      role: 'user',
      content: 'Rettet',
      authorSubjectId: 'u-ola',
      createdAt: at(14, 34),
      revisionState: 'edited',
      revisedAt: at(15, 4),
    },
  ]

  function renderLocalised(locale: 'no' | 'en') {
    window.localStorage.setItem(localeStorageKey, locale)
    return render(() => (
      <I18nProvider>
        <SpaceTurnList
          turns={() => turns() as never}
          roster={() => roster as never}
          viewerSubjectId={() => 'u-kari'}
          agentName="Lagerhjelpen"
        />
      </I18nProvider>
    ))
  }

  afterEach(() => {
    window.localStorage.removeItem(localeStorageKey)
  })

  it('draws the times as Norwegian clock times for a Norwegian reader', () => {
    renderLocalised('no')
    const times = [...document.querySelectorAll('.verevon-room-turn__time')].map((el) => el.textContent)
    expect(times).toEqual(['14:32', '14:33', '14:34'])
    expect(document.querySelector('[data-revision-state="edited"]')?.getAttribute('title')).toMatch(/31\. oktober 2026/)
  })

  it('draws the times, and the time of the edit, in English for an English reader', () => {
    renderLocalised('en')
    const times = [...document.querySelectorAll('.verevon-room-turn__time')].map((el) => el.textContent ?? '')
    expect(times).toHaveLength(3)
    expect(times[0]).toMatch(/^02:32\s?PM$/)
    expect(times[1]).toMatch(/^02:33\s?PM$/)
    expect(times[2]).toMatch(/^02:34\s?PM$/)
    const marker = document.querySelector('[data-revision-state="edited"]') as HTMLElement
    expect(marker.textContent).toBe('(edited)')
    expect(marker.getAttribute('title')).toMatch(/October 31, 2026.*3:04\s?PM/)
  })
})

describe('SpaceTurnList: Remind me on each message (WS11-S11)', () => {
  // The message toolbar is drawn for a message only with a lifecycle, and nothing here is editable, so
  // Remind me is the only control on it.
  const lifecycle = {
    canEdit: () => false,
    canDelete: () => false,
    canReact: () => false,
    edit: async () => undefined,
    remove: async () => undefined,
    react: async () => [],
  }
  const turns = [
    { role: 'user', content: 'Husk levering', authorSubjectId: 'u-ola', messageId: 'm-1' },
    { role: 'user', content: 'Og lageret', authorSubjectId: 'u-ola', messageId: 'm-2' },
  ]
  const inThread = { spaceRef: 'room-1', threadId: 'thread-1', lifecycle }

  it('draws Remind me on each message only while reminders are offered for the room', () => {
    renderTurns(turns, inThread)
    expect(screen.queryAllByRole('button', { name: 'Påminn meg' })).toHaveLength(0)
    cleanup()

    publishSpaceRoomFeatures({ reminders: true })
    renderTurns(turns, inThread)
    expect(screen.getAllByRole('button', { name: 'Påminn meg' })).toHaveLength(2)
  })

  it('draws none under zero data retention, even with the flag on', () => {
    applyServerRetention(true)
    publishSpaceRoomFeatures({ reminders: true })
    renderTurns(turns, inThread)
    expect(screen.getByText('Og lageret')).toBeTruthy()
    expect(screen.queryAllByRole('button', { name: 'Påminn meg' })).toHaveLength(0)
  })

  it('draws none for a message without an id, or in a list that names no thread', () => {
    publishSpaceRoomFeatures({ reminders: true })
    renderTurns([{ role: 'user', content: 'Uten id', authorSubjectId: 'u-ola' }], inThread)
    renderTurns(turns, { spaceRef: 'room-1', lifecycle })
    expect(screen.queryAllByRole('button', { name: 'Påminn meg' })).toHaveLength(0)
  })
})
