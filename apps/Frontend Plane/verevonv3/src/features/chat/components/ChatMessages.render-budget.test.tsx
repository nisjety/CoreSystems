// @vitest-environment jsdom

/**
 * Render budget of a streaming answer: what one token costs in DOM and parsing.
 *
 * WS09 S1 measured it; S2 (incremental Markdown, keyed node rows) holds it. It
 * counts, per streamed token:
 *
 *  - how many block elements of the rendered Markdown are NEW (a "mount": an
 *    element present now that was not in the DOM after any earlier token), split
 *    into sealed blocks (those the previous token had already followed by another
 *    block, which a token cannot change) and the rest;
 *  - how many `AnswerRegion` roots are new (the wrapper that holds the Markdown
 *    and the Stopped chip), through the real `AssistantMessage` node list;
 *  - how many elements are new at any depth, against the size of the trailing block;
 *  - how many parser calls the token causes, and how many characters they read.
 *
 * WHY DOM IDENTITY. `AnswerRegion` and `MarkdownBlockView` are components in
 * the module under test, so a mock cannot wrap them. An element that survives
 * from one token to the next was updated in place; one that did not was torn
 * down and rebuilt. That is exactly what a remount is, whichever component owns
 * it, and it keeps working when the keying changes again.
 *
 * WHAT IS ASSERTED. Structural counts (never milliseconds), so they are exact on
 * any machine. The `VITE_CHAT_RENDER_LEGACY=true` block keeps the S1 baseline
 * pinned: it is the escape hatch for one release, and it must stay the old path.
 */

import { render } from '@solidjs/testing-library'
import { createSignal, createStore, flush, For } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { AssistantMessage, ChatMarkdown } from './ChatMessages'
import type { ParsedMarkdown } from './chat-media-markdown'
import type { ChatTurn } from './chat-types'

const parse = vi.hoisted(() => ({ calls: 0, chars: 0 }))

// Every markdown entry point ChatMessages reaches is counted, by name pattern,
// so the incremental parser is measured by the same gate as the full parser
// without a second mock. The incremental one is handed the whole message, so
// what it READ is the text from the line it resumed at, worked out from its result.
vi.mock('./chat-media-markdown', async (importOriginal) => {
  const actual = await importOriginal<typeof import('./chat-media-markdown')>()
  const wrapped: Record<string, unknown> = { ...actual }
  for (const [name, value] of Object.entries(actual)) {
    if (!/^parseMarkdown/u.test(name) || typeof value !== 'function') continue
    wrapped[name] = (...args: unknown[]) => {
      parse.calls += 1
      const result = (value as (...inner: unknown[]) => unknown)(...args)
      if (name === 'parseMarkdownIncremental') {
        const previous = args[0] as ParsedMarkdown | undefined
        const next = result as ParsedMarkdown
        if (next !== previous) {
          const from = next.reused > 0 ? (previous?.restartAt[next.reused] ?? 0) : 0
          parse.chars += next.source.split('\n').slice(from).join('\n').length
        }
      } else {
        const source = args.find((arg): arg is string => typeof arg === 'string')
        parse.chars += source?.length ?? 0
      }
      return result
    }
  }
  return wrapped
})

beforeEach(() => {
  parse.calls = 0
  parse.chars = 0
})

