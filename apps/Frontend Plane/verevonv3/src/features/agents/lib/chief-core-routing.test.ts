import { describe, expect, it } from 'vitest'

import { groupInstallationsBySpace } from './chief-core-routing'

describe('groupInstallationsBySpace', () => {
  it('re-keys definitions by destination Space, sorted by Space name', () => {
    const grouped = groupInstallationsBySpace([
      {
        agent_ref: 'agent-1',
        name: 'Driftsassistent',
        installations: [
          { space_ref: 'space-b', space_name: 'Bravo rom', space_kind: 'room', status: 'active' },
          { space_ref: 'space-a', space_name: 'Alfa rom', space_kind: 'room', status: 'pending' },
        ],
      },
      {
        agent_ref: 'agent-2',
        name: 'Statusagent',
        installations: [
          { space_ref: 'space-a', space_name: 'Alfa rom', space_kind: 'room', status: 'active' },
        ],
      },
    ])

    expect(grouped.map((space) => space.space_name)).toEqual(['Alfa rom', 'Bravo rom'])
    expect(grouped[0]!.agents.map((agent) => agent.name)).toEqual(['Driftsassistent', 'Statusagent'])
    expect(grouped[0]!.agents[0]!.status).toBe('pending')
    expect(grouped[1]!.agents.map((agent) => agent.name)).toEqual(['Driftsassistent'])
  })

  it('returns nothing for an empty registry', () => {
    expect(groupInstallationsBySpace([])).toEqual([])
  })

  it('does not mutate its input', () => {
    const source = [
      {
        agent_ref: 'agent-1',
        name: 'Driftsassistent',
        installations: [
          { space_ref: 'space-a', space_name: 'Alfa rom', space_kind: 'room' as const, status: 'active' as const },
        ],
      },
    ]
    const snapshot = JSON.stringify(source)
    groupInstallationsBySpace(source)
    expect(JSON.stringify(source)).toBe(snapshot)
  })
})
