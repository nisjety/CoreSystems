// @vitest-environment jsdom

/**
 * Render budget of a streaming answer: what one token costs in DOM and parsing.
 *
 * WS09 S1 ("measure first"). The transcript-performance work (incremental
 * Markdown, no per-token churn, ...) was written on a reading of the Solid 2
 * runtime, not a run of it. This file runs it. It counts, per streamed token:
 *
 *  - how many block elements of the rendered Markdown are NEW (a "mount": an
 *    element present now that was not in the DOM after any earlier token), split
 *    into sealed blocks (everything before the trailing one, which a token can
 *    never change) and the trailing block;
 *  - how many `AnswerRegion` roots are new (the wrapper that holds the Markdown
 *    and the Stopped chip), through the real `AssistantMessage` node list;
 *  - how many parser calls the token causes, and how many characters they read.
 *
 * WHY DOM IDENTITY. `AnswerRegion` and `MarkdownBlockView` are components in
 * the module under test, so a mock cannot wrap them. An element that survives
 * from one token to the next was updated in place; one that did not was torn
 * down and rebuilt. That is exactly what a remount is, whichever component owns
 * it, and it keeps working when S2 changes how the blocks are keyed.
 *
 * WHAT IS ASSERTED. These are baseline pins of today's behaviour, written as
 * structural counts (never milliseconds), so they are exact on any machine. The
 * assertions carry an "S2 target" note: when S2 lands it flips exactly those
 * lines (sealed mounts 0, `AnswerRegion` mounts 1, parse work bounded by the
 * trailing block). A change that moves a number without flipping its pin fails
 * here, which is the point: the budget cannot drift unseen.
 */

import { render } from '@solidjs/testing-library'
import { createSignal, createStore, flush, For } from 'solid-js'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { AssistantMessage, ChatMarkdown } from './ChatMessages'
import type { ChatTurn } from './chat-types'

const parse = vi.hoisted(() => ({ calls: 0, chars: 0 }))

// Every markdown entry point ChatMessages reaches is counted, by name pattern,
// so S2's `parseMarkdownIncremental` is measured by the same gate as the full
// parser without a second mock.
vi.mock('./chat-media-markdown', async (importOriginal) => {
  const actual = await importOriginal<typeof import('./chat-media-markdown')>()
  const wrapped: Record<string, unknown> = { ...actual }
  for (const [name, value] of Object.entries(actual)) {
    if (!/^parseMarkdown/u.test(name) || typeof value !== 'function') continue
    wrapped[name] = (...args: unknown[]) => {
      parse.calls += 1
      const source = args.find((arg): arg is string => typeof arg === 'string')
      parse.chars += source?.length ?? 0
      return (value as (...inner: unknown[]) => unknown)(...args)
    }
  }
  return wrapped
})

beforeEach(() => {
  parse.calls = 0
  parse.chars = 0
})

/**
 * A long answer shaped like a real one: a heading, then paragraphs and lists, so
 * the block count grows while the answer streams. Deterministic, no randomness.
 */
const PARAGRAPHS = 18
const answerBlocks = (): string[] => {
  const blocks: string[] = ['## Kvartalsoversikt']
  for (let index = 0; index < PARAGRAPHS; index += 1) {
    if (index % 6 === 3) {
      blocks.push(
        `- Punkt ${index} om omsetning og margin\n- Punkt ${index} om kunder i offentlig sektor\n- Punkt ${index} om risiko`,
      )
    } else {
      blocks.push(
        `Avsnitt ${index}: omsetningen i kvartalet endte pa 4,2 millioner kroner, opp 12 prosent ` +
          'fra samme kvartal i fjor, og veksten kom hovedsakelig fra nye kunder i offentlig sektor.',
      )
    }
  }
  return blocks
}

/** The answer cut into the deltas a model streams: about six words each. */
const tokenPrefixes = (): string[] => {
  const full = answerBlocks().join('\n\n')
  const words = full.split(/(?<=\s)/u)
  const prefixes: string[] = []
  let acc = ''
  words.forEach((word, index) => {
    acc += word
    if ((index + 1) % 6 === 0 || index === words.length - 1) prefixes.push(acc)
  })
  return prefixes
}

