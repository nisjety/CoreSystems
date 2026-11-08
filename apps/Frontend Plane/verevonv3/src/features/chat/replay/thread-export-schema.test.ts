import { describe, expect, it } from 'vitest'
import { readThreadExport, THREAD_EXPORT_FORMAT } from './thread-export-schema'

// A redacted v1 export as the gateway writes it: two turns and one run.
function exportFixture(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    format: THREAD_EXPORT_FORMAT,
    version: 1,
    exported_at: '2026-11-05T10:00:00Z',
    profile: 'redacted',
    thread: { id: 'thread-1', title: 'Plan', created_at: '2026-11-04T09:00:00Z' },
    messages: [
      {
        id: 'm-1',
        role: 'user',
        content: 'Hei',
        created_at: '2026-11-04T09:00:01Z',
        artifacts: [],
        citations: [],
        metadata: {},
      },
      {
        id: 'm-2',
        role: 'assistant',
        content: 'Hallo',
        created_at: '2026-11-04T09:00:05Z',
        artifacts: [{ kind: 'document', title: 'Notat' }],
        citations: [],
        metadata: { tool_steps: [] },
      },
    ],
    runs: [
      {
        id: 'run-1',
        status: 'completed',
        origin: 'chat',
        model: 'model-a',
        input_tokens: 120,
        output_tokens: 40,
        created_at: '2026-11-04T09:00:02Z',
      },
    ],
    truncated: false,
    ...overrides,
  }
}

const asText = (value: unknown) => JSON.stringify(value)

describe('readThreadExport', () => {
  it('reads a v1 export into the document it names', () => {
    const reading = readThreadExport(asText(exportFixture()))

    expect(reading.status).toBe('ok')
    if (reading.status !== 'ok') return
    expect(reading.document.thread.title).toBe('Plan')
    expect(reading.document.messages.map((message) => message.role)).toEqual(['user', 'assistant'])
    expect(reading.document.runs[0]).toMatchObject({ status: 'completed', origin: 'chat', input_tokens: 120 })
  })

  it('refuses a version it does not know, and reads none of its contents', () => {
    expect(readThreadExport(asText(exportFixture({ version: 2 })))).toEqual({ status: 'unsupported' })
  })

  it('refuses a document that names another format', () => {
    expect(readThreadExport(asText(exportFixture({ format: 'verevon.thread-export.v2' })))).toEqual({
      status: 'unsupported',
    })
  })

  it('refuses text that is not JSON, and JSON that is not an object', () => {
    expect(readThreadExport('# Plan\n\nHei')).toEqual({ status: 'invalid' })
    expect(readThreadExport('[]')).toEqual({ status: 'invalid' })
    expect(readThreadExport('null')).toEqual({ status: 'invalid' })
  })

  it('refuses a v1 document whose shape is wrong', () => {
    expect(readThreadExport(asText(exportFixture({ messages: 'none' })))).toEqual({ status: 'invalid' })
    expect(readThreadExport(asText(exportFixture({ profile: 'everything' })))).toEqual({ status: 'invalid' })
  })

  it('leaves out fields it does not know rather than refusing the export', () => {
    const reading = readThreadExport(asText(exportFixture({ future_field: { anything: true } })))

    expect(reading.status).toBe('ok')
  })
})
