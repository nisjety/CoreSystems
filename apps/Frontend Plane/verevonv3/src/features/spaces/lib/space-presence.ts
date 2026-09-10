import type { SpacePresentMember, SpaceRosterMember } from '@/shared/api/spaces-client'

/**
 * Turning "who is here" into a sentence a person reads.
 *
 * Presence arrives as Control subject ids; names come from the roster the room
 * already has. A subject the roster does not know is *counted* but never
 * named — inventing a label for an identifier would be worse than saying
 * "and 2 others", and a member who just left the room is exactly that case.
 *
 * Kept pure and separate from the components so the counting and the wording
 * can be tested without a room.
 */

/** How many people are named before the rest become a count. */
const NAMED_LIMIT = 2

export type PresenceReading = {
  /** Everyone else the server says is in the room, named where possible. */
  readonly hereNames: readonly string[]
  readonly hereCount: number
  /** The subset actively writing. */
  readonly typingNames: readonly string[]
  readonly typingCount: number
}

export function readPresence(
  present: readonly SpacePresentMember[] | undefined,
  roster: readonly SpaceRosterMember[],
): PresenceReading {
  const names = new Map<string, string>()
  for (const member of roster) {
    const id = member.subject_id?.trim()
    const name = member.display_name?.trim()
    if (id && name) names.set(id, name)
  }
  const here: string[] = []
  const typing: string[] = []
  let hereCount = 0
  let typingCount = 0
  for (const member of present ?? []) {
    const id = member.subject_id?.trim()
    if (!id) continue
    hereCount += 1
    const name = names.get(id)
    if (name) here.push(name)
    if (member.status === 'typing') {
      typingCount += 1
      if (name) typing.push(name)
    }
  }
  here.sort((a, b) => a.localeCompare(b))
  typing.sort((a, b) => a.localeCompare(b))
  return { hereNames: here, hereCount, typingNames: typing, typingCount }
}

/**
 * "Kari", "Kari og Ola", "Kari, Ola og 2 andre".
 *
 * `count` is the real total, which can exceed the names: the remainder is
 * stated as a number rather than dropped, so the sentence never claims fewer
 * people are present than are.
 */
export function nameList(
  names: readonly string[],
  count: number,
  tr: (no: string, en: string) => string,
): string {
  const shown = names.slice(0, NAMED_LIMIT)
  const rest = count - shown.length
  if (shown.length === 0) {
    return count === 1
      ? tr('1 person', '1 person')
      : tr(`${count} personer`, `${count} people`)
  }
  if (rest <= 0) {
    if (shown.length === 1) return shown[0] as string
    return `${shown[0]} ${tr('og', 'and')} ${shown[1]}`
  }
  return `${shown.join(', ')} ${tr('og', 'and')} ${tr(`${rest} andre`, `${rest} ${rest === 1 ? 'other' : 'others'}`)}`
}

/** "Kari og Ola er her nå" — or nothing at all when the room is yours alone. */
export function hereSentence(
  reading: PresenceReading,
  tr: (no: string, en: string) => string,
): string | undefined {
  if (reading.hereCount === 0) return undefined
  const who = nameList(reading.hereNames, reading.hereCount, tr)
  return reading.hereCount === 1
    ? tr(`${who} er her nå`, `${who} is here now`)
    : tr(`${who} er her nå`, `${who} are here now`)
}

/** "Kari skriver …" — the room's own typing indicator. */
export function typingSentence(
  reading: PresenceReading,
  tr: (no: string, en: string) => string,
): string | undefined {
  if (reading.typingCount === 0) return undefined
  const who = nameList(reading.typingNames, reading.typingCount, tr)
  return reading.typingCount === 1
    ? tr(`${who} skriver …`, `${who} is writing…`)
    : tr(`${who} skriver …`, `${who} are writing…`)
}
