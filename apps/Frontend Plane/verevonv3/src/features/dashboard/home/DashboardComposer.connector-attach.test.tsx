// @vitest-environment jsdom

// Connector-attach picker ("Fra en tilkoblet kilde") — the composer's new
// settings-menu row that lets a person attach a OneDrive/Google Drive file,
// an Outlook/Gmail message, or a calendar event straight from a connected
// source. See plans/system-audits/chat-wave2-specs-2026-09-28/connector-picker.md.
// Mirrors DashboardComposer.paste-attachment.test.tsx's render/mock harness.

import { createRouter, memoryHistory } from '@solidjs/router'
import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { createSignal, flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'

const { mockListConnectorPickerTabs, mockBrowseConnection, mockFetchConnectionContent } = vi.hoisted(() => ({
  mockListConnectorPickerTabs: vi.fn(),
  mockBrowseConnection: vi.fn(),
  mockFetchConnectionContent: vi.fn(),
}))

vi.mock('@/shared/api/chat-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-client')>()
  return {
    ...actual,
    listModels: vi.fn().mockResolvedValue([]),
    listChatThreads: vi.fn().mockResolvedValue([]),
    saveChatThreadSnapshot: vi.fn().mockResolvedValue(null),
  }
})

vi.mock('@/shared/api/chat-actions-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-actions-client')>()
  return { ...actual, loadSpecializedActions: vi.fn().mockResolvedValue(actual.BUILTIN_ACTIONS) }
})

vi.mock('@/shared/api/integrations-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/integrations-client')>()
  return {
    ...actual,
    listConnectorPickerTabs: mockListConnectorPickerTabs,
    browseConnection: mockBrowseConnection,
    fetchConnectionContent: mockFetchConnectionContent,
  }
})

vi.mock('@/shared/session/session-store', () => ({
  getSession: () => ({ activeOrg: { id: 'org-1', name: 'Test', role: 'admin' } }),
}))

import { ApiError } from '@/shared/api/http'
import { DashboardComposer } from './DashboardComposer'

function ComposerPage(props: { temporaryChat?: boolean }) {
  const [message, setMessage] = createSignal('')
  return (
    <DashboardComposer
      message={message()}
      onMessageChange={setMessage}
      onSubmit={() => {}}
      temporaryChat={props.temporaryChat}
    />
  )
}

function renderComposer(temporaryChat?: boolean) {
  const TestRouter = createRouter({
    routes: [{ path: '/*all', component: () => <ComposerPage temporaryChat={temporaryChat} /> }],
    history: memoryHistory(),
    explicitLinks: true,
  })
  return render(() => <TestRouter>{(props) => <>{props.children}</>}</TestRouter>)
}

async function openConnectorPicker() {
  fireEvent.click(screen.getByRole('button', { name: 'Innstillinger' }))
  flush()
  const row = await screen.findByText('Fra en tilkoblet kilde')
  fireEvent.click(row)
  flush()
}

const oneMicrosoftTab = [{ connectionId: 'conn-ms-1', providerKey: 'microsoft' as const, label: 'OneDrive (Ima)' }]

afterEach(() => {
  vi.clearAllMocks()
  flush()
})

describe('opening the picker and searching', () => {
  it('lists an active connection as a tab and posts the debounced query to /browse', async () => {
    mockListConnectorPickerTabs.mockResolvedValue(oneMicrosoftTab)
    mockBrowseConnection.mockResolvedValue({ providerKey: 'microsoft', operation: 'drive.files', result: { value: [] } })

    renderComposer()
    await openConnectorPicker()

    await screen.findByText('OneDrive (Ima)')

    const search = screen.getByPlaceholderText('Søk ...')
    fireEvent.input(search, { target: { value: 'rapport' } })

    // Debounced ~300ms — not sent immediately.
    expect(mockBrowseConnection).not.toHaveBeenCalledWith('org-1', 'conn-ms-1', 'drive.files', { query: 'rapport' })

    await waitFor(
      () => expect(mockBrowseConnection).toHaveBeenCalledWith('org-1', 'conn-ms-1', 'drive.files', { query: 'rapport' }),
      { timeout: 2000 },
    )
  })
})

