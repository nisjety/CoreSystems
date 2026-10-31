// @vitest-environment jsdom
import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

vi.mock('@/shared/api/browser-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/browser-client')>()),
  listBrowserEgressReceipts: vi.fn(),
  listBrowserReceipts: vi.fn(),
  compileBrowserProcedure: vi.fn(),
}))
vi.mock('@/shared/api/auth-client', () => ({
  getSessionContext: vi.fn(async () => ({ userId: 'user-1', orgs: [{ id: 'org-1', name: 'Org', role: 'owner' }] })),
}))

import { ApiError } from '@/shared/api/http'
import { compileBrowserProcedure, listBrowserReceipts } from '@/shared/api/browser-client'
import { ProcedurePanel } from './ProcedurePanel'

const compiled = {
  procedure: {
    procedureId: 'proc-1',
    version: 1,
    sourceReceiptIds: ['r1', 'r2'],
    steps: [
      { step: 1, action: { type: 'navigate', url: 'https://example.com/' }, expectedOutcome: 'page loads' },
      { step: 2, action: { type: 'type', text: { redacted: true, length: 6 } }, expectedOutcome: 'field filled' },
    ],
  },
  quality: { stepCount: 2, verifiedStepCount: 2, promotionAllowed: true, reasons: [] },
}

function compileButton(): HTMLButtonElement {
  return document.querySelector('.browser-evidence-panel__action') as HTMLButtonElement
}

beforeEach(() => {
  vi.mocked(compileBrowserProcedure).mockReset()
  vi.mocked(listBrowserReceipts).mockReset()
  vi.mocked(listBrowserReceipts).mockResolvedValue({ receipts: [], truncated: false, zdr: false })
})

afterEach(() => vi.restoreAllMocks())

describe('ProcedurePanel', () => {
  it('is disabled for a ZDR session and sends no request', async () => {
    render(() => <ProcedurePanel sessionId="session-1" zdr={true} />)

    expect(compileButton().disabled).toBe(true)
    fireEvent.click(compileButton())
    expect(compileBrowserProcedure).not.toHaveBeenCalled()
    expect(listBrowserReceipts).not.toHaveBeenCalled()
  })

  it('shows the typed 409 reason as text, not the raw server message', async () => {
    vi.mocked(compileBrowserProcedure).mockRejectedValue(
      new ApiError('raw server text', 409, 'browser_procedure_not_compilable'),
    )
    render(() => <ProcedurePanel sessionId="session-1" zdr={false} />)

    await waitFor(() => expect(compileButton().disabled).toBe(false))
    fireEvent.click(compileButton())
    const alert = await screen.findByRole('alert')
    expect(alert.textContent).toMatch(/kompileres|compiled/)
    expect(alert.textContent).not.toContain('raw server text')
  })

  it('shows the quality verdict and never a typed value', async () => {
    vi.mocked(compileBrowserProcedure).mockResolvedValue(compiled)
    render(() => <ProcedurePanel sessionId="session-1" zdr={false} />)

    await waitFor(() => expect(compileButton().disabled).toBe(false))
    fireEvent.click(compileButton())
    expect(await screen.findByText(/Klar for promotering|Ready to promote/)).toBeTruthy()
    expect(screen.getByText(/skjult tekst \(6 tegn\)|hidden text \(6 characters\)/)).toBeTruthy()
    expect(document.body.textContent ?? '').not.toContain('hunter2')
  })
})
