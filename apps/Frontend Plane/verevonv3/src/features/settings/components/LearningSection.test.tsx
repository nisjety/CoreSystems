// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { LearningSection, lineDiff } from '@/features/settings/components/LearningSection'

vi.mock('@/shared/session/session-store', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/session/session-store')>()),
  getSession: () => ({ activeOrg: { id: 'org-1' }, user: { id: 'user-1' } }),
}))

function jsonResponse(data: unknown, status = 200): Response {
  return new Response(JSON.stringify(data), {
    headers: { 'Content-Type': 'application/json' },
    status,
  })
}

const PROPOSAL = {
  id: 'skill_1',
  org_id: 'org-1',
  name: 'Utrullingsrutine',
  description: '',
  content: 'Kjør testene\nBygg',
  trigger_keywords: ['deploy'],
  trigger_file_patterns: [],
  tool_restrictions: [],
  enabled: false,
  scope: 'user',
  owner_user_id: 'user-1',
  shared_with: [],
  origin: 'background_review',
  review_state: 'proposed',
  proposal_reason: 'Gjentatt i tre samtaler',
  source_run_id: 'run_1',
}

const NO_SKILLS = { skills: [] }
const EMPTY_LEDGER = { entries: [], next_cursor: '', memory_skip_reason: '' }

function updatedEntry(overrides: Record<string, unknown> = {}) {
  return {
    entry_id: 'led_1',
    kind: 'skill',
    action: 'updated',
    skill_id: 'skill_1',
    skill_name: 'Utrullingsrutine',
    revision: 2,
    actor_kind: 'system',
    thread_id: 'thr_1',
    reason: '',
    before_json: JSON.stringify({ content: 'Kjør testene' }),
    after_json: JSON.stringify({ content: 'Kjør testene\nBygg' }),
    undone: false,
    undoable: true,
    created_at: '2026-10-25T10:00:00Z',
    ...overrides,
  }
}

/**
 * Routes the section's reads and writes; `overrides` replaces one by `METHOD path`.
 * Anything not routed is a 404, so a missing route fails the test rather than passing.
 */
function stubBackend(
  reads: { skills?: unknown; ledger?: unknown } = {},
  overrides: Record<string, () => Response> = {},
) {
  const fetchMock = vi.fn(async (input: unknown, init?: RequestInit) => {
    const key = `${init?.method ?? 'GET'} ${String(input)}`
    const override = overrides[key]
    if (override) return override()
    if (key === 'GET /api/v1/skills') return jsonResponse(reads.skills ?? NO_SKILLS)
    if (key === 'GET /api/v1/learning/ledger') return jsonResponse(reads.ledger ?? EMPTY_LEDGER)
    return jsonResponse({}, 404)
  })
  vi.stubGlobal('fetch', fetchMock)
  return fetchMock
}

function callsTo(fetchMock: ReturnType<typeof stubBackend>, method: string, path: string) {
  return fetchMock.mock.calls.filter(
    ([input, init]) => String(input) === path && ((init as RequestInit | undefined)?.method ?? 'GET') === method,
  )
}

function bodyOf(fetchMock: ReturnType<typeof stubBackend>, method: string, path: string): unknown {
  const [call] = callsTo(fetchMock, method, path)
  return call ? JSON.parse(String((call[1] as RequestInit).body)) : undefined
}

afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
  vi.restoreAllMocks()
})

describe('lineDiff', () => {
  it('marks a changed line as removed and its replacement as added, keeping the rest', () => {
    expect(lineDiff('a\nb\nc', 'a\nx\nc')).toEqual([
      { kind: 'same', text: 'a' },
      { kind: 'removed', text: 'b' },
      { kind: 'added', text: 'x' },
      { kind: 'same', text: 'c' },
    ])
  })

  it('shows a new text as all added and a deleted text as all removed', () => {
    expect(lineDiff('', 'new')).toEqual([{ kind: 'added', text: 'new' }])
    expect(lineDiff('gone', '')).toEqual([{ kind: 'removed', text: 'gone' }])
    expect(lineDiff('', '')).toEqual([])
  })
})