describe('picking a result', () => {
  it('fetches content and adds a chip carrying source:"onedrive"', async () => {
    mockListConnectorPickerTabs.mockResolvedValue(oneMicrosoftTab)
    mockBrowseConnection.mockResolvedValue({
      providerKey: 'microsoft',
      operation: 'drive.files',
      result: { value: [{ id: 'item-1', name: 'Rapport.pdf', size: 12_345, file: { mimeType: 'application/pdf' } }] },
    })
    mockFetchConnectionContent.mockResolvedValue({
      providerKey: 'microsoft',
      operation: 'drive.file.content',
      result: { content_base64: btoa('PDF bytes'), mime_type: 'application/pdf', size: 9, truncated: false },
    })

    renderComposer()
    await openConnectorPicker()

    const row = await screen.findByText('Rapport.pdf')
    fireEvent.click(row)
    flush()

    await waitFor(() => expect(mockFetchConnectionContent).toHaveBeenCalledWith('org-1', 'conn-ms-1', 'drive.file.content', { itemId: 'item-1' }))

    const chip = await waitFor(() => {
      const found = document.querySelector('[data-attachment-source="onedrive"]')
      if (!found) throw new Error('expected a chip carrying data-attachment-source="onedrive"')
      return found
    })
    expect(chip.textContent).toContain('Rapport.pdf')
  })

  it('disables the picker and shows the ZDR message during a temporary chat', async () => {
    mockListConnectorPickerTabs.mockResolvedValue(oneMicrosoftTab)

    renderComposer(true)
    await openConnectorPicker()

    expect(await screen.findByText('Ikke tilgjengelig i midlertidig chat.')).toBeTruthy()
    expect(screen.queryByText('OneDrive (Ima)')).toBeNull()
    expect(mockListConnectorPickerTabs).not.toHaveBeenCalled()
  })

  it('greys out and disables an over-cap file so it cannot be picked', async () => {
    mockListConnectorPickerTabs.mockResolvedValue(oneMicrosoftTab)
    mockBrowseConnection.mockResolvedValue({
      providerKey: 'microsoft',
      operation: 'drive.files',
      result: {
        value: [{ id: 'item-big', name: 'Arkiv.pdf', size: 5_000_000, file: { mimeType: 'application/pdf' } }],
      },
    })

    renderComposer()
    await openConnectorPicker()

    const row = (await screen.findByText('Arkiv.pdf')).closest('button')
    expect(row).toBeTruthy()
    expect(row?.disabled).toBe(true)

    fireEvent.click(row!)
    flush()
    expect(mockFetchConnectionContent).not.toHaveBeenCalled()
  })
})

// Owner ruling (security-personal-connections, 2026-09-28): a connection can
// stop being the caller's own mid-session (e.g. picked from a stale tab
// after ownership changed) — integration-corev2 then answers 403 with one of
// two stable codes, and the picker must show its OWN specific Norwegian
// message, never the generic "stale pick"/"could not search" copy and never
// the raw English gateway text.
describe('personal connection ownership errors', () => {
  it('shows the ownership-specific message when picking an item whose connection is no longer the caller’s own', async () => {
    mockListConnectorPickerTabs.mockResolvedValue(oneMicrosoftTab)
    mockBrowseConnection.mockResolvedValue({
      providerKey: 'microsoft',
      operation: 'drive.files',
      result: { value: [{ id: 'item-1', name: 'Rapport.pdf', size: 12_345, file: { mimeType: 'application/pdf' } }] },
    })
    mockFetchConnectionContent.mockRejectedValueOnce(
      new ApiError('This connection belongs to a different user and cannot be used here.', 403, 'personal_connection_forbidden'),
    )

    renderComposer()
    await openConnectorPicker()

    const row = await screen.findByText('Rapport.pdf')
    fireEvent.click(row)
    flush()

    await screen.findByText(/tilhører en annen bruker/i)
    // Not the generic 404 "stale pick" copy, and no chip was attached.
    expect(screen.queryByText(/ikke lenger tilgjengelig/i)).toBeNull()
    expect(document.querySelector('[data-attachment-source="onedrive"]')).toBeNull()
  })

  it('shows the no-delegated-user message instead of the generic search failure', async () => {
    mockListConnectorPickerTabs.mockResolvedValue(oneMicrosoftTab)
    mockBrowseConnection.mockRejectedValue(
      new ApiError(
        'This personal connection requires you to be signed in as its owner.',
        403,
        'personal_connection_no_delegated_user',
      ),
    )

    renderComposer()
    await openConnectorPicker()

    await screen.findByText(/logget inn som eieren/i)
    expect(screen.queryByText('Kunne ikke søke.')).toBeNull()
  })
})

