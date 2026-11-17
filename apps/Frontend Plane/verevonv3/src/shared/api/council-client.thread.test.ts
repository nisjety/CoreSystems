import { afterEach, describe, expect, it, vi, type Mock } from 'vitest'
import { CouncilContractError } from '@/shared/actions/ai-council'
import { estimateCouncil, runCouncil } from './council-client'

// The council's request body (DEC-V-R8): a question asked on a thread names the thread, and the body carries it only then.
// A question that names no thread is the body it was before the council named one.

const COUNCIL = { question: 'Hva er forskjellen?', models: ['model-a', 'model-b'] }

function reply(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } })
}

type FetchMock = Mock<(input: RequestInfo | URL, init?: RequestInit) => Promise<Response>>

function stubFetch(response: Response): FetchMock {
  const fetchMock = vi.fn<(input: RequestInfo | URL, init?: RequestInit) => Promise<Response>>(async () => response.clone())
  vi.stubGlobal('fetch', fetchMock)
  return fetchMock
}

function sentBody(fetchMock: FetchMock): string {
  return String(fetchMock.mock.calls[0]?.[1]?.body)
}

describe('council request body', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('sends the question, the models and the flags alone when no thread is named, exactly as before', async () => {
    const fetchMock = stubFetch(reply(200, { data: { members: [] } }))
    await runCouncil(COUNCIL)
    expect(sentBody(fetchMock)).toBe(
      '{"question":"Hva er forskjellen?","models":["model-a","model-b"],"allow_subscription":false,"estimate_only":false}',
    )
  })

  it('sends the thread the question is asked on, as thread_id, on the run', async () => {
    const fetchMock = stubFetch(reply(200, { data: { members: [] } }))
    await runCouncil({ ...COUNCIL, threadId: 'thread-1' })
    expect(sentBody(fetchMock)).toBe(
      '{"question":"Hva er forskjellen?","models":["model-a","model-b"],"allow_subscription":false,"estimate_only":false,"thread_id":"thread-1"}',
    )
  })

  it('sends the same thread on the estimate, which prices the run on that thread', async () => {
    const fetchMock = stubFetch(
      reply(200, {
        data: {
          estimate: {
            zdr: false,
            members: [],
            synthesis: {
              model: 'model-a',
              provider: 'provider-a',
              estimated_input_tokens: 10,
              max_output_tokens: 20,
              estimated_cost_usd: null,
            },
            dropped: [],
            total_estimated_cost_usd: null,
          },
        },
      }),
    )
    await estimateCouncil({ ...COUNCIL, threadId: 'thread-1' })
    expect(sentBody(fetchMock)).toBe(
      '{"question":"Hva er forskjellen?","models":["model-a","model-b"],"allow_subscription":false,"estimate_only":true,"thread_id":"thread-1"}',
    )
  })

  it('refuses a malformed thread before anything is sent', async () => {
    const fetchMock = stubFetch(reply(200, { data: { members: [] } }))
    await expect(runCouncil({ ...COUNCIL, threadId: 'thread 1' })).rejects.toThrow(CouncilContractError)
    expect(fetchMock).not.toHaveBeenCalled()
  })
})
