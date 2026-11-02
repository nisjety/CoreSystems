import { For } from 'solid-js'

import { foldLabel, mentionPresent } from '../lib/mention-model'

/**
 * A person's post as plain text with its mentions picked out (WS08-S6). A name in the room's
 * roster or agent list that follows an `@` is wrapped; the reader's own name is emphasized so
 * "someone mentioned you" is visible at a glance. Display only: the text is rendered as text
 * nodes (never as HTML), and a mention here proves nothing, since the record is the server's.
 */

export interface MentionTextProps {
  readonly text: string
  /** Every name that can be mentioned in this room (people and agents). */
  readonly names: readonly string[]
  /** The reading member's own display name, when known. */
  readonly viewerName?: string
}

type Piece = { readonly text: string; readonly mention?: 'other' | 'self' }

/** Split `text` into plain runs and `@name` runs. Longest names first, so `@Kari Nordmann` wins over `@Kari`. */
export function splitMentions(text: string, names: readonly string[], viewerName?: string): readonly Piece[] {
  const candidates = [...new Set(names.map((name) => name.trim()).filter(Boolean))]
    .filter((name) => mentionPresent(text, name))
    .sort((a, b) => b.length - a.length)
  if (candidates.length === 0) return [{ text }]
  const pieces: Piece[] = []
  let plain = ''
  let index = 0
  const viewer = viewerName ? foldLabel(viewerName) : undefined
  while (index < text.length) {
    let matched: string | undefined
    if (text[index] === '@' && (index === 0 || /\s/u.test(text[index - 1] ?? ''))) {
      matched = candidates.find((name) => {
        const end = index + 1 + name.length
        return foldLabel(text.slice(index + 1, end)) === foldLabel(name) && !/[\p{L}\p{N}_]/u.test(text[end] ?? '')
      })
    }
    if (matched) {
      if (plain) pieces.push({ text: plain })
      plain = ''
      pieces.push({
        text: text.slice(index, index + 1 + matched.length),
        mention: viewer && foldLabel(matched) === viewer ? 'self' : 'other',
      })
      index += 1 + matched.length
    } else {
      plain += text[index]
      index += 1
    }
  }
  if (plain) pieces.push({ text: plain })
  return pieces
}

export function MentionText(props: MentionTextProps) {
  return (
    <For each={splitMentions(props.text, props.names, props.viewerName)}>
      {(piece) =>
        piece.mention ? <mark class="verevon-space-mention" data-self={piece.mention === 'self' ? 'true' : undefined}>{piece.text}</mark> : <>{piece.text}</>
      }
    </For>
  )
}
