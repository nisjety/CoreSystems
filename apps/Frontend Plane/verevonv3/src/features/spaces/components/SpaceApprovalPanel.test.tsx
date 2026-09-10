import { cleanup, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const approvals = vi.hoisted(() => ({
  readPendingApprovals: vi.fn(),
  settleApproval: vi.fn(),
}))

vi.mock('../lib/space-approvals', async () => {
  const actual = await vi.importActual<typeof import('../lib/space-approvals')>(
    '../lib/space-approvals',
  )
  return {
    ...actual,
    readPendingApprovals: approvals.readPendingApprovals,
    settleApproval: approvals.settleApproval,
  }
})

const { SpaceApprovalPanel } = await import('./SpaceApprovalPanel')

beforeEach(() => {
  approvals.readPendingApprovals.mockReset()
  approvals.settleApproval.mockReset()
})

afterEach(() => {
  cleanup()
})

describe('SpaceApprovalPanel', () => {
  it('offers exactly the two decisions the backend can record', async () => {
    approvals.readPendingApprovals.mockResolvedValue({
      approvals: [{ id: 'ap-1', detail: 'Send the summary to the customer' }],
      refused: false,
    })
    render(() => <SpaceApprovalPanel runId="run-1" />)

    expect(await screen.findByText('Send the summary to the customer')).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Godkjenn' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Avslå' })).toBeTruthy()
    // Every reference product offers a remembered "always allow"; Model Plane
    // records one decision for one approval, so offering it here would be a
    // button that silently does something narrower than it says.
    expect(screen.queryByRole('button', { name: /alltid|always/i })).toBeNull()
  })

  it('decides, then tells the room so the post stops saying it is waiting', async () => {
    approvals.readPendingApprovals.mockResolvedValue({
      approvals: [{ id: 'ap-1', detail: 'Run the deploy' }],
      refused: false,
    })
    approvals.settleApproval.mockResolvedValue('granted')
    const onSettled = vi.fn()
    render(() => <SpaceApprovalPanel runId="run-1" onSettled={onSettled} />)

    const approve = await screen.findByRole('button', { name: 'Godkjenn' })
    approve.click()

    await waitFor(() => expect(approvals.settleApproval).toHaveBeenCalled())
    expect(approvals.settleApproval).toHaveBeenCalledWith({
      approvalId: 'ap-1',
      runId: 'run-1',
      decision: 'approve',
    })
    await waitFor(() => expect(onSettled).toHaveBeenCalled())
    expect(await screen.findByText(/Kjøringen fortsetter/)).toBeTruthy()
  })

  // The room shows other members' work. Seeing that something waits without
  // being able to answer it is a normal state, and it has to read as one.
  it('says who decides when the caller may not', async () => {
    approvals.readPendingApprovals.mockResolvedValue({ approvals: [], refused: true })
    render(() => <SpaceApprovalPanel runId="run-1" />)

    expect(await screen.findByText(/tilhører et annet medlem/)).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Godkjenn' })).toBeNull()
  })

  // An unreachable Model Plane must never render as "nothing to approve" — the
  // run is still paused, and saying otherwise invites the reader to walk away.
  it('separates "could not load" from "nothing pending"', async () => {
    approvals.readPendingApprovals.mockRejectedValue(new Error('down'))
    render(() => <SpaceApprovalPanel runId="run-1" />)

    expect(await screen.findByRole('alert')).toBeTruthy()
    expect(screen.getByText(/står fortsatt på pause/)).toBeTruthy()
    expect(screen.queryByText(/Ingenting venter/)).toBeNull()
  })

  it('reports an unconfirmed decision without inviting a blind retry', async () => {
    approvals.readPendingApprovals.mockResolvedValue({
      approvals: [{ id: 'ap-1', detail: 'Charge the card' }],
      refused: false,
    })
    approvals.settleApproval.mockResolvedValue('unconfirmed')
    render(() => <SpaceApprovalPanel runId="run-1" />)

    ;(await screen.findByRole('button', { name: 'Godkjenn' })).click()

    expect(await screen.findByText(/Sjekk statusen før du prøver igjen/)).toBeTruthy()
    // "Try again" would risk deciding twice at a gate that already moved.
    expect(screen.queryByRole('button', { name: /Prøv igjen/i })).toBeNull()
  })
})
