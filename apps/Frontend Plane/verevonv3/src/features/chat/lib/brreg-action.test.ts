import { describe, expect, it } from 'vitest'
import { createSelectedAgentToolSpecs } from '@/shared/actions/agent-tools'
import {
  BRREG_LOOKUP_ACTION,
  shouldAttachBrregLookupAction,
  withBrregLookupAction,
} from './brreg-action'

describe('Brreg chat action detection', () => {
  it('detects explicit Brreg and Norwegian organization-number prompts', () => {
    expect(shouldAttachBrregLookupAction('Find Aquatiq in Brreg')).toBe(true)
    expect(shouldAttachBrregLookupAction('Hva er organisasjonsnummeret til Aquatiq?')).toBe(true)
    expect(shouldAttachBrregLookupAction('What is the org number for Aquatiq?')).toBe(true)
    expect(shouldAttachBrregLookupAction('Search the Norwegian business registry for Aquatiq')).toBe(true)
  })

  it('does not trigger for unrelated programming registry questions', () => {
    expect(shouldAttachBrregLookupAction('Explain the Windows registry')).toBe(false)
    expect(shouldAttachBrregLookupAction('How do I publish an npm package?')).toBe(false)
  })

  it('adds the lookup action once', () => {
    expect(withBrregLookupAction([], 'Brønnøysund Aquatiq')).toEqual([BRREG_LOOKUP_ACTION])
    expect(withBrregLookupAction([BRREG_LOOKUP_ACTION], 'Brreg Aquatiq')).toEqual([BRREG_LOOKUP_ACTION])
  })

  it('attaches the canonical registry action with its required lookup schema', () => {
    const actions = withBrregLookupAction([], 'Find Aquatiq in Brreg')
    const [tool] = createSelectedAgentToolSpecs({ actions })

    expect(tool?.name).toBe('brreg.lookup_organization')
    expect(JSON.parse(tool?.parametersJson ?? '{}')).toMatchObject({
      type: 'object',
      properties: {
        q: { type: 'string' },
        size: { type: 'integer' },
      },
      required: ['q', 'size'],
    })
  })
})
