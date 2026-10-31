import { describe, expect, it } from 'vitest'

import { robotsBlockedText } from './knowledge-robots'

const english = (_noText: string, enText: string) => enText
const norwegian = (noText: string) => noText

describe('robotsBlockedText (WS14.S4)', () => {
  it('shows nothing when the robots file kept no page out', () => {
    expect(robotsBlockedText(undefined, english)).toBe('')
    expect(robotsBlockedText(0, english)).toBe('')
    expect(robotsBlockedText(0.5, english)).toBe('')
    expect(robotsBlockedText(Number.NaN, english)).toBe('')
    expect(robotsBlockedText(Number.POSITIVE_INFINITY, english)).toBe('')
  })

  it('names the count in English, singular and plural', () => {
    expect(robotsBlockedText(1, english)).toBe('1 page blocked by robots.txt')
    expect(robotsBlockedText(3, english)).toBe('3 pages blocked by robots.txt')
  })

  it('names the count in Norwegian, singular and plural', () => {
    expect(robotsBlockedText(1, norwegian)).toBe('1 side blokkert av robots.txt')
    expect(robotsBlockedText(4, norwegian)).toBe('4 sider blokkert av robots.txt')
  })

  it('reads a whole count and never a fraction', () => {
    expect(robotsBlockedText(2.9, english)).toBe('2 pages blocked by robots.txt')
  })
})
