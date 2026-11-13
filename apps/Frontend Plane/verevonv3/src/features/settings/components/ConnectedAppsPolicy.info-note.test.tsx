// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { apiErrorFromResponse } from '@/shared/api/http'
import type { ConnectorPolicies } from '@/shared/api/orchestration-client'

// W3-EP-10-S7-a: capability-core refuses an "ask" on the provider write with the typed envelope
// {error:{code:'already_always_ask'}} (422). The panel reads that as an info note (what already happens),
// not as a failure; any other refusal stays an error.

const client = vi.hoisted(() => ({ listConnectorPolicies: vi.fn(), setConnectorPolicy: vi.fn() }))
vi.mock('@/shared/api/orchestration-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/orchestration-client')>()),
  listConnectorPolicies: client.listConnectorPolicies,
  setConnectorPolicy: client.setConnectorPolicy,
}))

const { ConnectedAppsPolicy } = await import('./ConnectedAppsPolicy')

const policies: ConnectorPolicies = {
  enforced: true,
  read: 'use_without_asking',
  write: 'always_ask',
  orgRead: 'use_without_asking',
  orgWrite: 'always_ask',
}

/** The ApiError the transport builds from capability-core's typed 422 body, as the gateway relays it. */
function typedRefusal(code: string, status = 422) {
  return apiErrorFromResponse(
    { error: { code, message: 'Every action with this app already asks you first.', details: {} } },
    { status, headers: new Headers(), url: 'http://localhost/api/v1/tool-policies/cap.tool.provider.read' },
  )
}

async function changeRead(level: string) {
  render(() => <ConnectedAppsPolicy />)
  await waitFor(() => expect(document.getElementById('connected-apps-policy-read')).not.toBeNull())
  fireEvent.change(document.getElementById('connected-apps-policy-read') as HTMLSelectElement, {
    target: { value: level },
  })
  flush()
}

beforeEach(() => {
  client.listConnectorPolicies.mockReset().mockResolvedValue(policies)
  client.setConnectorPolicy.mockReset()
})

afterEach(() => cleanup())

describe('ConnectedAppsPolicy already_always_ask', () => {
  it('reads the typed envelope code', () => {
    expect(typedRefusal('already_always_ask').code).toBe('already_always_ask')
  })

  it('shows an info note, not an error, for already_always_ask', async () => {
    client.setConnectorPolicy.mockRejectedValue(typedRefusal('already_always_ask'))
    await changeRead('ask_first')
    const note = await screen.findByText(/spør deg allerede|already asks you before every action/)
    expect(note.getAttribute('role')).toBe('note')
    expect(screen.queryByRole('alert')).toBeNull()
  })

  it('shows any other refusal as an error, with no info note', async () => {
    client.setConnectorPolicy.mockRejectedValue(typedRefusal('policy_conflict', 409))
    await changeRead('ask_first')
    await screen.findByRole('alert')
    expect(screen.queryByText(/spør deg allerede|already asks you before every action/)).toBeNull()
  })

  it('clears the note on the next change', async () => {
    client.setConnectorPolicy.mockRejectedValueOnce(typedRefusal('already_always_ask')).mockResolvedValueOnce(undefined)
    await changeRead('ask_first')
    await screen.findByText(/spør deg allerede|already asks you before every action/)
    fireEvent.change(document.getElementById('connected-apps-policy-read') as HTMLSelectElement, {
      target: { value: 'off' },
    })
    flush()
    await waitFor(() =>
      expect(screen.queryByText(/spør deg allerede|already asks you before every action/)).toBeNull(),
    )
  })
})
