// @vitest-environment jsdom

/**
 * The English-locale leak smoke for the chat surfaces that carry a reader's
 * decisions: approvals, the plan-grant control, artifacts, the live run and the
 * run receipt. Under `en` none of their copy may be Norwegian. Under `no` the
 * same surfaces must still read Norwegian, so the switch is checked both ways.
 *
 * Some copy is still Norwegian under `en` in files outside this smoke's slice:
 * the run receipt (ChatPanels.tsx) and the file-type labels on the file meta
 * line (chat-artifacts.ts). Those strings are pinned below by their exact text.
 * A new Norwegian string fails the smoke, and a pinned string fails it too once
 * it is translated, until it leaves the pin.
 */

import { cleanup, fireEvent, screen } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { Approval } from '@/shared/api/orchestration-client'
import type { ProofBundle, RunEventHandlers } from '@/shared/api/run-console-client'
import { localeStorageKey } from '@/shared/i18n/locales'
import { expectNoNorwegian, renderInLocale } from '@/test/locale-leak'
import { ArtifactsPanel } from './ChatArtifactPanel'
import { mergeArtifactVersion } from './chat-artifacts'
import type { ArtifactPanelItem, ChatArtifact, ChatTurn } from './chat-types'
import { ChatLiveRunPanel } from './ChatLiveRunPanel'
import { ApprovalRequests, PlanApprovalControl, RUNG_LABEL } from './ChatMessages'
import { RunProofPanel } from './ChatPanels'

const hoisted = vi.hoisted(() => ({
  getRunProofBundle: vi.fn(),
  subscriptions: [] as Array<{ handlers: RunEventHandlers; runId: string }>,
}))

// The live panel and the receipt read the run console. A live run's stream never
// settles, so the fixture stream stays open the same way.
vi.mock('@/shared/api/run-console-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/run-console-client')>()),
  getRunProofBundle: (runId: string) => hoisted.getRunProofBundle(runId),
  listRunEventReplay: async () => ({ events: [], nextEventId: null, truncated: false }),
  streamRunEvents: (runId: string, handlers: RunEventHandlers) => {
    hoisted.subscriptions.push({ handlers, runId })
    return new Promise<void>(() => {})
  },
}))

vi.mock('@/shared/api/runs-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/runs-client')>()),
  getRun: () => new Promise(() => {}),
  getRunWatchStatus: async () => ({ watching: false }),
  unwatchRun: async () => undefined,
  watchRun: async () => ({ watching: true }),
}))

const APPROVALS: Approval[] = [
  { id: 'approval-1', kind: 'tool', detail: 'Write the supplier summary to the workspace' },
  { id: 'approval-2' },
]

const PLAN_STEPS = [
  { id: 'step-1', title: 'Collect the open invoices', operation: 'read' },
  { id: 'step-2', title: 'Draft the summary' },
]

const RECEIPT: ProofBundle = {
  bundleVersion: 1,
  runId: 'run_abc123',
  effectClass: 'effectful',
  run: { goal: 'Send the supplier reminder', status: 'completed' },
  approvals: [{
    approvalId: 'approval-1',
    kind: 'tool',
    status: 'approved',
    execution: {
      receiptId: 'rcpt_0001',
      outcome: { outcome: 'completed', verification: { status: 'verified_success' } },
    },
  }],
  unavailable: [{ section: 'cost', reason: 'Not recorded for this run.' }],
}

// Nothing durable was recorded, so every fallback the receipt has is on screen.
const SPARSE_RECEIPT: ProofBundle = {
  effectClass: 'unknown',
  run: { status: 'completed' },
  approvals: [{ execution: null }],
  unavailable: [],
}

// Copy in the run receipt (ChatPanels.tsx) that is still Norwegian under `en`.
// The header is on every receipt state; the rest is on a receipt with an effect
// or with missing evidence.
const RECEIPT_HEADER_LEAKS = ['Kvittering', 'Oppdater']
const RECEIPT_EFFECT_LEAKS = [
  ...RECEIPT_HEADER_LEAKS,
  'Verifisert',
  'Effektklasse',
  'Utfall',
  'Effekt forsøkt',
  'Utfallsstatus',
  'Verifisering',
  'Noen deler av utfallet er ikke dekket av denne pakken.',
]
const SPARSE_RECEIPT_LEAKS = [
  ...RECEIPT_HEADER_LEAKS,
  'Verifisert',
  'Effektklasse',
  'Ikke fastslått',
  'Utfall',
  'Arbeidskjøring',
  'registrert',
]

