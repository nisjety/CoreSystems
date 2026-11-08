/**
 * Conversation outline (F-t3-098): the headings in the assistant's answers, in order, for a table of contents that
 * jumps to the turn holding each one.
 *
 * Pure. It reads only the text the transcript already shows, so nothing is stored, sent or translated, and it is safe
 * for temporary chats and ZDR turns alike. A streaming turn is left out until it settles, so the outline does not move
 * under the reader while an answer is still arriving. Behind `VITE_CHAT_READER_POLISH`, which the page reads.
 */

import type { ChatTurn } from '../components/chat-types'

/** A table of contents needs at least two entries. One heading alone is not worth a control. */
export const CHAPTER_TOC_MIN_ENTRIES = 2
/** The most recent entries kept, so a very long conversation keeps its outline cheap to render. */
export const CHAPTER_TOC_MAX_ENTRIES = 60
/** Longer headings are cut to this many characters, ending in an ellipsis. */
export const CHAPTER_TITLE_MAX_CHARS = 120

export type ChapterLevel = 1 | 2 | 3

export type ChapterEntry = {
  turnId: string
  level: ChapterLevel
  title: string
}

export type ChapterSource = Pick<ChatTurn, 'id' | 'role' | 'content' | 'streaming'>

type Heading = { level: ChapterLevel; title: string }

const OPENING_FENCE = /^ {0,3}(`{3,}|~{3,})/
const CLOSING_FENCE = /^ {0,3}(`{3,}|~{3,})[ \t]*$/
const ATX_HEADING = /^ {0,3}(#{1,6})(?:[ \t]+(.*))?$/

/** Strips the markup a heading can carry, so the outline reads as plain text. The words of links and code stay. */
function plainTitle(raw: string): string {
  const text = raw
    .replace(/[ \t]+#+[ \t]*$/, '')
    .replace(/\[([^\]]*)\]\([^)]*\)/g, '$1')
    .replace(/`([^`]*)`/g, '$1')
    .replace(/(\*\*|__)(.+?)\1/g, '$2')
    .replace(/\s+/g, ' ')
    .trim()
  const chars = Array.from(text)
  return chars.length > CHAPTER_TITLE_MAX_CHARS ? `${chars.slice(0, CHAPTER_TITLE_MAX_CHARS - 1).join('')}…` : text
}

/** The ATX headings of one answer, outside fenced code. Levels past three are shown at level three. */
export function headingsIn(markdown: string): Heading[] {
  const headings: Heading[] = []
  let fence: string | undefined
  for (const line of markdown.split(/\r?\n/)) {
    if (fence !== undefined) {
      const closing = CLOSING_FENCE.exec(line)?.[1]
      if (closing && closing[0] === fence[0] && closing.length >= fence.length) fence = undefined
      continue
    }
    const opening = OPENING_FENCE.exec(line)?.[1]
    if (opening) {
      fence = opening
      continue
    }
    const heading = ATX_HEADING.exec(line)
    if (!heading) continue
    const title = plainTitle(heading[2] ?? '')
    if (title) headings.push({ level: Math.min(heading[1]?.length ?? 1, 3) as ChapterLevel, title })
  }
  return headings
}

/**
 * The headings of each turn, kept by turn id while the turn's text is unchanged. The transcript recomputes the
 * outline on every streamed token, so a settled answer must not be scanned again each time.
 */
const parsed = new Map<string, { content: string; headings: Heading[] }>()
const PARSED_CAP = 500

function headingsOf(turn: ChapterSource): Heading[] {
  const seen = parsed.get(turn.id)
  if (seen && seen.content === turn.content) return seen.headings
  const headings = headingsIn(turn.content)
  parsed.delete(turn.id)
  parsed.set(turn.id, { content: turn.content, headings })
  if (parsed.size > PARSED_CAP) {
    const oldest = parsed.keys().next().value
    if (oldest !== undefined) parsed.delete(oldest)
  }
  return headings
}

/** The outline of a thread: the headings of its settled assistant turns, in order, the most recent entries kept. */
export function chapterEntries(turns: readonly ChapterSource[]): ChapterEntry[] {
  const entries: ChapterEntry[] = []
  for (const turn of turns) {
    if (turn.role !== 'assistant' || turn.streaming) continue
    for (const heading of headingsOf(turn)) {
      entries.push({ turnId: turn.id, level: heading.level, title: heading.title })
    }
  }
  return entries.slice(-CHAPTER_TOC_MAX_ENTRIES)
}

/** A selector for the element that holds one turn. The id is escaped, so no id can end the attribute early. */
export function turnSelector(turnId: string): string {
  return `[data-turn-id="${turnId.replace(/["\\]/g, '\\$&')}"]`
}
