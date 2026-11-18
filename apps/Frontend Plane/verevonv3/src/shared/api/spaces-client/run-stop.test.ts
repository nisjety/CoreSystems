import { afterEach, describe, expect, it, vi } from 'vitest'

import { stopSpaceRun } from './run-stop'

type FetchCall = { url: string; init: RequestInit | undefined }

function stubFetch(status: number, body: unknown): FetchCall[] {
  const calls: FetchCall[] = []
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string, init?: RequestInit) => {
      calls.push({ url: String(url), init })
      return new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } })
    }),
  )
  return calls
}

afterEach(() => {
  vi.unstubAllGlobals()
})

const sentBody = (call: FetchCall | undefined) => JSON.parse(String(call?.init?.body)) as Record<string, unknown>

describe('stopSpaceRun (WS09-S8)', () => {
  it('POSTs the encoded run stop path with the idempotency key and parses the receipt', async () => {
    const calls = stubFetch(200, { data: { run_id: 'run/1', status: 'cancelled', cancelled: true, receipt_id: 'rcpt_1' } })
    const receipt = await stopSpaceRun('space 1', 'run/1', 'stop-key-0001')
    expect(calls[0]?.url.endsWith('/api/v1/spaces/space%201/runs/run%2F1/stop')).toBe(true)
    expect(calls[0]?.init?.method).toBe('POST')
    expect(sentBody(calls[0])).toEqual({ idempotency_key: 'stop-key-0001' })
    expect(receipt).toEqual({ runId: 'run/1', status: 'cancelled', cancelled: true, receiptId: 'rcpt_1' })
  })

  it('sends an empty body without a key and reads an already_terminal run as not cancelled', async () => {
    const calls = stubFetch(200, {
      data: { run_id: 'run_2', status: 'already_terminal', cancelled: false, receipt_id: 'rcpt_2' },
    })
    const receipt = await stopSpaceRun('room', 'run_2')
    expect(sentBody(calls[0])).toEqual({})
    expect(receipt).toEqual({ runId: 'run_2', status: 'already_terminal', cancelled: false, receiptId: 'rcpt_2' })
  })
})