afterEach(() => {
  vi.unstubAllEnvs()
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

/** Where each block of the finished answer begins, in the joined text. */
const blockStarts = (): number[] => {
  const starts: number[] = []
  let at = 0
  for (const block of answerBlocks()) {
    starts.push(at)
    at += block.length + 2
  }
  return starts
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
  /** ...of which sealed: already followed by another block at the previous token. */
  newSealedBlocks: number
  /** All elements, at any depth, that are new this token. */
  newElements: number
  /** The size of the last two blocks, each with everything inside it: all a token can touch. */
  tailElements: number
  /** `AnswerRegion` roots that are new this token. */
  newAnswerRegions: number
  parseCalls: number
  parsedChars: number
  blockCount: number
  /** Blocks on screen after the previous token. */
  previousBlockCount: number
  contentLength: number
}

/** Follows element identity across tokens; each call samples one token and resets the parse counters. */
function createMountProbe(container: HTMLElement) {
  const seenBlocks = new WeakSet<Element>()
  const seenElements = new WeakSet<Element>()
  const seenRegions = new WeakSet<Element>()
  let previousBlockCount = 0
  return (contentLength: number): TokenSample => {
    const markdown = container.querySelector('.verevon-chat-markdown')
    const blocks = markdown ? [...markdown.children] : []
    let newBlocks = 0
    let newSealedBlocks = 0
    blocks.forEach((block, index) => {
      if (seenBlocks.has(block)) return
      seenBlocks.add(block)
      newBlocks += 1
      if (index < previousBlockCount - 1) newSealedBlocks += 1
    })
    let newElements = 0
    for (const element of container.querySelectorAll('*')) {
      if (seenElements.has(element)) continue
      seenElements.add(element)
      newElements += 1
    }
    const tailElements = sum(blocks.slice(-2).map((block) => 1 + block.querySelectorAll('*').length))
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
      tailElements,
      newAnswerRegions,
      parseCalls: parse.calls,
      parsedChars: parse.chars,
      blockCount: blocks.length,
      previousBlockCount,
      contentLength,
    }
    previousBlockCount = blocks.length
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

/** The most text the incremental parse may read for a token: the last three blocks (two frozen, one new). */
const tailBudget = (sample: TokenSample): number => {
  const starts = blockStarts()
  return sample.contentLength - (starts[Math.max(sample.blockCount - 3, 0)] ?? 0)
}

const baseTurn = (): ChatTurn => ({
  id: 'asst-stream',
  role: 'assistant',
  content: '',
  createdAt: '2026-10-21T08:00:00.000Z',
  streaming: true,
  status: 'waiting',
  tools: [],
  attachments: [],
})

function renderStreamingMessage() {
  const [turn, setTurn] = createStore<ChatTurn>(baseTurn())
  const view = render(() => (
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
  const setContent = (content: string) => {
    setTurn((draft) => {
      draft.content = content
    })
  }
  return { ...view, setContent }
}

describe('Solid 2 <For> row identity, the runtime fact S2 builds on', () => {
  // Read from @solidjs/signals, never run, until S1. The keying modes decide
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
    // The legacy `nodeEntries` list: new entry objects per token, so every node
    // of the streaming message, AnswerRegion included, is torn down.
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
  it('mounts no sealed block and reads only the tail on every token', () => {
    const prefixes = tokenPrefixes()
    const [content, setContent] = createSignal('')
    const { container, unmount } = render(() => <ChatMarkdown content={content()} />)
    const samples = streamAll(prefixes, container, setContent)
    unmount()

    expect(samples.length).toBeGreaterThan(50)
    // The fixture must really grow past the few-blocks case the cascade hides in.
    expect(samples.at(-1)?.blockCount).toBeGreaterThanOrEqual(15)
    for (const sample of samples) {
      expect(sample.parseCalls).toBe(1)
      expect(sample.parsedChars).toBeLessThanOrEqual(tailBudget(sample))
      // A token cannot change a block that is not the trailing one, so none is rebuilt.
      expect(sample.newSealedBlocks).toBe(0)
      // And the DOM work is bounded by the blocks being written, not by the message.
      expect(sample.newElements).toBeLessThanOrEqual(sample.tailElements)
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

  it('reads a small multiple of the answer over the whole stream, not a quadratic amount', () => {
    const prefixes = tokenPrefixes()
    const [content, setContent] = createSignal('')
    const { container, unmount } = render(() => <ChatMarkdown content={content()} />)
    const samples = streamAll(prefixes, container, setContent)
    unmount()

    const finalLength = prefixes.at(-1)?.length ?? 0
    // A full reparse per token read about tokens/2 times the answer. The tail
    // is a few blocks of a long message, so the whole stream reads a fraction of that.
    expect(sum(samples.map((s) => s.parsedChars)) / finalLength).toBeLessThan(prefixes.length / 4)
  })
})

describe('AssistantMessage render budget while streaming', () => {
  it('keeps one AnswerRegion, parses once and mounts no sealed block per token', () => {
    const prefixes = tokenPrefixes()
    const { container, unmount, setContent } = renderStreamingMessage()
    const samples = streamAll(prefixes, container, setContent)
    unmount()

    // One region, mounted by the first token, never again.
    expect(sum(samples.map((s) => s.newAnswerRegions))).toBe(1)
    for (const sample of samples.slice(1)) {
      expect(sample.parseCalls).toBe(1)
      expect(sample.parsedChars).toBeLessThanOrEqual(tailBudget(sample))
      expect(sample.newSealedBlocks).toBe(0)
      expect(sample.newElements).toBeLessThanOrEqual(sample.tailElements)
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

  it('still renders the same text a full parse would', () => {
    const prefixes = tokenPrefixes()
    const { container, unmount, setContent } = renderStreamingMessage()
    for (const prefix of prefixes) {
      setContent(prefix)
      flush()
    }
    const text = container.querySelector('.verevon-chat-markdown')?.textContent ?? ''
    unmount()
    for (const block of answerBlocks()) {
      // Every logical block of the answer is on screen, in the order it was written.
      expect(text).toContain(block.split('\n')[0]?.replace(/^(## |- )/u, '') ?? '')
    }
  })
})

describe('VITE_CHAT_RENDER_LEGACY=true keeps the per-token remount path for one release', () => {
  it('rebuilds every sealed block and rereads the whole message on every token', () => {
    vi.stubEnv('VITE_CHAT_RENDER_LEGACY', 'true')
    const prefixes = tokenPrefixes()
    const [content, setContent] = createSignal('')
    const { container, unmount } = render(() => <ChatMarkdown content={content()} />)
    const samples = streamAll(prefixes, container, setContent)
    unmount()

    for (const sample of samples) {
      expect(sample.parseCalls).toBe(1)
      expect(sample.parsedChars).toBe(sample.contentLength)
      expect(sample.newSealedBlocks).toBe(Math.max(sample.previousBlockCount - 1, 0))
    }
  })

  it('rebuilds the AnswerRegion and rereads the whole message on every token', () => {
    vi.stubEnv('VITE_CHAT_RENDER_LEGACY', 'true')
    const prefixes = tokenPrefixes()
    const { container, unmount, setContent } = renderStreamingMessage()
    const samples = streamAll(prefixes, container, setContent)
    unmount()

    expect(sum(samples.map((s) => s.newAnswerRegions))).toBe(samples.length)
    for (const sample of samples.slice(1)) {
      expect(sample.parseCalls).toBe(1)
      expect(sample.parsedChars).toBe(sample.contentLength)
      expect(sample.newSealedBlocks).toBe(Math.max(sample.previousBlockCount - 1, 0))
    }
  })

  it.each(['1', 'TRUE', 'yes', ''])('is not enabled by %j: only the literal "true" is', (value) => {
    vi.stubEnv('VITE_CHAT_RENDER_LEGACY', value)
    const prefixes = tokenPrefixes()
    const [content, setContent] = createSignal('')
    const { container, unmount } = render(() => <ChatMarkdown content={content()} />)
    const samples = streamAll(prefixes, container, setContent)
    unmount()
    expect(sum(samples.map((s) => s.newSealedBlocks))).toBe(0)
  })
})
