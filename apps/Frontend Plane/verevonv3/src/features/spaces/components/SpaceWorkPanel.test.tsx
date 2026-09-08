import { cleanup, render, screen } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const spacesClient = vi.hoisted(() => ({ getSpaceWork: vi.fn() }))
vi.mock('@/shared/api/spaces-client', () => spacesClient)

const { SpaceWorkPanel } = await import('./SpaceWorkPanel')

const space = { space_ref: 'space_1', name: 'Leveranse', kind: 'room', lifecycle: 'active' }
const membership = {
  space_ref: 'space_1', org_id: 'org_1', subject_id: 'user_1', kind: 'room', role: 'owner',
  revisions: { authority: 1, membership: 1, privacy: 1, recipient_audience: 1, entitlement: 1 },
}

beforeEach(() => {
  spacesClient.getSpaceWork.mockReset()
})

afterEach(() => cleanup())

describe('SpaceWorkPanel', () => {
  it('shows runs and schedules together, consequence first', async () => {
    spacesClient.getSpaceWork.mockResolvedValue({
      space,
      membership,
      runs: [
        { id: 'r1', goal: 'Rydde lager', status: 'completed', space_id: 'space_1', thread_id: 't1' },
        { id: 'r2', goal: 'Sende varsel', status: 'awaiting_approval', space_id: 'space_1', thread_id: 't2' },
      ],
      schedules: [{ id: 'c1', name: 'Daglig rapport', enabled: true, schedule_expr: '0 8 * * *' }],
      unavailable: [],
    })
    render(() => <SpaceWorkPanel spaceRef="space_1" />)

    expect(await screen.findByText('Sende varsel')).toBeTruthy()
    expect(screen.getByText('Rydde lager')).toBeTruthy()
    expect(screen.getByText('Daglig rapport')).toBeTruthy()
    // What needs a person is first, even though the completed run is listed
    // before it upstream.
    const objects = [...document.querySelectorAll('.verevon-activity-object')].map((n) => n.textContent)
    expect(objects[0]).toBe('Sende varsel')
  })

  // The tab's entire purpose is "what needs me". A room with pending work that
  // looks idle is the failure this surface exists to avoid, so a gap is stated
  // alongside whatever did load.
  it('names a missing section while still showing what resolved', async () => {
    spacesClient.getSpaceWork.mockResolvedValue({
      space,
      membership,
      runs: [{ id: 'r1', goal: 'Kjører nå', status: 'running', space_id: 'space_1', thread_id: 't1' }],
      schedules: [],
      unavailable: [{
        section: 'schedules',
        code: 'schedules_upstream_unavailable',
        reason: 'Model Plane could not return this room’s schedules.',
      }],
    })
    render(() => <SpaceWorkPanel spaceRef="space_1" />)

    expect(await screen.findByText('Kjører nå')).toBeTruthy()
    // The known code is said in the reader's language, not relayed in English.
    expect(screen.getByText(/kunne ikke hentes/)).toBeTruthy()
    expect(screen.getByText(/Planlagt arbeid:/)).toBeTruthy()
  })

  // A failed read is not an idle room, and whatever is running is still running.
  it('separates "could not load" from "nothing is running"', async () => {
    spacesClient.getSpaceWork.mockRejectedValue(new Error('down'))
    render(() => <SpaceWorkPanel spaceRef="space_1" />)

    expect(await screen.findByRole('alert')).toBeTruthy()
    expect(screen.getByText(/kjører fortsatt/)).toBeTruthy()
    expect(screen.queryByText(/Ingenting kjører/)).toBeNull()
  })

  // A build that does not know a code must still state the gap, in the
  // server's words, rather than dropping it or inventing a translation.
  it('falls back to the server sentence for an unrecognised code', async () => {
    spacesClient.getSpaceWork.mockResolvedValue({
      space, membership, runs: [], schedules: [],
      unavailable: [{ section: 'monitors', code: 'something_new', reason: 'Monitors are not published yet.' }],
    })
    render(() => <SpaceWorkPanel spaceRef="space_1" />)

    expect(await screen.findByText(/Monitors are not published yet/)).toBeTruthy()
    expect(screen.getByText(/monitors:/)).toBeTruthy()
  })

  it('says plainly when the room genuinely has no work', async () => {
    spacesClient.getSpaceWork.mockResolvedValue({
      space, membership, runs: [], schedules: [], unavailable: [],
    })
    render(() => <SpaceWorkPanel spaceRef="space_1" />)

    expect(await screen.findByText(/Ingenting kjører eller er planlagt/)).toBeTruthy()
    expect(screen.queryByRole('alert')).toBeNull()
  })

  // Control declining the shared read is a real answer: the caller can take
  // part in the room without seeing other members' work.
  it('reports an unauthorized shared read as its own gap', async () => {
    spacesClient.getSpaceWork.mockResolvedValue({
      space,
      membership,
      runs: [],
      schedules: [{ id: 'c1', name: 'Daglig rapport', enabled: true }],
      unavailable: [{
        section: 'runs',
        code: 'runs_read_not_authorized',
        reason: 'Reading this Space’s shared work is not authorized.',
      }],
    })
    render(() => <SpaceWorkPanel spaceRef="space_1" />)

    expect(await screen.findByText(/Kjøringer:/)).toBeTruthy()
    expect(screen.getByText(/andre medlemmer kjører/)).toBeTruthy()
    // And the schedules that DID load are still shown.
    expect(screen.getByText('Daglig rapport')).toBeTruthy()
  })

  // Routines: the room states where they are managed rather than offering a
  // create form the release gates still hold closed (see audit §17).
  it('points to Settings for routines instead of offering a form the room cannot honour', async () => {
    spacesClient.getSpaceWork.mockResolvedValue({ runs: [], schedules: [], unavailable: [] })
    render(() => <SpaceWorkPanel spaceRef="room-1" />)
    const link = await screen.findByRole('link', { name: 'Innstillinger › Planlagte kjøringer' })
    expect(link.getAttribute('href')).toBe('/settings/cron')
    expect(screen.queryByRole('button', { name: /Ny rutine|Opprett/ })).toBeNull()
  })
})
