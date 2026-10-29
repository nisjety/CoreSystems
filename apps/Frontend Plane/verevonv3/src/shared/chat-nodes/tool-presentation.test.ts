import { describe, expect, it } from 'vitest'
import { toolPresentation } from './tool-presentation'
import type { ChatToolCall } from '@/features/chat/components/chat-types'

const call = (overrides: Partial<ChatToolCall>): ChatToolCall => ({
  id: 't1',
  name: 'shell',
  ...overrides,
})

describe('toolPresentation', () => {
  it('classifies commands as terminal', () => {
    for (const name of ['shell', 'code_interpreter', 'CODE_INTERPRETER']) {
      expect(toolPresentation(call({ name })).intent).toBe('terminal')
    }
  })

  it('classifies result-set tools as search and single fetches as read', () => {
    for (const name of ['web_search', 'knowledge_search', 'company_lookup']) {
      expect(toolPresentation(call({ name })).intent).toBe('search')
    }
    for (const name of [
      'web_fetch',
      'yr_weather',
      'track_shipment',
      'recall_memory',
      'reattach_context',
    ]) {
      expect(toolPresentation(call({ name })).intent).toBe('read')
    }
  })

  /**
   * An unknown tool must NOT be guessed at. Third-party MCP output has no shape
   * we can predict, and a wrong card is most confusing precisely there.
   */
  it('falls back to generic for action, MCP and subagent tools', () => {
    for (const name of [
      'book_shipment',
      'mcp__visma__list_invoices',
      'subagent.research',
      '',
    ]) {
      expect(toolPresentation(call({ name })).intent).toBe('generic')
    }
  })

  it('reads both count spellings the backends emit', () => {
    // model-gateway's builtins say `count`; execution-core's knowledge tools say
    // `result_count`. Supporting one silently loses the number for half the tools.
    expect(
      toolPresentation(call({ name: 'company_lookup', output: '{"count":3}' }))
        .count,
    ).toBe(3)
    expect(
      toolPresentation(
        call({ name: 'knowledge_search', output: '{"result_count":7}' }),
      ).count,
    ).toBe(7)
  })

  it('falls back to counting the results array when no count is reported', () => {
    expect(
      toolPresentation(
        call({ name: 'web_search', output: '{"results":[{},{},{}]}' }),
      ).count,
    ).toBe(3)
  })

  it('surfaces truncation, which is not the same as no results', () => {
    const truncated = toolPresentation(
      call({ name: 'web_fetch', output: '{"truncated":true}' }),
    )
    expect(truncated.truncated).toBe(true)
    const complete = toolPresentation(
      call({ name: 'web_fetch', output: '{"truncated":false}' }),
    )
    expect(complete.truncated).toBe(false)
    // Absent means the tool did not report either way — not "complete".
    expect(
      toolPresentation(call({ name: 'web_fetch', output: '{}' })).truncated,
    ).toBeUndefined()
  })

  it('carries the tool-reported outcome through', () => {
    expect(
      toolPresentation(
        call({ name: 'knowledge_search', output: '{"status":"no_results"}' }),
      ).outcome,
    ).toBe('no_results')
  })

  /**
   * A tool whose output is plain text, or JSON cut off mid-object, must still
   * classify — the intent comes from the NAME. Only the extracted fields are
   * unavailable.
   */
  it('classifies even when the output is not parseable JSON', () => {
    for (const output of [
      "total 4\ndrwxr-xr-x",
      '{"results":[{"id":"a"',
      undefined,
    ]) {
      const presentation = toolPresentation(call({ name: 'shell', output }))
      expect(presentation.intent).toBe('terminal')
      expect(presentation.count).toBeUndefined()
    }
  })

  /**
   * The "never persisted" property, asserted as purity: the same call always
   * yields the same presentation, and nothing is written back onto the call.
   * That is what lets this mapping change and take all existing transcripts with
   * it, with no migration.
   */
  it('is a pure function of the call and mutates nothing', () => {
    const original = call({
      name: 'knowledge_search',
      output: '{"result_count":2,"status":"ok"}',
    })
    const snapshot = JSON.stringify(original)
    const first = toolPresentation(original)
    const second = toolPresentation(original)
    expect(first).toEqual(second)
    expect(JSON.stringify(original)).toBe(snapshot)
    // And no presentation field leaked onto the call itself.
    expect(Object.keys(original)).not.toContain('intent')
  })
})

describe('diff intent', () => {
  const patch = [
    '--- a/f',
    '+++ b/f',
    '@@ -1,2 +1,2 @@',
    ' keep',
    '-old',
    '+new',
  ].join("\n")

  /**
   * The whole point of detecting from content: a tool nobody registered here —
   * an MCP git server — still gets the right card.
   */
  it('classifies any tool whose output IS a patch, whatever its name', () => {
    for (const name of ['mcp__git__diff', 'some_unknown_tool', 'shell']) {
      const presentation = toolPresentation(call({ name, output: patch }))
      expect(presentation.intent).toBe('diff')
      expect(presentation.added).toBe(1)
      expect(presentation.removed).toBe(1)
    }
  })

  /**
   * Content wins over the name-based mapping, but only for real patches — a
   * terminal tool whose output merely mentions `-`/`+` stays terminal.
   */
  it('does not steal the intent from output that only looks diff-ish', () => {
    const listOutput = "- first\n- second\n+ plus"
    expect(
      toolPresentation(call({ name: 'shell', output: listOutput })).intent,
    ).toBe('terminal')
    const logOutput = "INFO - started\nWARN + retried"
    expect(
      toolPresentation(call({ name: 'web_fetch', output: logOutput })).intent,
    ).toBe('read')
  })

  it('leaves a tool with no output on its name-based intent', () => {
    expect(toolPresentation(call({ name: 'shell' })).intent).toBe('terminal')
    expect(toolPresentation(call({ name: 'knowledge_search' })).intent).toBe(
      'search',
    )
  })
})
