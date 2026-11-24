import { createEffect, For, Show } from 'solid-js'

import { ChatMarkdown } from '@/features/chat/components/ChatMessages'

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

/** Elements whose text is never a mention: a link keeps its own text, code is literal, a mark is already one. */
const SKIP_MENTION_INSIDE = new Set(['A', 'CODE', 'PRE', 'KBD', 'SAMP', 'MARK', 'SCRIPT', 'STYLE', 'TEXTAREA', 'BUTTON'])

/**
 * DEPTH-WS08-S6-2: pick out the mentions in rendered markdown. Only text nodes are touched, and
 * never one inside a link, code or an existing mark, so a URL or an inline code span stays as the
 * author wrote it. Each new node is a text node or a `<mark>` holding a text node, so nothing the
 * author typed is ever parsed as HTML. Running it twice changes nothing.
 */
export function highlightMentionsIn(root: HTMLElement, names: readonly string[], viewerName?: string): void {
  const doc = root.ownerDocument
  const walker = doc.createTreeWalker(root, 4 /* NodeFilter.SHOW_TEXT */)
  const targets: Text[] = []
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    let skip = false
    for (let parent = node.parentElement; parent && parent !== root; parent = parent.parentElement) {
      if (SKIP_MENTION_INSIDE.has(parent.tagName)) {
        skip = true
        break
      }
    }
    if (!skip && (node.nodeValue ?? '').includes('@')) targets.push(node as Text)
  }
  for (const node of targets) {
    const pieces = splitMentions(node.nodeValue ?? '', names, viewerName)
    if (!pieces.some((piece) => piece.mention)) continue
    const fragment = doc.createDocumentFragment()
    for (const piece of pieces) {
      if (!piece.mention) {
        fragment.appendChild(doc.createTextNode(piece.text))
        continue
      }
      const mark = doc.createElement('mark')
      mark.className = 'verevon-space-mention'
      if (piece.mention === 'self') mark.dataset.self = 'true'
      mark.appendChild(doc.createTextNode(piece.text))
      fragment.appendChild(mark)
    }
    node.parentNode?.replaceChild(fragment, node)
  }
}

export interface MentionMarkdownProps extends MentionTextProps {
  /** Class of the wrapping element. */
  readonly class?: string
}

/**
 * A person's post as markdown with its mentions picked out (DEPTH-WS08-S6-2). ChatMarkdown renders
 * the words as before; the highlight is applied to its text nodes once they are in the document.
 * The subtree is rebuilt whenever the text or the names change, so the marks never fight a later
 * update of the markdown for the same nodes.
 */
export function MentionMarkdown(props: MentionMarkdownProps) {
  const key = () => JSON.stringify([props.text, props.names, props.viewerName ?? ''])
  return (
    <Show when={key()} keyed>
      {(_key) => <MentionMarkdownBody text={props.text} names={props.names} viewerName={props.viewerName} class={props.class} />}
    </Show>
  )
}

function MentionMarkdownBody(props: MentionMarkdownProps) {
  let el: HTMLDivElement | undefined
  // Solid v2 has no onMount: this effect runs once the markdown is in the document.
  createEffect(
    () => undefined,
    () => {
      if (el) highlightMentionsIn(el, props.names, props.viewerName)
    },
  )
  return (
    <div class={props.class} ref={el}>
      <ChatMarkdown content={props.text} />
    </div>
  )
}