describe('LearningSection proposals', () => {
  it('shows a pending proposal with its reason, its change, and a link to its conversation', async () => {
    stubBackend({
      skills: { skills: [PROPOSAL] },
      ledger: { entries: [updatedEntry()], next_cursor: '', memory_skip_reason: '' },
    })

    render(() => <LearningSection />)

    expect(await screen.findByText('Gjentatt i tre samtaler')).toBeTruthy()
    const diff = screen.getByLabelText('Endringer i forslaget')
    expect(diff.textContent).toContain('+ Bygg')
    expect(diff.textContent).toContain('  Kjør testene')
    const links = screen.getAllByRole('link', { name: 'Åpne samtalen' })
    expect(links.some((link) => link.getAttribute('href') === '/chat?thread_id=thr_1')).toBe(true)
  })

  it('accepting sends only the decision and refreshes the inbox', async () => {
    const fetchMock = stubBackend(
      { skills: { skills: [PROPOSAL] } },
      {
        'POST /api/v1/learning/proposals/skill_1/decision': () =>
          jsonResponse({ review_state: 'accepted', enabled: true, revision: 2 }),
      },
    )

    render(() => <LearningSection />)
    fireEvent.click(await screen.findByRole('button', { name: 'Godta Utrullingsrutine' }))

    await waitFor(() => {
      expect(bodyOf(fetchMock, 'POST', '/api/v1/learning/proposals/skill_1/decision')).toEqual({
        decision: 'accept',
      })
    })
    await waitFor(() => expect(callsTo(fetchMock, 'GET', '/api/v1/skills').length).toBeGreaterThan(1))
  })

  it('rejecting sends the reject decision', async () => {
    const fetchMock = stubBackend(
      { skills: { skills: [PROPOSAL] } },
      {
        'POST /api/v1/learning/proposals/skill_1/decision': () =>
          jsonResponse({ review_state: 'rejected', enabled: false, revision: 2 }),
      },
    )

    render(() => <LearningSection />)
    fireEvent.click(await screen.findByRole('button', { name: 'Avvis Utrullingsrutine' }))

    await waitFor(() => {
      expect(bodyOf(fetchMock, 'POST', '/api/v1/learning/proposals/skill_1/decision')).toEqual({
        decision: 'reject',
      })
    })
  })

  it('shows the server refusal as an alert and does not claim the decision was saved', async () => {
    stubBackend(
      { skills: { skills: [PROPOSAL] } },
      {
        'POST /api/v1/learning/proposals/skill_1/decision': () =>
          jsonResponse({ error: { code: 'not_proposed', message: 'not_proposed' } }, 409),
      },
    )

    render(() => <LearningSection />)
    fireEvent.click(await screen.findByRole('button', { name: 'Godta Utrullingsrutine' }))

    expect(await screen.findByRole('alert')).toBeTruthy()
    expect(screen.getByRole('alert').textContent).toContain('Kunne ikke lagre avgjørelsen.')
  })

  it('shows nothing pending when no proposal belongs to the signed-in owner', async () => {
    stubBackend({
      skills: { skills: [{ ...PROPOSAL, owner_user_id: 'user-2' }, { ...PROPOSAL, id: 'skill_2', review_state: 'accepted' }] },
    })

    render(() => <LearningSection />)

    expect(await screen.findByText('Ingen forslag venter på deg.')).toBeTruthy()
  })
})

