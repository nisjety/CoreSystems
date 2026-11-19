import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { UNTRUSTED_CONTENT_KIND } from '@/shared/api/orchestration-client'

const approvals = vi.hoisted(() => ({
  readPendingApprovals: vi.fn(),
  settleApproval: vi.fn(),
  readQuarantinePreview: vi.fn(),
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

vi.mock('@/shared/api/orchestration-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/orchestration-client')>()),
  readQuarantinePreview: approvals.readQuarantinePreview,
}))

const { SpaceApprovalPanel } = await import('./SpaceApprovalPanel')

beforeEach(() => {
  approvals.readPendingApprovals.mockReset()
  approvals.settleApproval.mockReset()
  approvals.readQuarantinePreview.mockReset()
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
    expect(approvals.settleApproval).toHaveBeenCalledWith(
      expect.objectContaining({ approvalId: 'ap-1', runId: 'run-1', decision: 'approve', scope: 'once' }),
    )
    await waitFor(() => expect(onSettled).toHaveBeenCalled())
    expect(await screen.findByText(/Godkjent\. Avgjørelsen er registrert/)).toBeTruthy()
  })

  // The room shows other members' work. Seeing that something waits without
  // being able to answer it is a normal state, and it has to read as one.
  it('says who decides when the caller may not', async () => {
    approvals.readPendingApprovals.mockResolvedValue({ approvals: [], refused: true })
    render(() => <SpaceApprovalPanel runId="run-1" />)

    expect(await screen.findByText(/Venter på medlemmet som startet kjøringen/)).toBeTruthy()
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

// WS10-S1: what approving does is the server's `resumeClass`, said in words; the sentence after a
// decision follows what was settled, and never promises that the run continues by itself.
describe('SpaceApprovalPanel: what approving does (WS10-S1)', () => {
  it.each([
    [
      'action_continuation',
      'Godkjenning kjører denne handlingen som medlemmet som ba om den. Agenten fortsetter ikke av seg selv. Avslag stopper kjøringen.',
    ],
    ['none', 'Godkjenning lar agenten prøve akkurat denne handlingen på nytt én gang. Avslag stopper kjøringen.'],
    ['live_poll', 'Godkjenning lar det kjørende nettleser-steget gå videre. Avslag stopper det.'],
    [undefined, 'Godkjenning registrerer avgjørelsen din. Avslag stopper kjøringen.'],
  ] as const)('words the consequence for resume class %s', async (resumeClass, copy) => {
    approvals.readPendingApprovals.mockResolvedValue({
      approvals: [{ id: 'ap-1', detail: 'Run the deploy', resumeClass }],
      refused: false,
    })
    render(() => <SpaceApprovalPanel runId="run-1" />)
    expect(await screen.findByText(copy)).toBeTruthy()
    cleanup()
  })

  it.each([
    ['granted', 'Godkjent. Avgjørelsen er registrert.', 'status'],
    [
      'granted_action',
      'Godkjent. Handlingen kjøres som medlemmet som ba om den. Agenten fortsetter ikke av seg selv.',
      'status',
    ],
    [
      'granted_retry',
      'Godkjent. Ingenting kjøres nå: agenten kan prøve akkurat denne handlingen på nytt én gang.',
      'status',
    ],
    ['granted_live', 'Godkjent. Det kjørende nettleser-steget går videre.', 'status'],
    ['granted_continue', 'Godkjent. Handlingen brukes når kjøringen fortsetter.', 'status'],
    ['denied', 'Avslått. Kjøringen er stoppet.', 'status'],
    ['already_decided', 'Denne var allerede avgjort — sannsynligvis av noen andre.', 'status'],
    ['refused', 'Du kan ikke avgjøre denne. Den venter på medlemmet som startet kjøringen.', 'status'],
    [
      'unconfirmed',
      'Vi fikk ikke bekreftet om avgjørelsen ble registrert. Sjekk statusen før du prøver igjen.',
      'alert',
    ],
  ] as const)('says the settled outcome %s in words, as %s', async (outcome, copy, role) => {
    approvals.readPendingApprovals.mockResolvedValue({
      approvals: [{ id: 'ap-1', detail: 'Run the deploy', resumeClass: 'action_continuation' }],
      refused: false,
    })
    approvals.settleApproval.mockResolvedValue(outcome)
    render(() => <SpaceApprovalPanel runId="run-1" />)

    ;(await screen.findByRole('button', { name: 'Godkjenn' })).click()

    const line = await screen.findByText(copy)
    expect(line.getAttribute('role')).toBe(role)
    cleanup()
  })

  // A standing rule is recorded after the decision. When it is not, the approval still stands and
  // the card says the rule was not created, so nobody assumes the agent will not ask again.
  it('says a standing rule was not created, and that the approval itself still stands', async () => {
    approvals.readPendingApprovals.mockResolvedValue({
      approvals: [{ id: 'ap-1', detail: 'Run the deploy', toolKey: 'mcp__github__create_issue' }],
      refused: false,
    })
    approvals.settleApproval.mockImplementation(async (input: { onGrant?: (grant: unknown) => void }) => {
      input.onGrant?.({ status: 'not_recorded', reason: 'approval_projection_incomplete' })
      return 'granted'
    })
    render(() => <SpaceApprovalPanel runId="run-1" />)

    ;(await screen.findByRole('button', { name: 'Godkjenn' })).click()

    expect(await screen.findByText(/Godkjent denne gangen, men den faste regelen ble ikke opprettet/)).toBeTruthy()
    expect(screen.getByText('Godkjent. Avgjørelsen er registrert.')).toBeTruthy()
  })
})

// WS10-S8: held tool output. The card says what approving does to the agent, and the held text is
// read only when the person asks for it, drawn as text and never as markup.
describe('SpaceApprovalPanel: held tool output (WS10-S8)', () => {
  const heldApproval = {
    id: 'ap-q',
    detail: 'Output from the web fetch',
    toolKey: 'mcp__web__fetch',
    kind: UNTRUSTED_CONTENT_KIND,
    resumeClass: 'action_continuation' as const,
  }
  const holdCard =
    'Agenten har ikke sett dette innholdet. Godkjenning lar agenten gjenta akkurat dette kallet én gang og få innholdet som ikke-klarert data. Avslag sletter det.'

  it('uses the held-output kind token the gateway sends, and words the consequence for it, whatever the resume class', async () => {
    expect(UNTRUSTED_CONTENT_KIND).toBe('APPROVAL_KIND_UNTRUSTED_CONTENT')
    approvals.readPendingApprovals.mockResolvedValue({ approvals: [heldApproval], refused: false })
    render(() => <SpaceApprovalPanel runId="run-1" />)
    expect(await screen.findByText(holdCard)).toBeTruthy()
    expect(screen.queryByText(/Godkjenning kjører denne handlingen/)).toBeNull()
  })

  it('reads no held text until the person asks for the preview', async () => {
    approvals.readPendingApprovals.mockResolvedValue({ approvals: [heldApproval], refused: false })
    render(() => <SpaceApprovalPanel runId="run-1" />)
    await screen.findByText(holdCard)
    expect(approvals.readQuarantinePreview).not.toHaveBeenCalled()
    expect(screen.getByRole('button', { name: 'Vis en ren tekstforhåndsvisning' })).toBeTruthy()
  })

  it('draws the preview as plain text: markup in the held output stays text', async () => {
    approvals.readPendingApprovals.mockResolvedValue({ approvals: [heldApproval], refused: false })
    approvals.readQuarantinePreview.mockResolvedValue({
      text: '<img src="x" onerror="alert(1)"><b>fet</b>',
      truncated: false,
    })
    const view = render(() => <SpaceApprovalPanel runId="run-1" />)
    fireEvent.click(await screen.findByRole('button', { name: 'Vis en ren tekstforhåndsvisning' }))

    const pre = await waitFor(() => {
      const found = view.container.querySelector('pre')
      expect(found).not.toBeNull()
      return found as HTMLPreElement
    })
    expect(approvals.readQuarantinePreview).toHaveBeenCalledWith('ap-q')
    expect(pre.textContent).toBe('<img src="x" onerror="alert(1)"><b>fet</b>')
    expect(view.container.querySelector('img')).toBeNull()
    expect(view.container.querySelector('b')).toBeNull()
  })

  it('shows that the preview is loading, then says when it was cut short', async () => {
    approvals.readPendingApprovals.mockResolvedValue({ approvals: [heldApproval], refused: false })
    let answer: (value: unknown) => void = () => undefined
    approvals.readQuarantinePreview.mockImplementation(() => new Promise((resolve) => { answer = resolve }))
    render(() => <SpaceApprovalPanel runId="run-1" />)
    fireEvent.click(await screen.findByRole('button', { name: 'Vis en ren tekstforhåndsvisning' }))
    expect(await screen.findByText('Henter forhåndsvisningen …')).toBeTruthy()

    answer({ text: 'første del', truncated: true })
    expect(await screen.findByText('første del')).toBeTruthy()
    expect(screen.getByText('Vist: de første 1024 byte.')).toBeTruthy()
  })

  it('says no preview is available to a person who may not see it, whether the read is empty or refused', async () => {
    approvals.readPendingApprovals.mockResolvedValue({ approvals: [heldApproval], refused: false })
    approvals.readQuarantinePreview.mockResolvedValueOnce(null)
    const first = render(() => <SpaceApprovalPanel runId="run-1" />)
    fireEvent.click(await screen.findByRole('button', { name: 'Vis en ren tekstforhåndsvisning' }))
    expect(await screen.findByText('Ingen forhåndsvisning er tilgjengelig for deg.')).toBeTruthy()
    first.unmount()

    approvals.readQuarantinePreview.mockRejectedValueOnce(new Error('not found'))
    render(() => <SpaceApprovalPanel runId="run-1" />)
    fireEvent.click(await screen.findByRole('button', { name: 'Vis en ren tekstforhåndsvisning' }))
    expect(await screen.findByText('Ingen forhåndsvisning er tilgjengelig for deg.')).toBeTruthy()
  })
})
