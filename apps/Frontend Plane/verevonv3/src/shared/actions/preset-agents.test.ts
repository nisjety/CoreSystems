import { describe, expect, it } from 'vitest'
import { actionRegistry } from '@/shared/actions/action-registry'
import {
  getPresetAgent,
  presetAgentChatActions,
  presetAgents,
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
      for (const actionId of preset.actionIds) {
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
