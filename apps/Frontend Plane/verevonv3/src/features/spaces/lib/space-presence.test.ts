import { describe, expect, it } from 'vitest'

import { hereSentence, nameList, readPresence, typingSentence } from './space-presence'

const tr = (no: string) => no

const roster = [
  { subject_id: 'u-kari', display_name: 'Kari', subject_type: 'user' },
  { subject_id: 'u-ola', display_name: 'Ola', subject_type: 'user' },
  { subject_id: 'u-anne', display_name: 'Anne', subject_type: 'user' },
] as never

const present = (...members: Array<[string, 'online' | 'typing']>) =>
  members.map(([subject_id, status]) => ({ subject_id, status, last_seen_at: 1 }))

describe('reading presence against the roster', () => {
  it('names the people it can and counts everyone', () => {
    const reading = readPresence(present(['u-kari', 'online'], ['u-ola', 'typing']), roster)
    expect(reading.hereNames).toEqual(['Kari', 'Ola'])
    expect(reading.hereCount).toBe(2)
    expect(reading.typingNames).toEqual(['Ola'])
    expect(reading.typingCount).toBe(1)
  })

  // A member who left the room, or a roster that has not loaded yet. Inventing
  // a label for a bare identifier would be worse than counting them.
  it('counts a subject the roster does not know, and never names it', () => {
    const reading = readPresence(present(['u-kari', 'online'], ['u-stranger', 'online']), roster)
    expect(reading.hereNames).toEqual(['Kari'])
    expect(reading.hereCount).toBe(2)
    expect(hereSentence(reading, tr)).toBe('Kari og 1 andre er her nå')
  })

  it('is empty when nobody else is here, and when presence is unknown', () => {
    expect(hereSentence(readPresence([], roster), tr)).toBeUndefined()
    expect(hereSentence(readPresence(undefined, roster), tr)).toBeUndefined()
    expect(typingSentence(readPresence([], roster), tr)).toBeUndefined()
  })
})

describe('the sentence', () => {
  it('says one name, two names, then a remainder', () => {
    expect(nameList(['Kari'], 1, tr)).toBe('Kari')
    expect(nameList(['Kari', 'Ola'], 2, tr)).toBe('Kari og Ola')
    expect(nameList(['Kari', 'Ola', 'Anne'], 3, tr)).toBe('Kari, Ola og 1 andre')
  })

  it('falls back to a bare count when no name is known', () => {
    expect(nameList([], 1, tr)).toBe('1 person')
    expect(nameList([], 4, tr)).toBe('4 personer')
  })

  it('reads as a room line and a writing line', () => {
    const one = readPresence(present(['u-kari', 'typing']), roster)
    expect(hereSentence(one, tr)).toBe('Kari er her nå')
    expect(typingSentence(one, tr)).toBe('Kari skriver …')

    const two = readPresence(present(['u-kari', 'typing'], ['u-ola', 'typing']), roster)
    expect(typingSentence(two, tr)).toBe('Kari og Ola skriver …')
  })

  // Presence rows arrive ordered by identifier; the sentence orders by name so
  // the line reads the same way twice running.
  it('orders names alphabetically, not by arrival', () => {
    const reading = readPresence(present(['u-ola', 'online'], ['u-anne', 'online']), roster)
    expect(reading.hereNames).toEqual(['Anne', 'Ola'])
  })
})
