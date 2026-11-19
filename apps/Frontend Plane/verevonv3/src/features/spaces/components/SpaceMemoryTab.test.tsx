import { cleanup, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// EP-8 S7: the Space memory tab. The transport is mocked: no request leaves the
// test, and the gateway's answers are the shapes `domains/spaces/memory.rs` builds.

const http = vi.hoisted(() => ({ requestJson: vi.fn() }))
vi.mock('@/shared/api/http', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  ...http,
}))

const { ApiError } = await import('@/shared/api/http')
const { SpaceMemoryTab } = await import('./SpaceMemoryTab')

const row = (over: Record<string, unknown> = {}) => ({
  memory_id: 'm1',
  content: 'Vi leverer til Bergen på fredager.',
  updated_at: '2026-11-03T10:00:00Z',
  provenance: 'stated',
  source_label: 'room_statement',
  thread_id: 't1',
  withheld: false,
  ...over,
})

beforeEach(() => {
  http.requestJson.mockReset()
})

afterEach(() => {
  cleanup()
})

describe('SpaceMemoryTab', () => {
  it('lists the room memory from the Space path, with a link to its conversation', async () => {
    http.requestJson.mockResolvedValueOnce({ memories: [row()], withheld: 0 })
    render(() => <SpaceMemoryTab spaceRef="room 1" />)

    expect(await screen.findByText('Vi leverer til Bergen på fredager.')).toBeTruthy()
    expect(http.requestJson).toHaveBeenCalledWith('/api/v1/spaces/room%201/memory')
    const link = screen.getByRole('link') as HTMLAnchorElement
    expect(link.getAttribute('href')).toBe('/spaces/room%201?thread_id=t1')
    expect(screen.queryByTestId('space-memory-withheld')).toBeNull()
  })

  it('explains the withheld count instead of hiding it', async () => {
    http.requestJson.mockResolvedValueOnce({
      memories: [row(), row({ memory_id: 'm2', content: 'Gammelt', withheld: true })],
      withheld: 1,
    })
    render(() => <SpaceMemoryTab spaceRef="room-1" />)

    const note = await screen.findByTestId('space-memory-withheld')
    expect(note.textContent).toContain('1')
    expect(screen.getByText('Holdt tilbake')).toBeTruthy()
  })

  it('tells a member without the owner or admin role why nothing is listed', async () => {
    http.requestJson.mockRejectedValueOnce(new ApiError('no', 403, 'space_memory_not_authorized'))
    render(() => <SpaceMemoryTab spaceRef="room-1" />)

    expect(await screen.findByText(/Bare eieren eller en administrator/)).toBeTruthy()
    expect(screen.queryByRole('listitem')).toBeNull()
  })

  it('says the feature is off rather than showing an empty memory', async () => {
    http.requestJson.mockRejectedValueOnce(new ApiError('off', 404, 'feature_unavailable'))
    render(() => <SpaceMemoryTab spaceRef="room-1" />)

    expect(await screen.findByText(/ikke slått på/)).toBeTruthy()
    expect(screen.queryByText('Ingenting husket ennå')).toBeNull()
  })

  it('shows a failed read as an error with Retry, never as empty', async () => {
    http.requestJson
      .mockRejectedValueOnce(new ApiError('boom', 500, null))
      .mockResolvedValueOnce({ memories: [], withheld: 0 })
    render(() => <SpaceMemoryTab spaceRef="room-1" />)

    const retry = await screen.findByRole('button', { name: 'Prøv igjen' })
    expect(screen.queryByText('Ingenting husket ennå')).toBeNull()
    retry.click()
    expect(await screen.findByText('Ingenting husket ennå')).toBeTruthy()
  })

  it('deletes a row only after the in-place confirmation', async () => {
    http.requestJson
      .mockResolvedValueOnce({ memories: [row()], withheld: 0 })
      .mockResolvedValueOnce({ deleted: true })
    render(() => <SpaceMemoryTab spaceRef="room-1" />)

    ;(await screen.findByRole('button', { name: 'Slett' })).click()
    expect(http.requestJson).toHaveBeenCalledTimes(1)
    ;(await screen.findByRole('button', { name: 'Bekreft sletting av minnet' })).click()

    await waitFor(() =>
      expect(http.requestJson).toHaveBeenLastCalledWith('/api/v1/spaces/room-1/memory/m1', { method: 'DELETE' }),
    )
    await waitFor(() => expect(screen.queryByText('Vi leverer til Bergen på fredager.')).toBeNull())
  })

  /** Opens the tab with one row and asks to delete it, leaving the in-place confirmation to the test. */
  async function deleteFirstRowWith(failure: unknown) {
    http.requestJson
      .mockResolvedValueOnce({ memories: [row()], withheld: 0 })
      .mockRejectedValueOnce(failure)
    render(() => <SpaceMemoryTab spaceRef="room-1" />)
    ;(await screen.findByRole('button', { name: 'Slett' })).click()
    ;(await screen.findByRole('button', { name: 'Bekreft sletting av minnet' })).click()
  }

  it('says a legal hold kept the row, with its catalogue copy, not as a retryable failure', async () => {
    await deleteFirstRowWith(
      new ApiError('A legal hold covers this data.', 409, 'erasure_blocked_legal_hold', {
        details: { retryable: false },
      }),
    )

    expect(await screen.findByText(/juridisk tilbakeholdelse gjelder disse dataene/)).toBeTruthy()
    expect(screen.queryByText(/Vi fikk ikke slettet minnet/)).toBeNull()
    expect(screen.getByText('Vi leverer til Bergen på fredager.')).toBeTruthy()
  })

  it('says a hold check that could not be made is retryable, with its catalogue copy', async () => {
    await deleteFirstRowWith(
      new ApiError('Legal holds could not be checked.', 503, 'erasure_hold_check_unavailable', {
        details: { retryable: true },
      }),
    )

    expect(await screen.findByText(/Juridiske tilbakeholdelser kunne ikke kontrolleres/)).toBeTruthy()
    expect(screen.queryByText(/Vi fikk ikke slettet minnet/)).toBeNull()
  })

  it('keeps the generic failure copy for any other delete failure', async () => {
    await deleteFirstRowWith(new ApiError('boom', 500, null))

    expect(await screen.findByText('Vi fikk ikke slettet minnet. Prøv igjen.')).toBeTruthy()
  })
})
