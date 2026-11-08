// @vitest-environment jsdom

import { cleanup, fireEvent, render } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { RunRequestSnapshot } from '@/shared/api/run-console-client'
import { isRequestInspectorEnabled, manifestEntries, RequestInspector, requestJson, toolEntries } from './RequestInspector'

// Without a provider the page renders in its default locale, Norwegian.

afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

const request = (overrides: Partial<RunRequestSnapshot> = {}): RunRequestSnapshot => ({
  runId: 'run-1',
  callSeq: 1,
  callPurpose: 'answer',
  snapshot: {
    schema: 'request-snapshot-v1',
    model: 'gpt-5.1',
    provider: 'azure-openai',
    params: { temperature: 0.2 },
    messages: [
      { role: 'system', source_tag: '', chars: 40, sha256: 'a'.repeat(64) },
      { role: 'system', source_tag: 'memory', chars: 30, sha256: 'b'.repeat(64) },
      { role: 'user', source_tag: '', chars: 22, sha256: 'c'.repeat(64) },
    ],
  },
  sections: [
    { label: 'stable', source: 'platform', digest: 'a'.repeat(64), chars: 40 },
    { label: 'Memory', source: 'memory', digest: 'b'.repeat(64), chars: 30, text: 'The user prefers metric units.' },
  ],
  tools: [{ name: 'web_search', description: 'Search the web.', parameters_json: '{"type":"object"}' }],
  toolsTruncated: false,
  systemTruncated: false,
  dropped: 0,
  ...overrides,
})

describe('request inspector helpers', () => {
  it('turns on only for the literal true', () => {
    expect(isRequestInspectorEnabled('true')).toBe(true)
    for (const raw of [undefined, '', '1', 'TRUE', 'yes']) expect(isRequestInspectorEnabled(raw)).toBe(false)
  })

  it('reads tools and the manifest without inventing entries', () => {
    expect(toolEntries([{ name: 'web_search' }, { description: 'no name' }, 'text'])).toEqual([
      { name: 'web_search', description: '', parameters: '' },
    ])
    expect(toolEntries(null)).toEqual([])
    expect(manifestEntries(request().snapshot).map((entry) => entry.role)).toEqual(['system', 'system', 'user'])
    expect(manifestEntries(null)).toEqual([])
  })

  it('copies exactly the shown record', () => {
    const copied = JSON.parse(requestJson(request()))
    expect(copied.call_seq).toBe(1)
    expect(copied.system_sections[0].text).toBeUndefined()
    expect(copied.system_sections[1].text).toBe('The user prefers metric units.')
  })
})

describe('RequestInspector', () => {
  it('fetches nothing until asked, then shows sections, tools and the manifest', async () => {
    const load = vi.fn(async () => request())
    const { container, findByText } = render(() => <RequestInspector runId="run-1" load={load} />)
    expect(load).not.toHaveBeenCalled()
    fireEvent.click(container.querySelector('button') as HTMLButtonElement)
    expect(load).toHaveBeenCalledWith('run-1', 1)
    await findByText('The user prefers metric units.')

    const platform = container.querySelector('[data-section-source="platform"]')
    expect(platform?.querySelector('pre')).toBeNull()
    expect(platform?.textContent).toContain('Teksten vises ikke.')
    expect(platform?.textContent).toContain('a'.repeat(12))
    expect(container.textContent).toContain('web_search')
    expect(container.querySelectorAll('.verevon-chat-request__manifest tbody tr')).toHaveLength(3)
    expect(container.querySelector('.verevon-chat-request__incomplete')).toBeNull()
  })

  it('says the record is incomplete when calls were not captured', async () => {
    const load = vi.fn(async () => request({ dropped: 2 }))
    const { container, findByText } = render(() => <RequestInspector runId="run-1" load={load} />)
    fireEvent.click(container.querySelector('button') as HTMLButtonElement)
    await findByText('Denne oversikten er ufullstendig: 2 kall ble ikke registrert.')
  })

  it('copies the request JSON to the clipboard', async () => {
    const writeText = vi.fn(async () => undefined)
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true })
    const load = vi.fn(async () => request())
    const { container, findByText, getByText } = render(() => <RequestInspector runId="run-1" load={load} />)
    fireEvent.click(container.querySelector('button') as HTMLButtonElement)
    await findByText('The user prefers metric units.')
    fireEvent.click(getByText('Kopier forespørsel som JSON'))
    await findByText('Forespørselen er kopiert som JSON.')
    expect(writeText).toHaveBeenCalledWith(requestJson(request()))
  })

  it('steps to the next call and says when a call is not available', async () => {
    const load = vi.fn(async (_runId: string, callSeq: number): Promise<RunRequestSnapshot> => {
      if (callSeq > 1) throw new Error('404')
      return request()
    })
    const { container, findByText, getByText } = render(() => <RequestInspector runId="run-1" load={load} />)
    fireEvent.click(container.querySelector('button') as HTMLButtonElement)
    await findByText('The user prefers metric units.')
    fireEvent.click(getByText('Neste kall'))
    expect(load).toHaveBeenLastCalledWith('run-1', 2)
    await findByText('Kall 2 er ikke tilgjengelig for denne kjøringen.')
  })
})
