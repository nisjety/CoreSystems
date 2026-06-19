import { describe, expect, it } from 'vitest'
import {
  createSelectedAgentToolSpecs,
  createVelionActionToolDefinitions,
  createVelionActionToolSpecs,
} from './agent-tools'

describe('agent tool surface', () => {
  it('exposes every registered UI action as an AI-visible tool contract', () => {
    const specs = createVelionActionToolSpecs()

    expect(specs.map((tool) => tool.name)).toContain('knowledge.recrawl_source')

    const recrawl = specs.find((tool) => tool.name === 'knowledge.recrawl_source')
    expect(recrawl?.description).toContain('Owner plane: ingestion')
    expect(recrawl?.description).toContain('Requires approval: no')
    expect(JSON.parse(recrawl?.parametersJson ?? '{}')).toMatchObject({
      type: 'object',
      properties: {
        sourceId: { type: 'string' },
      },
      required: ['sourceId'],
    })

    const brreg = specs.find((tool) => tool.name === 'brreg.lookup_organization')
    expect(brreg?.description).toContain('Owner plane: control')
    expect(JSON.parse(brreg?.parametersJson ?? '{}')).toMatchObject({
      type: 'object',
      properties: {
        q: { type: 'string' },
        size: { type: 'integer' },
      },
      required: ['q', 'size'],
    })
  })

  it('creates TanStack tool definitions with approval metadata', () => {
    const definitions = createVelionActionToolDefinitions()
    const deploy = definitions.find((tool) => tool.name === 'agents.deploy_channel')

    expect(deploy?.needsApproval).toBe(true)
    expect(deploy?.metadata).toMatchObject({
      actionId: 'agents.deploy_channel',
      ownerPlane: 'application',
      risk: 'high',
      reversible: true,
    })
  })

  it('dedupes selected composer tools and keeps builtin web search available to the model', () => {
    const specs = createSelectedAgentToolSpecs({
      browseWeb: true,
      actions: [
        { id: 'web_search', name: 'Web search', kind: 'tool' },
        { id: 'knowledge.recrawl_source', name: 'Recrawl source', kind: 'tool' },
      ],
    })

    expect(specs.map((tool) => tool.name)).toEqual([
      'web_search',
      'knowledge.recrawl_source',
    ])
  })
})