describe('LearningSection ledger', () => {
  it('undoes a skill change with one click', async () => {
    const fetchMock = stubBackend(
      { ledger: { entries: [updatedEntry()], next_cursor: '', memory_skip_reason: '' } },
      {
        'POST /api/v1/learning/ledger/led_1/undo': () =>
          jsonResponse({ action: 'restored', skill_id: 'skill_1', revision: 3 }),
      },
    )

    render(() => <LearningSection />)
    fireEvent.click(await screen.findByRole('button', { name: 'Angre Utrullingsrutine' }))

    await waitFor(() => expect(callsTo(fetchMock, 'POST', '/api/v1/learning/ledger/led_1/undo')).toHaveLength(1))
  })

  it('forgets a learned memory only after a second click, through the memory delete path', async () => {
    const memory = updatedEntry({
      entry_id: 'mem_1',
      kind: 'memory',
      action: 'learned',
      skill_id: '',
      skill_name: '',
      revision: 0,
      after_json: JSON.stringify({ content: 'Bruker mørk modus' }),
      before_json: '',
      undoable: false,
    })
    const fetchMock = stubBackend(
      { ledger: { entries: [memory], next_cursor: '', memory_skip_reason: '' } },
      { 'DELETE /api/v1/memory/mem_1': () => jsonResponse({ deleted: true }) },
    )

    render(() => <LearningSection />)
    fireEvent.click(await screen.findByRole('button', { name: 'Glem Bruker mørk modus' }))

    expect(await screen.findByText('Bekreft glemsel?')).toBeTruthy()
    expect(callsTo(fetchMock, 'DELETE', '/api/v1/memory/mem_1')).toHaveLength(0)

    fireEvent.click(screen.getByRole('button', { name: 'Glem Bruker mørk modus' }))
    await waitFor(() => expect(callsTo(fetchMock, 'DELETE', '/api/v1/memory/mem_1')).toHaveLength(1))
  })

  it('reports a memory the server declined to erase as not forgotten', async () => {
    const memory = updatedEntry({
      entry_id: 'mem_2',
      kind: 'memory',
      action: 'learned',
      skill_id: '',
      skill_name: '',
      revision: 0,
      after_json: JSON.stringify({ content: 'Liker kaffe' }),
      before_json: '',
      undoable: false,
    })
    stubBackend(
      { ledger: { entries: [memory], next_cursor: '', memory_skip_reason: '' } },
      {
        'DELETE /api/v1/memory/mem_2': () =>
          jsonResponse({ deleted: false, degraded: true, degradation_reason: 'zdr' }),
      },
    )

    render(() => <LearningSection />)
    const forget = await screen.findByRole('button', { name: 'Glem Liker kaffe' })
    fireEvent.click(forget)
    expect(await screen.findByText('Bekreft glemsel?')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Glem Liker kaffe' }))

    expect(await screen.findByRole('alert')).toBeTruthy()
    expect(screen.getByRole('alert').textContent).toContain('Minnet ble ikke glemt.')
  })

  it('offers no undo for a change that cannot be undone or already was', async () => {
    stubBackend({
      ledger: {
        entries: [
          updatedEntry({ entry_id: 'led_old', undoable: false }),
          updatedEntry({ entry_id: 'led_done', undone: true }),
        ],
        next_cursor: '',
        memory_skip_reason: '',
      },
    })

    render(() => <LearningSection />)

    expect(await screen.findByText('Angret')).toBeTruthy()
    expect(screen.queryByRole('button', { name: /^Angre/ })).toBeNull()
  })

  it('says why memories are missing when memory is paused', async () => {
    stubBackend({ ledger: { entries: [], next_cursor: '', memory_skip_reason: 'user_paused' } })

    render(() => <LearningSection />)

    expect(await screen.findByText('Minner vises ikke mens minnet er pauset.')).toBeTruthy()
  })

  it('shows an empty ledger as nothing learned yet, not as an error', async () => {
    stubBackend({})

    render(() => <LearningSection />)

    expect(await screen.findByText('Verevon har ikke lært noe ennå.')).toBeTruthy()
    expect(screen.queryByRole('alert')).toBeNull()
  })
})