// Pre-flight readability check (connector-picker spec §6.2): a picked PDF or Office file is read at pick time. A refusal
// that names its reason (a code, or a 401, 403 or 429 with its own copy) keeps that reason. Any other refusal, including
// imports-core's code-less {detail} answers, gets this file's read line, as does a failure nobody answered. Nothing is
// attached in any of these cases.
describe('a refusal from the pre-flight read', () => {
  // The extraction route answers with the gateway envelope; every other request keeps the setup's empty 200.
  function extractionAnswers(reply: () => Response | Error) {
    vi.stubGlobal(
      'fetch',
      vi.fn(async (input: RequestInfo | URL) => {
        if (!String(input).includes('/api/v1/chat/documents/extract')) {
          return new Response('{}', { status: 200, headers: { 'Content-Type': 'application/json' } })
        }
        const answer = reply()
        if (answer instanceof Error) throw answer
        return answer
      }),
    )
  }

  function envelope(status: number, error: Record<string, unknown>): () => Response {
    return () => new Response(JSON.stringify({ error }), { status, headers: { 'Content-Type': 'application/json' } })
  }

  // imports-core's read refusals: FastAPI's {detail} body, with no code.
  function detailAnswer(status: number, detail: string): () => Response {
    return () => new Response(JSON.stringify({ detail }), { status, headers: { 'Content-Type': 'application/json' } })
  }

  async function pickRapport() {
    mockListConnectorPickerTabs.mockResolvedValue(oneMicrosoftTab)
    mockBrowseConnection.mockResolvedValue({
      providerKey: 'microsoft',
      operation: 'drive.files',
      result: { value: [{ id: 'item-1', name: 'Rapport.pdf', size: 12_345, file: { mimeType: 'application/pdf' } }] },
    })
    mockFetchConnectionContent.mockResolvedValue({
      providerKey: 'microsoft',
      operation: 'drive.file.content',
      result: { content_base64: btoa('PDF bytes'), mime_type: 'application/pdf', size: 9, truncated: false },
    })

    renderComposer()
    await openConnectorPicker()
    fireEvent.click(await screen.findByText('Rapport.pdf'))
    flush()
  }

  function expectNothingAttached() {
    expect(document.querySelector('[data-attachment-source="onedrive"]')).toBeNull()
  }

  it('shows a zero-retention refusal of the read in its own words', async () => {
    extractionAnswers(envelope(403, { code: 'zdr_feature_not_supported', message: 'Zero retention', feature: 'web_search' }))
    await pickRapport()

    await screen.findByText(/Dette støttes ikke med Zero Data Retention/)
    expect(screen.queryByText(/Kunne ikke lese/)).toBeNull()
    expectNothingAttached()
  })

  it('shows the service outage of the read in its own words', async () => {
    // The gateway's own typed 502, sent when imports-core cannot be reached.
    extractionAnswers(envelope(502, { code: 'upstream_unavailable', message: 'The upstream service is unavailable.' }))
    await pickRapport()

    await screen.findByText('Tjenesten er utilgjengelig akkurat nå. Prøv igjen om litt.')
    expect(screen.queryByText(/Kunne ikke lese/)).toBeNull()
    expectNothingAttached()
  })

  it('shows the rate limit with its retry time', async () => {
    extractionAnswers(envelope(429, { code: 'rate_limited', message: 'Slow down', retryAfterSeconds: 30 }))
    await pickRapport()

    await screen.findByText('Det kommer for mange forespørsler akkurat nå. Prøv igjen om 30 sekunder.')
    expect(screen.queryByText(/Kunne ikke lese/)).toBeNull()
    expectNothingAttached()
  })

  it('reads a scanned PDF that imports-core refuses without a code as a failed read', async () => {
    extractionAnswers(detailAnswer(422, 'No readable text found in Rapport.pdf'))
    await pickRapport()

    await screen.findByText('Kunne ikke lese «Rapport.pdf». Prøv igjen eller velg en annen fil.')
    expect(screen.queryByText(/Noe gikk galt/)).toBeNull()
    expectNothingAttached()
  })

  it('reads a code-less 404 as a failed read, not as a stale pick', async () => {
    extractionAnswers(detailAnswer(404, 'Not Found'))
    await pickRapport()

    await screen.findByText('Kunne ikke lese «Rapport.pdf». Prøv igjen eller velg en annen fil.')
    expect(screen.queryByText(/ikke lenger tilgjengelig/)).toBeNull()
    expectNothingAttached()
  })

  it('reads a code-less 413 as a failed read', async () => {
    extractionAnswers(detailAnswer(413, 'Document too large'))
    await pickRapport()

    await screen.findByText('Kunne ikke lese «Rapport.pdf». Prøv igjen eller velg en annen fil.')
    expectNothingAttached()
  })

  it('keeps the rate-limit copy for a code-less 429', async () => {
    extractionAnswers(detailAnswer(429, 'Too many requests'))
    await pickRapport()

    await screen.findByText('Det kommer for mange forespørsler akkurat nå. Prøv igjen om litt.')
    expect(screen.queryByText(/Kunne ikke lese/)).toBeNull()
    expectNothingAttached()
  })

  it('reads a code-less 410 as a failed read, since a gone status has no copy without a code', async () => {
    // A 410 without a code has no copy of its own (a typed session-lost code does), so the read line is the honest one.
    extractionAnswers(detailAnswer(410, 'Gone'))
    await pickRapport()

    await screen.findByText('Kunne ikke lese «Rapport.pdf». Prøv igjen eller velg en annen fil.')
    expectNothingAttached()
  })

  it('keeps the generic read refusal for a failure nobody typed', async () => {
    extractionAnswers(() => new TypeError('Failed to fetch'))
    await pickRapport()

    await screen.findByText('Kunne ikke lese «Rapport.pdf». Prøv igjen eller velg en annen fil.')
    expectNothingAttached()
  })
})
