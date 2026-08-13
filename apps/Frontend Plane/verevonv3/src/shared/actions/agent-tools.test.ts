import { describe, expect, it } from 'vitest'
import {
  createSelectedAgentToolSpecs,
  createVerevonActionToolDefinitions,
  createVerevonActionToolSpecs,
} from './agent-tools'
import { actionRegistry } from './action-registry'

describe('agent tool surface', () => {
  it('does not advertise browser-only actions as Model tools before an owner path exists', () => {
    const specs = createVerevonActionToolSpecs()

    expect(actionRegistry).not.toHaveLength(0)
    expect(specs).toEqual([])
  })

  it('does not create client-side Model tool definitions for browser actions', () => {
    const definitions = createVerevonActionToolDefinitions()
    expect(definitions).toEqual([])
  })

  it('keeps builtin web search but strips known actions lacking Model eligibility', () => {
    const specs = createSelectedAgentToolSpecs({
      browseWeb: true,
      actions: [
        { id: 'web_search', name: 'Web search', kind: 'tool' },
        { id: 'knowledge.recrawl_source', name: 'Recrawl source', kind: 'tool' },
      ],
    })

    expect(specs.map((tool) => tool.name)).toEqual([
      'web_search',
    ])
  })

  it('retains explicitly supplied non-registry tools for the runtime that owns them', () => {
    const specs = createSelectedAgentToolSpecs({
      actions: [{ id: 'owner_runtime_tool', name: 'Owner runtime tool', kind: 'tool' }],
    })
    expect(specs.map((tool) => tool.name)).toEqual(['owner_runtime_tool'])
  })
})
