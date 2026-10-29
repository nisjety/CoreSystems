// @vitest-environment jsdom

import { cleanup, render, screen } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { OutboundDeliveryLedger } from './OutboundDeliveryLedger'

function jsonResponse(data: unknown) {
  return new Response(JSON.stringify({ data }), { headers: { 'Content-Type': 'application/json' } })
}

afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
})

describe('OutboundDeliveryLedger', () => {
  it('distinguishes provider delivery/read evidence from provider acceptance', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse([
      {
        id: 'intent-delivered', conversation_id: 'conv_1', status: 'submitted', provider: 'whatsapp',
        delivery_status: 'delivered', created_at: '2026-08-03T00:00:00.000Z', updated_at: '2026-08-03T00:01:00.000Z',
      },
      {
        id: 'intent-read', conversation_id: 'conv_1', status: 'submitted', provider: 'messenger',
        delivery_status: 'read', created_at: '2026-08-03T00:00:00.000Z', updated_at: '2026-08-03T00:02:00.000Z',
      },
      {
        id: 'intent-unconfirmed', conversation_id: 'conv_1', status: 'submitted', provider: 'gmail',
        delivery_status: 'unconfirmed', created_at: '2026-08-03T00:00:00.000Z', updated_at: '2026-08-03T00:03:00.000Z',
      },
    ])))

    render(() => <OutboundDeliveryLedger orgId="org_1" conversationId="conv_1" refreshKey={0} />)

    expect(await screen.findByText(/Leverandøren rapporterer levering via whatsapp/i)).toBeTruthy()
    expect(screen.getByText(/Leverandøren rapporterer at meldingen er lest via messenger/i)).toBeTruthy()
    expect(screen.getByText(/Sendt til gmail; leverandøren godtok forespørselen/i)).toBeTruthy()
    expect(screen.queryByText(/kund.*mottok|customer received/i)).toBeNull()
  })

  it('shows a provider delivery failure without erasing the earlier acceptance boundary', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse([{
      id: 'intent-failed', conversation_id: 'conv_1', status: 'submitted', provider: 'whatsapp',
      delivery_status: 'failed', created_at: '2026-08-03T00:00:00.000Z', updated_at: '2026-08-03T00:02:00.000Z',
    }])))

    render(() => <OutboundDeliveryLedger orgId="org_1" conversationId="conv_1" refreshKey={0} />)

    expect(await screen.findByText(/ikke ble levert via whatsapp/i)).toBeTruthy()
    expect(screen.getByText(/Leverandøraksept var tidligere registrert/i)).toBeTruthy()
  })
})