function turn(overrides: Partial<ChatTurn> = {}): ChatTurn {
  return {
    attachments: [],
    content: 'Make the artifact',
    createdAt: '2026-10-22T10:00:00.000Z',
    id: 'turn-1',
    role: 'assistant',
    streaming: false,
    tools: [],
    ...overrides,
  }
}

function item(artifact: ChatArtifact): ArtifactPanelItem {
  return { artifact, turn: turn({ artifacts: [artifact] }) }
}

function versionedDocument(): ChatArtifact {
  const revisions = [
    { content: '# Supplier summary\n\nFirst draft.', version: 1 },
    { content: '# Supplier summary\n\nRevised after the review.', version: 2 },
  ]
  return revisions.reduce<ChatArtifact | undefined>(
    (carried, revision) => mergeArtifactVersion(carried, {
      ...revision,
      id: 'artifact-doc',
      kind: 'document',
      title: 'Supplier summary',
    }),
    undefined,
  ) as ChatArtifact
}

/** Lets the resource and stream promises settle, then flushes Solid's queued updates. */
async function settle(): Promise<void> {
  for (let step = 0; step < 3; step += 1) {
    await new Promise<void>((resolve) => setTimeout(resolve, 0))
  }
  flush()
}

/** The live tail attaches after the replay page resolves, so wait for it. */
async function subscribed(): Promise<void> {
  for (let attempt = 0; attempt < 20 && hoisted.subscriptions.length === 0; attempt += 1) {
    await settle()
  }
  expect(hoisted.subscriptions.length).toBeGreaterThan(0)
}

beforeEach(() => {
  hoisted.subscriptions.length = 0
  hoisted.getRunProofBundle.mockReset().mockResolvedValue(null)
})

afterEach(() => {
  cleanup()
  window.localStorage.removeItem(localeStorageKey)
})

describe('approval block', () => {
  it('names the decisions and the fallback copy in English under en', () => {
    renderInLocale('en', () => <ApprovalRequests approvals={APPROVALS} onDecide={() => {}} />)

    expect(screen.getAllByRole('button', { name: 'Approve' })).toHaveLength(APPROVALS.length)
    expect(screen.getAllByRole('button', { name: 'Reject' })).toHaveLength(APPROVALS.length)
    expectNoNorwegian(document.body)
  })

  it('keeps the Norwegian decisions under no', () => {
    renderInLocale('no', () => <ApprovalRequests approvals={APPROVALS} onDecide={() => {}} />)

    expect(screen.getAllByRole('button', { name: 'Godkjenn' })).toHaveLength(APPROVALS.length)
    expect(screen.getAllByRole('button', { name: 'Avvis' })).toHaveLength(APPROVALS.length)
  })
})

describe('plan-approval control', () => {
  const structuredPlan = {
    planId: 'plan-1',
    summary: 'Prepare the supplier reminder',
    steps: PLAN_STEPS,
    pending: false,
    onApprove: () => {},
    onReject: () => {},
  }

  it('offers the structured plan, the grant and the reason in English under en', () => {
    renderInLocale('en', () => <PlanApprovalControl {...structuredPlan} />)

    expect(screen.getByRole('radio', { name: 'Write in the workspace' })).toBeTruthy()
    expect(screen.getByRole('radio', { name: 'Do anything, including outbound actions' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Reject the plan' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Approve' })).toBeTruthy()
    expect(screen.getByText('At least 12 characters – a reason someone can assess.')).toBeTruthy()
    expectNoNorwegian(document.body)
  })

  it('says the pending approval and the granted authority in English under en', () => {
    renderInLocale('en', () => (
      <PlanApprovalControl pending onApprove={() => {}} />
    ))
    expect(screen.getByRole('button', { name: 'Approving…' })).toBeTruthy()
    expectNoNorwegian(document.body)
  })

  it('states a granted rung as one English sentence', () => {
    renderInLocale('en', () => (
      <PlanApprovalControl grantedRung="danger_full_access" pending={false} onApprove={() => {}} />
    ))
    expect(screen.getByText('Approved: the agent can now do anything, including outbound actions.')).toBeTruthy()
    expectNoNorwegian(document.body)
  })

  it('says a rejected plan in English', () => {
    renderInLocale('en', () => (
      <PlanApprovalControl rejected pending={false} onApprove={() => {}} />
    ))
    expect(screen.getByText('The plan was rejected – nothing was executed.')).toBeTruthy()
    expectNoNorwegian(document.body)
  })

  it('keeps the Norwegian plan wording under no', () => {
    renderInLocale('no', () => <PlanApprovalControl {...structuredPlan} />)

    expect(screen.getByRole('radio', { name: 'skrive i arbeidsområdet' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Godkjenn' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Avvis planen' })).toBeTruthy()
  })
})

