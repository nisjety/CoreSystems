import { cleanup, render, screen } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const spacesClient = vi.hoisted(() => ({ getSpaceActivity: vi.fn() }))
vi.mock('@/shared/api/spaces-client', () => spacesClient)

const { SpaceActivityPanel } = await import('./SpaceActivityPanel')

const space = { space_ref: 'space_1', name: 'Leveranse', kind: 'room', lifecycle: 'active' }
const membership = {
  space_ref: 'space_1', org_id: 'org_1', subject_id: 'user_1', kind: 'room', role: 'owner',
  revisions: { authority: 1, membership: 1, privacy: 1, recipient_audience: 1, entitlement: 1 },
}

const answer = (overrides: Record<string, unknown>) => ({
  space, membership, runs: [], approvals: [], operations: [], authority: [], unavailable: [],
  ...overrides,
})

const renderPanel = (threads: unknown[] = []) =>
  render(() => (
    <SpaceActivityPanel
      spaceRef="space_1"
      threads={() => threads as never}
      threadsLoading={() => false}
    />
  ))

beforeEach(() => {
  spacesClient.getSpaceActivity.mockReset()
})

afterEach(() => cleanup())

describe('SpaceActivityPanel', () => {
  // S2.3 slice 5 / S2.5: the effect that actually left the system, in the room
  // whose authority permitted it.
  it('shows an owner effect and the authority behind it', async () => {
    spacesClient.getSpaceActivity.mockResolvedValue(
      answer({
        operations: [{
          operation_id: 'op-1', action_id: 'tickets.create', status: 'completed',
          subject_id: 'agent-7', ticket_id: 'ticket-9', created_at: '2026-09-07T10:00:00Z',
        }],
        authority: [{
          grant_id: 'grant-1', action_id: 'tickets.create', subject_id: 'agent-7',
          created_by_user_id: 'ima', created_at: '2026-09-06T10:00:00Z',
        }],
      }),
    )
    renderPanel()

    expect(await screen.findByText('Opprettet sak')).toBeTruthy()
    expect(screen.getByText('utført')).toBeTruthy()
    // The proof the effect produced, not just that it succeeded.
    expect(screen.getByText('Sak ticket-9')).toBeTruthy()
    expect(screen.getByText('Ga fullmakt')).toBeTruthy()
    expect(screen.getByText('Gitt av ima')).toBeTruthy()
  })

  // `unknown` means "we cannot tell whether this happened", which outranks a
  // plain failure: the failure at least resolved.
  it('raises an unknown outcome above a completed one', async () => {
    spacesClient.getSpaceActivity.mockResolvedValue(
      answer({
        operations: [
          { operation_id: 'op-done', action_id: 'tickets.create', status: 'completed', ticket_id: 't1', created_at: '2026-09-07T12:00:00Z' },
          { operation_id: 'op-unknown', action_id: 'tickets.create', status: 'unknown', terminal_reason: 'kvittering kom ikke', created_at: '2026-09-07T09:00:00Z' },
        ],
      }),
    )
    renderPanel()

    await screen.findByText('utfall er ukjent')
    const outcomes = [...document.querySelectorAll('.verevon-activity-outcome')].map((n) => n.textContent)
    // Newer AND completed still loses to older AND unknown.
    expect(outcomes[0]).toBe('utfall er ukjent')
    expect(screen.getByText('kvittering kom ikke')).toBeTruthy()
  })

  // An operation that is not `completed` has no ticket to name, and the row
  // must not imply one.
  it('never names a ticket for an effect it cannot prove landed', async () => {
    spacesClient.getSpaceActivity.mockResolvedValue(
      answer({
        operations: [{ operation_id: 'op-1', action_id: 'tickets.create', status: 'cancelled' }],
      }),
    )
    renderPanel()

    expect(await screen.findByText('avbrutt før effekt')).toBeTruthy()
    expect(screen.queryByText(/^Sak /)).toBeNull()
  })

  // The run contract has carried tokens and steps all along; Work never read
  // them and this is the tab that asks what something cost.
  it('shows a run’s cost and effort beside its outcome', async () => {
    spacesClient.getSpaceActivity.mockResolvedValue(
      answer({
        runs: [{
          run_id: 'run-1', goal: 'Rydde lager', status: 'completed', space_id: 'space_1',
          thread_id: 't1', input_tokens: 900, output_tokens: 120, steps_completed: 4,
        }],
      }),
    )
    renderPanel()

    expect(await screen.findByText('Rydde lager')).toBeTruthy()
    expect(screen.getByText('1020 tokens')).toBeTruthy()
    expect(screen.getByText('4 steg')).toBeTruthy()
  })

  it('renders a revoked authority as the withdrawal, keeping when it was given', async () => {
    spacesClient.getSpaceActivity.mockResolvedValue(
      answer({
        authority: [{
          grant_id: 'grant-1', action_id: 'tickets.create', subject_id: 'agent-7',
          created_by_user_id: 'ima', created_at: '2026-09-01T10:00:00Z',
          revoked_at: '2026-09-05T10:00:00Z', revoked_by_user_id: 'lead',
        }],
      }),
    )
    renderPanel()

    expect(await screen.findByText('Fjernet fullmakt')).toBeTruthy()
    expect(screen.getByText('ikke lenger tillatt')).toBeTruthy()
    expect(screen.getByText('Trukket av lead')).toBeTruthy()
    expect(screen.queryByText('Ga fullmakt')).toBeNull()
  })

  // A filter that hides rows without saying how many is a control that misleads
  // its own user.
  it('says how much a filter is hiding', async () => {
    spacesClient.getSpaceActivity.mockResolvedValue(
      answer({
        operations: [{ operation_id: 'op-1', action_id: 'tickets.create', status: 'completed', ticket_id: 't1' }],
        authority: [{ grant_id: 'grant-1', action_id: 'tickets.create', subject_id: 'a', created_at: '2026-09-01T10:00:00Z' }],
      }),
    )
    renderPanel()

    await screen.findByText('Opprettet sak')
    expect(screen.queryByText(/skjult av dette filteret/)).toBeNull()

    screen.getByRole('radio', { name: 'Fullmakter' }).click()
    expect(await screen.findByText(/1 flere hendelser er skjult/)).toBeTruthy()
    expect(screen.queryByText('Opprettet sak')).toBeNull()
  })

  it('filters by consequence, not by which plane a row came from', async () => {
    spacesClient.getSpaceActivity.mockResolvedValue(
      answer({
        operations: [{ operation_id: 'op-1', action_id: 'tickets.create', status: 'unknown' }],
        approvals: [{ id: 'ap-1', status: 'APPROVAL_STATE_REQUESTED', kind: 'tool' }],
        authority: [{ grant_id: 'grant-1', action_id: 'tickets.create', subject_id: 'a', created_at: '2026-09-01T10:00:00Z' }],
      }),
    )
    renderPanel()

    await screen.findByText('utfall er ukjent')
    screen.getByRole('radio', { name: 'Trenger oppmerksomhet' }).click()
    // Both the unknown effect and the waiting approval survive; the settled
    // authority row does not.
    expect(await screen.findByText('venter på en avgjørelse')).toBeTruthy()
    expect(screen.getByText('utfall er ukjent')).toBeTruthy()
    expect(screen.queryByText('Ga fullmakt')).toBeNull()
  })

  it('names a gap in the reader’s language and keeps what resolved', async () => {
    spacesClient.getSpaceActivity.mockResolvedValue(
      answer({
        operations: [{ operation_id: 'op-1', action_id: 'tickets.create', status: 'completed', ticket_id: 't1' }],
        unavailable: [{
          section: 'runs',
          code: 'runs_read_not_authorized',
          reason: 'Reading this Space’s shared work is not authorized.',
        }],
      }),
    )
    renderPanel()

    expect(await screen.findByText('Opprettet sak')).toBeTruthy()
    expect(screen.getByText(/andre medlemmer har kjørt/)).toBeTruthy()
    expect(screen.queryByText(/not authorized/)).toBeNull()
  })

  it('falls back to the server sentence for an unrecognised code', async () => {
    spacesClient.getSpaceActivity.mockResolvedValue(
      answer({ unavailable: [{ section: 'ledger', code: 'brand_new', reason: 'Ledger is offline.' }] }),
    )
    renderPanel()

    expect(await screen.findByText(/Ledger is offline/)).toBeTruthy()
    expect(screen.getByText(/ledger:/)).toBeTruthy()
  })

  // A failed read must not read as a quiet room, and the thread spine the page
  // already fetched is still worth showing.
  it('keeps the conversation projection when the wider read fails', async () => {
    spacesClient.getSpaceActivity.mockRejectedValue(new Error('down'))
    renderPanel([
      { thread_id: 't1', title: 'Innkjøp', latest_run_status: 'completed', updated_at: '2026-09-07T10:00:00Z' },
    ])

    expect(await screen.findByRole('alert')).toBeTruthy()
    // `getAllByText`: a thread carrying a latest run correctly yields BOTH a
    // conversation row and a run row, and both name the thread.
    expect(screen.getAllByText('Innkjøp').length).toBeGreaterThan(0)
  })

  // A read in flight is not an empty room. The loading line and the "nothing
  // here" line were briefly on screen together, which is two contradictory
  // claims about the same room.
  it('does not claim the room is empty while the read is still in flight', async () => {
    let settle: (value: unknown) => void = () => {}
    spacesClient.getSpaceActivity.mockReturnValue(new Promise((resolve) => { settle = resolve }))
    renderPanel()

    expect(await screen.findByText(/Henter aktiviteten/)).toBeTruthy()
    expect(screen.queryByText(/Ingen aktivitet er publisert/)).toBeNull()

    settle(answer({}))
    expect(await screen.findByText(/Ingen aktivitet er publisert/)).toBeTruthy()
  })

  it('says plainly when the room genuinely has no activity', async () => {
    spacesClient.getSpaceActivity.mockResolvedValue(answer({}))
    renderPanel()

    expect(await screen.findByText(/Ingen aktivitet er publisert/)).toBeTruthy()
    expect(screen.queryByRole('alert')).toBeNull()
  })
})
