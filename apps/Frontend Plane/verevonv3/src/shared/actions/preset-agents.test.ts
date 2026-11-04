import { afterEach, describe, expect, it, vi } from 'vitest'
import { actionRegistry } from '@/shared/actions/action-registry'
import {
  getPresetAgent,
  presetAgentChatActions,
  presetAgents,
  resolvePresetAgent,
} from '@/shared/actions/preset-agents'

const registryIds = new Set(actionRegistry.map((action) => action.id))

describe('preset agents', () => {
  it('ships between 3 and 5 presets with unique ids', () => {
    expect(presetAgents.length).toBeGreaterThanOrEqual(3)
    expect(presetAgents.length).toBeLessThanOrEqual(5)
    expect(new Set(presetAgents.map((preset) => preset.id)).size).toBe(presetAgents.length)
  })

  it('only references action ids that exist in the real action registry', () => {
    for (const preset of presetAgents) {
      for (const actionId of [...preset.requiredActionIds, ...preset.actionIds]) {
        expect(registryIds.has(actionId)).toBe(true)
      }
    }
  })

  it('never lets a preset opt out of the approval-gated agentic run path', () => {
    for (const preset of presetAgents) {
      expect(preset.defaults.planMode).toBe(true)
    }
  })

  it('gives each preset a non-empty goal template distinct from its label', () => {
    for (const preset of presetAgents) {
      expect(preset.goalTemplate.trim().length).toBeGreaterThan(20)
      expect(preset.goalTemplate).not.toBe(preset.label)
    }
  })

  it('looks up a preset by id', () => {
    expect(getPresetAgent('draft-reply-with-sources')?.label).toBe('Draft reply with sources')
    expect(getPresetAgent('refresh-knowledge-base')?.label).toBe('Plan a knowledge-base refresh')
    expect(getPresetAgent('refresh-knowledge-base')?.actionIds).toEqual([])
  })

  it('keeps presets tool-free until their owner actions become Model-eligible', () => {
    const preset = getPresetAgent('triage-urgent-tickets')!
    const actions = presetAgentChatActions(preset)

    expect(actions).toHaveLength(preset.actionIds.length)
    expect(actions.map((action) => action.id)).toEqual([...preset.actionIds])
    expect(actions).toEqual([])
  })
})

describe('preset owner actions', () => {
  const triage = getPresetAgent('triage-urgent-tickets')!
  const admittedTickets = new Set<string>(['tickets.create'])

  afterEach(() => {
    vi.unstubAllEnvs()
  })

  it('leaves every preset available and tool-free while the switch is off', () => {
    for (const preset of presetAgents) {
      const resolved = resolvePresetAgent(preset, admittedTickets)

      expect(resolved.available).toBe(true)
      expect(resolved.actionIds).toEqual([])
      expect(presetAgentChatActions(resolved)).toEqual([])
    }
  })

  it('reads the switch strictly, so only the literal "true" turns it on', () => {
    vi.stubEnv('VITE_PRESET_AGENT_ACTIONS', '1')

    expect(resolvePresetAgent(triage, new Set())).toMatchObject({ available: true, actionIds: [] })
  })

  it('offers a required action that the org admits', () => {
    vi.stubEnv('VITE_PRESET_AGENT_ACTIONS', 'true')
    const resolved = resolvePresetAgent(triage, admittedTickets)

    expect(resolved.available).toBe(true)
    expect(resolved.actionIds).toEqual(['tickets.create'])
    expect(presetAgentChatActions(resolved).map((action) => action.id)).toEqual(['tickets.create'])
  })

  it('marks a preset unavailable, with no actions, when its action is not admitted', () => {
    vi.stubEnv('VITE_PRESET_AGENT_ACTIONS', 'true')
    const resolved = resolvePresetAgent(triage, new Set())

    expect(resolved.available).toBe(false)
    expect(resolved.actionIds).toEqual([])
    expect(presetAgentChatActions(resolved)).toEqual([])
  })

  it('keeps plain-chat presets available when nothing is admitted', () => {
    vi.stubEnv('VITE_PRESET_AGENT_ACTIONS', 'true')

    for (const preset of presetAgents.filter((candidate) => candidate.requiredActionIds.length === 0)) {
      expect(resolvePresetAgent(preset, new Set())).toMatchObject({ available: true, actionIds: [] })
    }
  })

  it('needs every required action, not just one of them', () => {
    vi.stubEnv('VITE_PRESET_AGENT_ACTIONS', 'true')
    const twoActions = { ...triage, requiredActionIds: ['tickets.create', 'inbox.follow_conversation'] as const }
    const resolved = resolvePresetAgent(twoActions, admittedTickets)

    expect(resolved.available).toBe(false)
    expect(resolved.actionIds).toEqual([])
  })

  it('never offers an admitted row that has no model-executable path here', () => {
    vi.stubEnv('VITE_PRESET_AGENT_ACTIONS', 'true')
    const notExecutable = { ...triage, requiredActionIds: ['tickets.classify_conversation'] as const }
    const resolved = resolvePresetAgent(notExecutable, new Set(['tickets.classify_conversation']))

    expect(resolved.available).toBe(false)
    expect(resolved.actionIds).toEqual([])
  })

  it('only narrows: a resolved preset never offers an action it did not require', () => {
    vi.stubEnv('VITE_PRESET_AGENT_ACTIONS', 'true')
    const everything = new Set<string>(registryIds)

    for (const preset of presetAgents) {
      const resolved = resolvePresetAgent(preset, everything)
      for (const actionId of resolved.actionIds) {
        expect(preset.requiredActionIds).toContain(actionId)
      }
    }
  })
})