describe('artifacts', () => {
  it('names the empty panel in English under en', () => {
    renderInLocale('en', () => <ArtifactsPanel items={[]} />)

    expect(screen.getByText('No artifacts yet')).toBeTruthy()
    expectNoNorwegian(document.body)
  })

  it('reads a revised document, its version stepper and its export menu in English', () => {
    renderInLocale('en', () => <ArtifactsPanel items={[item(versionedDocument())]} />)

    expect(screen.getByRole('button', { name: 'Previous version' })).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Export the document' }))
    flush()
    expect(screen.getByRole('button', { name: 'Export to Word' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Save as PDF' })).toBeTruthy()
    expectNoNorwegian(document.body, ['Markdown-dokument'])
  })

  it('reads a code artifact in English, except its pinned MIME label', () => {
    const code: ChatArtifact = {
      content: 'def hello():\n    return 1\n',
      id: 'artifact-code',
      kind: 'code',
      title: 'solve.py',
      version: 1,
    }
    renderInLocale('en', () => <ArtifactsPanel items={[item(code)]} />)

    expectNoNorwegian(document.body, ['Tekstfil'])
  })

  it('names the HTML preview and its code toggle in English, except its pinned MIME label', () => {
    const page: ChatArtifact = {
      content: '<h1>Hello</h1>',
      id: 'artifact-html',
      kind: 'html',
      title: 'index.html',
      version: 1,
    }
    renderInLocale('en', () => <ArtifactsPanel items={[item(page)]} />)

    fireEvent.click(screen.getByText('Show code'))
    flush()
    expect(screen.getByText('Show preview')).toBeTruthy()
    expectNoNorwegian(document.body, ['HTML-dokument'])
  })

  it('reads a generated spreadsheet card in English, except its pinned MIME label', () => {
    const href = `data:application/vnd.openxmlformats-officedocument.spreadsheetml.sheet;base64,${btoa('sheet')}`
    const sheet: ChatArtifact = {
      content: href,
      id: 'artifact-sheet',
      kind: 'spreadsheet',
      title: 'Q4-figures',
      version: 1,
    }
    renderInLocale('en', () => <ArtifactsPanel items={[item(sheet)]} />)

    expect(screen.getAllByText('Download').length).toBeGreaterThan(0)
    expectNoNorwegian(document.body, ['Excel-regneark (XLSX)'])
  })

  it('says in English that an announced artifact could not be loaded', () => {
    const announced: ChatArtifact = {
      content: '',
      id: 'artifact-missing',
      kind: 'spreadsheet',
      title: 'Q4.xlsx',
      version: 1,
    }
    renderInLocale('en', () => <ArtifactsPanel items={[item(announced)]} />)

    expect(screen.getByText('The artifact could not be loaded')).toBeTruthy()
    expectNoNorwegian(document.body)
  })

  it('keeps the Norwegian artifact wording under no', () => {
    renderInLocale('no', () => <ArtifactsPanel items={[item(versionedDocument())]} />)

    expect(screen.getByRole('button', { name: 'Forrige versjon' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Eksporter dokumentet' })).toBeTruthy()
  })
})

describe('live run', () => {
  it('words the waiting and interrupted states in English under en', async () => {
    renderInLocale('en', () => (
      <ChatLiveRunPanel collapsed={false} onToggleCollapsed={() => {}} runId="run_abc123" zdr={false} />
    ))
    await subscribed()
    flush()

    expect(screen.getByText('Waiting for the agent')).toBeTruthy()
    expectNoNorwegian(document.body)

    hoisted.subscriptions[0]?.handlers.onError?.(new Error('boom'))
    flush()

    expect(screen.getByText('The event stream for this run could not be read.')).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Reconnect' })).toBeTruthy()
    expectNoNorwegian(document.body)
  })

  it('names the collapsed live-panel toggle in English', () => {
    renderInLocale('en', () => (
      <ChatLiveRunPanel collapsed onToggleCollapsed={() => {}} runId="run_abc123" zdr={false} />
    ))

    expect(screen.getByRole('button', { name: 'Show the live panel' })).toBeTruthy()
    expectNoNorwegian(document.body)
  })

  it('keeps the Norwegian live-run wording under no', async () => {
    renderInLocale('no', () => (
      <ChatLiveRunPanel collapsed={false} onToggleCollapsed={() => {}} runId="run_abc123" zdr={false} />
    ))
    await subscribed()
    flush()

    expect(screen.getByText('Venter på agenten')).toBeTruthy()
  })
})

describe('run receipt', () => {
  it('reads the loading state in English', () => {
    hoisted.getRunProofBundle.mockReturnValue(new Promise(() => {}))
    renderInLocale('en', () => <RunProofPanel runId="run_abc123" />)

    expect(screen.getByText('Loading the run receipt …')).toBeTruthy()
    expectNoNorwegian(document.body, RECEIPT_HEADER_LEAKS)
  })

  it('reads the empty state in English', async () => {
    renderInLocale('en', () => <RunProofPanel runId="run_abc123" />)
    await settle()

    expect(screen.getByText('No durable receipt has been recorded yet.')).toBeTruthy()
    expectNoNorwegian(document.body, RECEIPT_HEADER_LEAKS)
  })

  it('reads the failed state in English', async () => {
    hoisted.getRunProofBundle.mockRejectedValue(new Error('network'))
    renderInLocale('en', () => <RunProofPanel runId="run_abc123" />)
    await settle()

    expect(screen.getByText('The receipt could not be loaded right now.')).toBeTruthy()
    expectNoNorwegian(document.body, RECEIPT_HEADER_LEAKS)
  })

  it('reads a receipt with a verified effect in English, except its pinned copy', async () => {
    hoisted.getRunProofBundle.mockResolvedValue(RECEIPT)
    renderInLocale('en', () => <RunProofPanel runId="run_abc123" />)
    await settle()

    expect(screen.getByText('Send the supplier reminder')).toBeTruthy()
    expect(screen.getByText('Approvals')).toBeTruthy()
    expectNoNorwegian(document.body, RECEIPT_EFFECT_LEAKS)
  })

  it('reads the fallback copy of a sparse receipt in English, except its pinned copy', async () => {
    hoisted.getRunProofBundle.mockResolvedValue(SPARSE_RECEIPT)
    renderInLocale('en', () => <RunProofPanel runId="run_abc123" />)
    await settle()

    expectNoNorwegian(document.body, SPARSE_RECEIPT_LEAKS)
  })

  it('keeps the Norwegian receipt wording under no', async () => {
    hoisted.getRunProofBundle.mockResolvedValue(null)
    renderInLocale('no', () => <RunProofPanel runId="run_abc123" />)
    await settle()

    expect(screen.getByText('Ingen varig kvittering er registrert ennå.')).toBeTruthy()
  })
})

describe('the Norwegian check', () => {
  it('fails on the Norwegian grant name the control showed before WS12-S3', () => {
    const { container } = renderInLocale('en', () => <p>{RUNG_LABEL.workspace_write}</p>)

    expect(() => expectNoNorwegian(container)).toThrow()
  })

  it('reads accessible names as well as text', () => {
    const { container } = renderInLocale('en', () => (
      <button type="button" aria-label="Lukk arbeidsflaten">Close</button>
    ))

    expect(() => expectNoNorwegian(container)).toThrow()
  })

  it('catches a Norwegian file-type label such as the one a spreadsheet card showed', () => {
    const { container } = renderInLocale('en', () => <span>Excel-regneark (XLSX)</span>)

    expect(() => expectNoNorwegian(container)).toThrow()
  })

  it('passes English copy', () => {
    const { container } = renderInLocale('en', () => (
      <button type="button" aria-label="Close the workspace">Close</button>
    ))

    expect(() => expectNoNorwegian(container)).not.toThrow()
  })
})
