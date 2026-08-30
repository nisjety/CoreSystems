import { describe, expect, it } from 'vitest'
import {
  createSelectedAgentToolSpecs,
  createVerevonActionToolDefinitions,
  createVerevonActionToolSpecs,
} from './agent-tools'
import { actionRegistry } from './action-registry'
import { isModelExecutableAction } from './model-eligibility'

describe('agent tool surface', () => {
  it('advertises only actions whose owner path has been proven', () => {
    const specs = createVerevonActionToolSpecs()

    // The registry is the catalogue of what a HUMAN can do; eligibility is a
    // separate, evidence-backed claim about what a MODEL may be offered. The
    // gap between the two numbers is the point of this test, so assert the
    // exact eligible set rather than a count that drifts as the registry grows.
    expect(actionRegistry.length).toBeGreaterThan(1)
    expect(specs.map((tool) => tool.name)).toEqual([
      'inbox.follow_conversation',
      'inbox.set_csat_preference',
      'inbox.review_ai_action',
      'tickets.create',
      'org.mark_exported',
      'org.acknowledge_deletion',
      'chat.save_thread_snapshot',
      'chat.submit_feedback',
    ])
  })

  it('describes an eligible action with its risk and approval posture', () => {
    const specs = createVerevonActionToolSpecs()
    const spec = specs.find((tool) => tool.name === 'tickets.create')
    expect(spec).toBeDefined()
    if (!spec) return

    // The model is told the governance facts, not just the name: an action it
    // can call is not the same as an action it may call unattended.
    expect(spec.description).toContain('Owner plane: application.')
    expect(spec.description).toContain('Risk: medium.')
    expect(spec.description).toContain('Requires approval: no.')
    expect(spec.description).toContain('Reversible: yes.')
    // The parameter schema must be the registry's own zod contract, so the
    // model cannot be handed a looser shape than the owner will accept.
    expect(JSON.parse(spec.parametersJson).required).toContain('conversationId')
  })

  it('carries the owner and approval metadata into tool definitions', () => {
    const definitions = createVerevonActionToolDefinitions()

    expect(definitions.map((tool) => tool.name)).toEqual([
      'inbox.follow_conversation',
      'inbox.set_csat_preference',
      'inbox.review_ai_action',
      'tickets.create',
      'org.mark_exported',
      'org.acknowledge_deletion',
      'chat.save_thread_snapshot',
      'chat.submit_feedback',
    ])
    const definition = definitions.find((tool) => tool.name === 'tickets.create')
    expect(definition).toBeDefined()
    if (!definition) return
    expect(definition.needsApproval).toBe(false)
    expect(definition.metadata).toMatchObject({
      actionId: 'tickets.create',
      ownerPlane: 'application',
      risk: 'medium',
      reversible: true,
    })
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

  it('refuses to promote a known-but-ineligible action through the dynamic escape hatch', () => {
    // A registry action that is NOT allowlisted must never fall through to the
    // generic dynamic-tool path, which would advertise it with an empty schema
    // and no owner path — the exact false capability eligibility exists to stop.
    // Pinned against a real registry id so it fails if that id ever becomes
    // eligible without this test being revisited.
    expect(isModelExecutableAction('social.publish_post')).toBe(false)

    const specs = createSelectedAgentToolSpecs({
      actions: [{ id: 'social.publish_post', name: 'Publish post', kind: 'tool' }],
    })

    expect(specs).toEqual([])
  })

  it('retains explicitly supplied non-registry tools for the runtime that owns them', () => {
    const specs = createSelectedAgentToolSpecs({
      actions: [{ id: 'owner_runtime_tool', name: 'Owner runtime tool', kind: 'tool' }],
    })
    expect(specs.map((tool) => tool.name)).toEqual(['owner_runtime_tool'])
  })
})