type TokenSample = {
  /** Block elements of the markdown that were not in the DOM after any earlier token. */
  newBlocks: number
  /** ...of which sealed: not the trailing block, so a token cannot have changed it. */
  newSealedBlocks: number
  /** All elements, at any depth, that are new this token. */
  newElements: number
  /** `AnswerRegion` roots that are new this token. */
  newAnswerRegions: number
  parseCalls: number
  parsedChars: number
  blockCount: number
  contentLength: number
}

/** Follows element identity across tokens; each call samples one token and resets the parse counters. */
function createMountProbe(container: HTMLElement) {
  const seenBlocks = new WeakSet<Element>()
  const seenElements = new WeakSet<Element>()
  const seenRegions = new WeakSet<Element>()
  return (contentLength: number): TokenSample => {
    const markdown = container.querySelector('.verevon-chat-markdown')
    const blocks = markdown ? [...markdown.children] : []
    const lastIndex = blocks.length - 1
    let newBlocks = 0
    let newSealedBlocks = 0
    blocks.forEach((block, index) => {
      if (seenBlocks.has(block)) return
      seenBlocks.add(block)
      newBlocks += 1
      if (index < lastIndex) newSealedBlocks += 1
    })
    let newElements = 0
    for (const element of container.querySelectorAll('*')) {
      if (seenElements.has(element)) continue
      seenElements.add(element)
      newElements += 1
    }
    // The answer region's own root is the wrapper that holds the markdown.
    const region = markdown?.parentElement ?? null
    let newAnswerRegions = 0
    if (region && !seenRegions.has(region)) {
      seenRegions.add(region)
      newAnswerRegions = 1
    }
    const sample: TokenSample = {
      newBlocks,
      newSealedBlocks,
      newElements,
      newAnswerRegions,
      parseCalls: parse.calls,
      parsedChars: parse.chars,
      blockCount: blocks.length,
      contentLength,
    }
    parse.calls = 0
    parse.chars = 0
    return sample
  }
}

const sum = (values: number[]) => values.reduce((total, value) => total + value, 0)

/** Streams every prefix into `setContent`, sampling after each flush. */
function streamAll(
  prefixes: string[],
  container: HTMLElement,
  setContent: (content: string) => void,
): TokenSample[] {
  const probe = createMountProbe(container)
  probe(0)
  const samples: TokenSample[] = []
  for (const prefix of prefixes) {
    setContent(prefix)
    flush()
    samples.push(probe(prefix.length))
  }
  return samples
}

describe('Solid 2 <For> row identity, the runtime fact S2 builds on', () => {
  // Read from @solidjs/signals, never run, until this. The keying modes decide
  // whether a re-derived list rebuilds its rows or updates them.
  type Row = { id: string; text: string }
  const rowsAfterReplacingItems = (mode: 'reference' | 'accessor' | 'id') => {
    const [items, setItems] = createSignal<Row[]>([
      { id: 'a', text: 'one' },
      { id: 'b', text: 'two' },
    ])
    const container = document.createElement('ul')
    document.body.append(container)
    const { unmount } = render(
      () => {
        if (mode === 'accessor') {
          return <For each={items()} keyed={false}>{(item) => <li>{item().text}</li>}</For>
        }
        if (mode === 'id') {
          return <For each={items()} keyed={(item) => item.id}>{(item) => <li>{item().text}</li>}</For>
        }
        return <For each={items()}>{(item) => <li>{item.text}</li>}</For>
      },
      { container },
    )
    const before = [...container.querySelectorAll('li')]
    // The same rows as brand-new objects: what a memo that rebuilds its entries does.
    setItems([
      { id: 'a', text: 'one more' },
      { id: 'b', text: 'two more' },
    ])
    flush()
    const after = [...container.querySelectorAll('li')]
    const result = {
      kept: after.filter((element, index) => element === before[index]).length,
      texts: after.map((element) => element.textContent),
    }
    unmount()
    container.remove()
    return result
  }

  it('the default (reference-keyed) <For> rebuilds every row when the item objects are replaced', () => {
    // This is ChatMessages `nodeEntries`: new entry objects per token, so every
    // node of the streaming message, AnswerRegion included, is torn down.
    expect(rowsAfterReplacingItems('reference')).toEqual({ kept: 0, texts: ['one more', 'two more'] })
  })

  it('keyed={false} updates every row in place', () => {
    expect(rowsAfterReplacingItems('accessor')).toEqual({ kept: 2, texts: ['one more', 'two more'] })
  })

  it('keyed by an id function keeps rows whose id survives', () => {
    expect(rowsAfterReplacingItems('id')).toEqual({ kept: 2, texts: ['one more', 'two more'] })
  })
})

