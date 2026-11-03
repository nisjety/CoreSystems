import { cleanup, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { ApiError } from '@/shared/api/http'

const client = vi.hoisted(() => ({
  listSpaceStandingApprovals: vi.fn(),
  revokeSpaceStandingApproval: vi.fn(),
}))

vi.mock('@/shared/api/space-approvals-client', () => client)
vi.mock('@/shared/a11y/announcer', () => ({ announce: vi.fn() }))

const { SpaceStandingApprovals } = await import('./SpaceStandingApprovals')

function rule(ruleId: string, toolKey: string, useCount = 0) {
  return {
    ruleId,
    scope: 'always',
    agentSubjectId: 'agent-1',
    toolKey,
    actionKey: toolKey,
    grantedBySubjectId: 'user-1',
    grantedByRole: 'manager',
    createdAt: '2026-11-01T10:00:00Z',
    reviewBy: '2026-12-01T10:00:00Z',
    useCount,
  }
}

beforeEach(() => {
  client.listSpaceStandingApprovals.mockReset()
  client.revokeSpaceStandingApproval.mockReset()
})

afterEach(() => {
  cleanup()
})

describe('SpaceStandingApprovals', () => {
  it('stays out of the way for non-managers and never asks Control', async () => {
    const view = render(() => <SpaceStandingApprovals spaceRef="space-1" canGovern={false} />)
    await Promise.resolve()
    expect(view.container.innerHTML).toBe('')
    expect(client.listSpaceStandingApprovals).not.toHaveBeenCalled()
  })

  it('is hidden when standing approvals are switched off', async () => {
    client.listSpaceStandingApprovals.mockRejectedValue(
      new ApiError('off', 404, 'feature_disabled'),
    )
    const view = render(() => <SpaceStandingApprovals spaceRef="space-1" canGovern={true} />)
    await waitFor(() => expect(client.listSpaceStandingApprovals).toHaveBeenCalled())
    await waitFor(() => expect(view.container.querySelector('section')).toBeNull())
    expect(screen.queryByRole('alert')).toBeNull()
  })

  it('is hidden when Control refuses the role', async () => {
    client.listSpaceStandingApprovals.mockRejectedValue(new ApiError('no', 403, 'role_forbidden'))
    const view = render(() => <SpaceStandingApprovals spaceRef="space-1" canGovern={true} />)
    await waitFor(() => expect(view.container.querySelector('section')).toBeNull())
  })

  it('separates a failed read from an empty list', async () => {
    client.listSpaceStandingApprovals.mockRejectedValue(new ApiError('boom', 503, null))
    render(() => <SpaceStandingApprovals spaceRef="space-1" canGovern={true} />)
    expect(await screen.findByRole('alert')).toBeTruthy()
    expect(screen.getByText(/Vi fikk ikke hentet de faste godkjenningene/)).toBeTruthy()
    expect(screen.queryByText(/Ingen faste godkjenninger/)).toBeNull()
  })

  it('says so when the room has no rules', async () => {
    client.listSpaceStandingApprovals.mockResolvedValue([])
    render(() => <SpaceStandingApprovals spaceRef="space-1" canGovern={true} />)
    expect(await screen.findByText('Ingen faste godkjenninger i dette rommet.')).toBeTruthy()
  })

  it('lists the rules, revokes one, and re-reads what Control holds', async () => {
    client.listSpaceStandingApprovals
      .mockResolvedValueOnce([rule('rule-1', 'mcp__github__create_issue', 3), rule('rule-2', 'mcp__slack__post')])
      .mockResolvedValueOnce([rule('rule-2', 'mcp__slack__post')])
    client.revokeSpaceStandingApproval.mockResolvedValue(undefined)
    render(() => <SpaceStandingApprovals spaceRef="space-1" canGovern={true} />)

    expect(await screen.findByText('mcp__github__create_issue')).toBeTruthy()
    expect(screen.getByText('mcp__slack__post')).toBeTruthy()
    expect(client.listSpaceStandingApprovals).toHaveBeenCalledWith('space-1')

    ;(screen.getAllByRole('button', { name: 'Trekk tilbake' })[0] as HTMLButtonElement).click()

    await waitFor(() => expect(client.revokeSpaceStandingApproval).toHaveBeenCalledWith('space-1', 'rule-1'))
    // The list is the second read, not an optimistic edit.
    await waitFor(() => expect(client.listSpaceStandingApprovals).toHaveBeenCalledTimes(2))
    await waitFor(() => expect(screen.queryByText('mcp__github__create_issue')).toBeNull())
    expect(screen.getByText('mcp__slack__post')).toBeTruthy()
  })

  it('reports a failed revoke and still re-reads before inviting another try', async () => {
    client.listSpaceStandingApprovals.mockResolvedValue([rule('rule-1', 'mcp__github__create_issue')])
    client.revokeSpaceStandingApproval.mockRejectedValue(new ApiError('boom', 503, null))
    render(() => <SpaceStandingApprovals spaceRef="space-1" canGovern={true} />)

    ;(await screen.findByRole('button', { name: 'Trekk tilbake' })).click()

    expect(await screen.findByText(/Vi fikk ikke trukket regelen tilbake/)).toBeTruthy()
    await waitFor(() => expect(client.listSpaceStandingApprovals).toHaveBeenCalledTimes(2))
    // The rule is still listed: nothing claimed it was gone.
    expect(screen.getByText('mcp__github__create_issue')).toBeTruthy()
  })
})
