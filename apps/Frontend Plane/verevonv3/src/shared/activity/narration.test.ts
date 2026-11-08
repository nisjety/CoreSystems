import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { describe, expect, it } from 'vitest'
import type { ChatToolCall, LifecycleNotice } from '@/features/chat/components/chat-types'
import {
  FAMILY_BY_TOOL,
  TEMPLATES,
  narrate,
  narrationState,
  toolFamily,
  type NarrationState,
  type ToolFamily,
  type Translator,
} from './narration'

const NO: Translator = { tr: (noText) => noText }
const EN: Translator = { tr: (_noText, enText) => enText }
const NORWEGIAN_LETTERS = /[æøåÆØÅ]/

const FAMILIES: ToolFamily[] = ['search', 'read', 'command', 'account', 'change', 'generic']
const STATES: NarrationState[] = ['started', 'done', 'failed', 'retrying', 'awaiting_approval']

/** A call under the id the notices below refer to. */
const call = (name: string, fields: Partial<ChatToolCall> = {}): ChatToolCall => ({ id: 'call-1', name, ...fields })

const retry = (attempt: number, maxAttempts: number, callId = 'call-1'): LifecycleNotice => ({
  kind: 'retry',
  attempt,
  maxAttempts,
  reason: 'timeout',
  callId,
})

describe('narrate', () => {
  it('reads a finished search with its query and its result count', () => {
    const sentence = narrate(
      call('web_search', { args: { query: 'Aquatiq hygiene' }, status: 'ok', output: JSON.stringify({ count: 8 }) }),
      EN,
    )
    expect(sentence).toBe("Searched for 'Aquatiq hygiene' - 8 results")
  })

  it('reads a failed read by the file it read and the error it gave', () => {
    const sentence = narrate(
      call('web_fetch', { args: { path: '/uploads/invoice.pdf' }, status: 'failed', error: 'file too large' }),
      EN,
    )
    expect(sentence).toBe('Read invoice.pdf - failed: file too large')
  })

  it('names a page by its host alone, never by its path or query', () => {
    const sentence = narrate(call('web_fetch', { args: { url: 'https://www.example.com/a/b?token=secret' }, status: 'ok' }), EN)
    expect(sentence).toBe('Read www.example.com')
    expect(sentence).not.toContain('secret')
  })

  it('caps a long object and marks the cut', () => {
    const sentence = narrate(call('web_search', { args: { query: 'x'.repeat(200) } }), EN)
    const object = /'(.+)'/.exec(sentence)?.[1] ?? ''
    expect(object.length).toBeLessThanOrEqual(60)
    expect(object.endsWith('…')).toBe(true)
  })

  it('keeps the object as typed, dollar signs and slot names included', () => {
    expect(narrate(call('web_search', { args: { query: 'price $& $$' } }), EN)).toBe("Searching for 'price $& $$'")
    expect(narrate(call('web_search', { args: { query: '{outcome}' }, status: 'ok', output: JSON.stringify({ count: 2 }) }), EN)).toBe(
      "Searched for '{outcome}' - 2 results",
    )
  })

  it('reads arguments sent as JSON text', () => {
    expect(narrate(call('web_search', { args: '{"query":"tekst"}', status: 'ok' }), NO)).toBe("Søkte etter 'tekst'")
  })

  it('leaves the outcome out of a search that reports none', () => {
    expect(narrate(call('web_search', { args: { query: 'q' }, status: 'ok' }), EN)).toBe("Searched for 'q'")
  })

  it('reads a search that found nothing as no results', () => {
    const output = JSON.stringify({ count: 0 })
    expect(narrate(call('web_search', { args: { query: 'q' }, status: 'ok', output }), EN)).toBe("Searched for 'q' - no results")
  })

  it('says a page was truncated when the tool says so', () => {
    const output = JSON.stringify({ truncated: true })
    expect(narrate(call('web_fetch', { args: { url: 'https://example.com/p' }, status: 'ok', output }), EN)).toBe(
      'Read example.com - truncated',
    )
  })

  it('reads a failure with no error text as no details', () => {
    expect(narrate(call('web_search', { args: { query: 'q' }, status: 'error' }), EN)).toBe("Search for 'q' - failed: no details")
  })

  it('never shows the text of a command', () => {
    const sentence = narrate(call('code_interpreter', { args: { code: 'rm -rf /' }, status: 'ok' }), EN)
    expect(sentence).toBe('Ran a command')
    expect(sentence).not.toContain('rm -rf')
  })

  it('names an unknown MCP tool through the generic family, by its name', () => {
    expect(narrate(call('mcp__acme__sync_inventory', { status: 'ok' }), EN)).toBe('Used acme sync inventory')
    expect(narrate(call('mcp__acme__sync_inventory', { status: 'running' }), EN)).toBe('Using acme sync inventory')
  })

  it('reads a diff result as a change, whatever the tool is called', () => {
    const output = ['--- a/src/app.ts', '+++ b/src/app.ts', '@@ -1,1 +1,1 @@', '-old', '+new'].join('\n')
    const sentence = narrate(call('some_tool', { args: { path: 'src/app.ts' }, status: 'ok', output }), EN)
    expect(sentence).toBe('Changed app.ts - +1 −1')
  })

  it('says a call that is out in the present tense', () => {
    expect(narrate(call('web_search', { args: { query: 'q' }, status: 'running' }), EN)).toBe("Searching for 'q'")
    expect(narrate(call('web_search', { args: { query: 'q' } }), EN)).toBe("Searching for 'q'")
  })

  it('says a call the gateway is retrying, with its attempt', () => {
    expect(narrate(call('web_search', { args: { query: 'q' } }), EN, [retry(2, 3)])).toBe(
      "Retrying search for 'q' (attempt 2 of 3)",
    )
    expect(narrate(call('web_search', { args: { query: 'q' } }), NO, [retry(2, 3)])).toBe(
      "Prøver søket etter 'q' på nytt (forsøk 2 av 3)",
    )
  })

  it('does not mark a call with a retry notice for another call', () => {
    expect(narrate(call('web_search', { args: { query: 'q' } }), EN, [retry(2, 3, 'call-9')])).toBe("Searching for 'q'")
  })

  it('reads an approval wait from the status the call carries', () => {
    expect(narrate(call('web_search', { args: { query: 'q' }, status: 'awaiting_approval' }), EN)).toBe(
      "Waiting for approval to search for 'q'",
    )
  })
})

