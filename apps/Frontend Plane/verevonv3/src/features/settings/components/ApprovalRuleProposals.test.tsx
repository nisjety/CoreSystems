// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  ApprovalRuleProposals,
  proposalLabel,
  type RuleProposalCard,
} from '@/features/settings/components/ApprovalRuleProposals'
import {
  confirmApprovalRule,
  type ApprovalRuleProposal,
} from '@/shared/api/orchestration-client'

const reviewedDigest = '2b6a1f0c9d8e7f6a5b4c3d2e1f0a9b8c7d6e5f4a3b2c1d0e9f8a7b6c5d4e3f2a'

const proposal: ApprovalRuleProposal = {
  id: 'arp_01',
  capabilityId: 'cap.mcp_tool.notes',
  toolName: 'mcp__notes__create_note',
  toolDigest: reviewedDigest,
  createdAt: '2026-11-01T10:00:00Z',
}

afterEach(() => cleanup())

describe('ApprovalRuleProposals', () => {
  it('confirming posts to the confirm route with an empty body, naming no capability or digest', async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValue(
        new Response(JSON.stringify({ data: {} }), { status: 200, headers: { 'content-type': 'application/json' } }),
      )
    vi.stubGlobal('fetch', fetchMock)

    await confirmApprovalRule(proposal)

    const [url, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(String(url)).toContain('/api/v1/orchestration/rule-proposals/arp_01/confirm')
    expect(init.method).toBe('POST')
    expect(JSON.parse(init.body as string)).toEqual({})
    vi.unstubAllGlobals()
  })

  it('shows each proposal and hands the choice to the parent that owns the list', () => {
    const onConfirm = vi.fn()
    const onDismiss = vi.fn()
    render(() => (
      <ApprovalRuleProposals proposals={[proposal]} onConfirm={onConfirm} onDismiss={onDismiss} />
    ))
    flush()
    expect(screen.getByText('mcp__notes__create_note')).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: /bekreft regel|confirm rule/i }))
    flush()
    expect(onConfirm).toHaveBeenCalledWith(proposal)

    fireEvent.click(screen.getByRole('button', { name: /avvis|dismiss/i }))
    flush()
    expect(onDismiss).toHaveBeenCalledWith(proposal)
  })

  it('names a shell proposal by its rule, never the command it came from', () => {
    const shell: RuleProposalCard = {
      id: 'arp_02',
      capabilityId: 'cap.command.shell',
      toolName: 'shell',
      toolDigest: '',
      createdAt: '2026-11-04T10:00:00Z',
      ruleText: 'git commit',
    }
    expect(proposalLabel(shell)).toBe('git commit')
    expect(proposalLabel({ ...shell, ruleText: '  ' })).toBe('shell')
    expect(proposalLabel(proposal)).toBe('mcp__notes__create_note')

    render(() => (
      <ApprovalRuleProposals proposals={[shell, proposal]} onConfirm={vi.fn()} onDismiss={vi.fn()} />
    ))
    flush()
    expect(screen.getByText('git commit')).toBeTruthy()
    expect(screen.getByText('mcp__notes__create_note')).toBeTruthy()
    expect(screen.queryByText('shell')).toBeNull()
  })

  it('renders nothing when no rule has been proposed', () => {
    render(() => (
      <ApprovalRuleProposals proposals={[]} onConfirm={vi.fn()} onDismiss={vi.fn()} />
    ))
    flush()
    expect(screen.queryByRole('button')).toBeNull()
  })
})