describe('ChatMarkdown render budget while streaming', () => {
  it('rebuilds every sealed block and rereads the whole message on every token (S2 flips this)', () => {
    const prefixes = tokenPrefixes()
    const [content, setContent] = createSignal('')
    const { container, unmount } = render(() => <ChatMarkdown content={content()} />)
    const samples = streamAll(prefixes, container, setContent)
    unmount()

    expect(samples.length).toBeGreaterThan(50)
    // The fixture must really grow past the few-blocks case the cascade hides in.
    expect(samples.at(-1)?.blockCount).toBeGreaterThanOrEqual(15)
    for (const sample of samples) {
      // One parse per token, over the whole accumulated message. S2 target:
      // the parse reads only the unsealed tail.
      expect(sample.parseCalls).toBe(1)
      expect(sample.parsedChars).toBe(sample.contentLength)
      // Every block but the trailing one is rebuilt, though a token cannot have
      // changed it. S2 target: 0.
      expect(sample.newSealedBlocks).toBe(sample.blockCount - 1)
    }
    console.info('[render-budget] markdown', JSON.stringify({
      tokens: samples.length,
      finalBlocks: samples.at(-1)?.blockCount,
      sealedMounts: sum(samples.map((s) => s.newSealedBlocks)),
      blockMounts: sum(samples.map((s) => s.newBlocks)),
      elementMounts: sum(samples.map((s) => s.newElements)),
      parsedChars: sum(samples.map((s) => s.parsedChars)),
    }))
  })

  it('reads a quadratic amount of text over the whole stream', () => {
    const prefixes = tokenPrefixes()
    const [content, setContent] = createSignal('')
    const { container, unmount } = render(() => <ChatMarkdown content={content()} />)
    const samples = streamAll(prefixes, container, setContent)
    unmount()

    const finalLength = prefixes.at(-1)?.length ?? 0
    // A full reparse per token reads about tokens/2 times the answer, not about
    // once. S2 target: a small constant multiple of the answer's length.
    expect(sum(samples.map((s) => s.parsedChars)) / finalLength).toBeGreaterThan(prefixes.length / 4)
  })
})

describe('AssistantMessage render budget while streaming', () => {
  it('rebuilds the AnswerRegion and parses twice on every token (S2 flips this)', () => {
    const prefixes = tokenPrefixes()
    const base: ChatTurn = {
      id: 'asst-stream',
      role: 'assistant',
      content: '',
      createdAt: '2026-10-21T08:00:00.000Z',
      streaming: true,
      status: 'waiting',
      tools: [],
      attachments: [],
    }
    const [turn, setTurn] = createStore<ChatTurn>(base)
    const { container, unmount } = render(() => (
      <AssistantMessage
        copied={false}
        message={turn}
        onBranch={() => {}}
        onCopy={() => {}}
        onFeedback={() => Promise.resolve(true)}
        onRegenerate={() => {}}
        onApprovalDecision={() => {}}
        onApprovePlan={() => {}}
        onViewSteps={() => {}}
      />
    ))
    const samples = streamAll(prefixes, container, (content) => {
      setTurn((draft) => {
        draft.content = content
      })
    })
    unmount()

    // A new region on every token. S2 target: exactly one, on the first token.
    expect(sum(samples.map((s) => s.newAnswerRegions))).toBe(samples.length)
    for (const sample of samples.slice(1)) {
      // The node list is rebuilt per token and the region remounts, so the
      // message is parsed by the instance being replaced and by its successor.
      // S2 target: 1 parse, over the tail only.
      expect(sample.parseCalls).toBe(2)
      expect(sample.parsedChars).toBe(2 * sample.contentLength)
      expect(sample.newSealedBlocks).toBe(sample.blockCount - 1)
    }
    console.info('[render-budget] assistant', JSON.stringify({
      tokens: samples.length,
      answerRegionMounts: sum(samples.map((s) => s.newAnswerRegions)),
      sealedMounts: sum(samples.map((s) => s.newSealedBlocks)),
      elementMounts: sum(samples.map((s) => s.newElements)),
      parseCalls: sum(samples.map((s) => s.parseCalls)),
      parsedChars: sum(samples.map((s) => s.parsedChars)),
    }))
  })
})