describe('narrationState', () => {
  it('is started while the call is out, and done once it has a result', () => {
    expect(narrationState(call('web_search'))).toBe('started')
    expect(narrationState(call('web_search', { status: 'running' }))).toBe('started')
    expect(narrationState(call('web_search', { status: 'completed' }))).toBe('done')
  })

  it('is failed on an error status or an error message', () => {
    expect(narrationState(call('web_search', { status: 'failed' }))).toBe('failed')
    expect(narrationState(call('web_search', { status: 'ok', error: 'boom' }))).toBe('failed')
  })

  it('is awaiting approval on the approval statuses', () => {
    expect(narrationState(call('web_search', { status: 'waiting_approval' }))).toBe('awaiting_approval')
  })

  it('is retrying only while a retry notice names the out call', () => {
    expect(narrationState(call('web_search'), [retry(2, 3)])).toBe('retrying')
    expect(narrationState(call('web_search', { status: 'ok' }), [retry(2, 3)])).toBe('done')
  })
})

describe('toolFamily', () => {
  it('takes the family a name is listed under', () => {
    expect(toolFamily('web_search', 'generic')).toBe('search')
    expect(toolFamily(' Web_Search ', 'generic')).toBe('search')
  })

  it('takes the family of the intent for a name no template lists', () => {
    expect(toolFamily('some_unknown_tool', 'terminal')).toBe('command')
    expect(toolFamily('some_unknown_tool', 'generic')).toBe('generic')
  })

  it('reads a patch as a change whatever its name', () => {
    expect(toolFamily('web_search', 'diff')).toBe('change')
  })

  it('takes the computer panel intents as command for a process, and generic for a browser step or a file', () => {
    expect(toolFamily('some_process_tool', 'process')).toBe('command')
    expect(toolFamily('browser.act', 'browser')).toBe('generic')
    expect(toolFamily('workspace_edit', 'file')).toBe('generic')
  })

  it('does not take an object prototype key for a listed name', () => {
    expect(toolFamily('constructor', 'search')).toBe('search')
    expect(toolFamily('constructor', 'generic')).toBe('generic')
  })
})

describe('the template table', () => {
  const keys = FAMILIES.flatMap((family) => STATES.map((state) => `${family}.${state}`))

  it('has every family in every state', () => {
    expect(Object.keys(TEMPLATES).sort()).toEqual([...keys].sort())
  })

  it.each(keys)('has a sentence for %s in both locales, and no Norwegian in the English one', (key) => {
    const template = TEMPLATES[key as keyof typeof TEMPLATES]
    const no = template(NO)
    const en = template(EN)
    expect(no).not.toBe('')
    expect(en).not.toBe('')
    expect(en).not.toMatch(NORWEGIAN_LETTERS)
    expect(no).not.toBe(en)
  })
})

describe('the Do tool catalogue', () => {
  const fixture = JSON.parse(
    readFileSync(resolve(process.cwd(), '../../Model Plane/rust/services/execution-core/tests/fixtures/do_tool_catalogue.json'), 'utf8'),
  ) as { tools: { name: string }[] }
  const names = fixture.tools.map((tool) => tool.name)

  it('reads the golden, which lists the offered tools', () => {
    expect(names.length).toBeGreaterThanOrEqual(30)
  })

  it('has an explicit family, or an explicit generic mapping, for every tool', () => {
    const unmapped = names.filter((name) => !Object.hasOwn(FAMILY_BY_TOOL, name.toLowerCase()))
    expect(unmapped).toEqual([])
  })

  it('narrates every tool in both locales', () => {
    const bad = names.filter((name) => {
      const no = narrate(call(name, { args: {}, status: 'ok' }), NO)
      const en = narrate(call(name, { args: {}, status: 'ok' }), EN)
      return no === '' || en === '' || NORWEGIAN_LETTERS.test(en)
    })
    expect(bad).toEqual([])
  })
})
