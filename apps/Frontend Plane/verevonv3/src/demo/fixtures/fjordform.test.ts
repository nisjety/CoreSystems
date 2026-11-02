import { describe, expect, it } from 'vitest'
import { nordlysDraftChunks, nordlysSources, nordlysStatusLines } from './fjordform'

describe('scene 02 delivery evidence', () => {
  it('attributes the delay and new date to Bring rather than the order system', () => {
    const bring = nordlysSources.find((source) => source.source_group_label === 'Bring')
    expect(bring?.title).toContain('Bring')
    expect(bring?.snippet).toContain('14. okt')
    expect(nordlysStatusLines.slice(0, 2).map((line) => line.ref)).toEqual(['Bring', 'Bring'])
    expect(nordlysStatusLines[0]?.text).toContain('Bring')
    expect(nordlysDraftChunks.join('')).toContain('forsinket hos Bring')
    expect(nordlysSources.some((source) => source.source_group_label === 'Visma.net')).toBe(false)
  })
})
