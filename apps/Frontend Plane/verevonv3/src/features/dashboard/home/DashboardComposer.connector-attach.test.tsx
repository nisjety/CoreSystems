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
