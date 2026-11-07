import { cleanup, fireEvent, render } from '@solidjs/testing-library'
import { createSignal, For, flush, Show } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { SettledTurnContent, SettledTurnFold } from './SettledTurnFold'
import type { ConversationNode, ConversationNodeContext, ConversationNodeEntry } from '@/shared/chat-nodes'
import type { SettledFold } from '../lib/settled-fold'
import { createSettledFoldState } from '../lib/settled-fold-state'

afterEach(() => {
  cleanup()
  flush()
})

/** `onSettled` runs once the first render has settled, so let that happen before a test acts. */
async function settle(): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, 0))
  flush()
}

const fold = (overrides: Partial<SettledFold> = {}): SettledFold => ({
  defaultFolded: false,
  steps: 2,
  files: 1,
  durationMs: 252_000,
  ...overrides,
})

const entry = (kind: ConversationNode['kind']): ConversationNodeEntry =>
  ({ node: { kind } as ConversationNode, context: {} as ConversationNodeContext })

/** A stand-in for the transcript's keyed list: one element per entry, tagged with its kind. */
const renderEntries = (entries: () => ConversationNodeEntry[]) => (
  <For each={entries()}>{(item) => <i data-kind={item.node.kind}>{item.node.kind}</i>}</For>
)

/** The folded state as the view holds it: the row reads it, and asks for a change through the setter. */
function viewState(initialFolded: boolean) {
  const [folded, setFolded] = createSignal(initialFolded)
  return { folded, setFolded }
}

describe('SettledTurnFold', () => {
  it('is folded, with the body hidden until found, when its owner says so', async () => {
    const view = render(() => (
      <SettledTurnFold fold={fold()} folded onFoldedChange={() => {}}>
        <p>Trinn</p>
      </SettledTurnFold>
    ))
    await settle()
    const toggle = view.container.querySelector('.verevon-chat-settled-fold__toggle')
    const body = view.container.querySelector('.verevon-chat-settled-fold__body')
    expect(toggle?.getAttribute('aria-expanded')).toBe('false')
    expect(body?.getAttribute('hidden')).toBe('until-found')
  })

  it('is open when its owner says so', async () => {
    const view = render(() => (
      <SettledTurnFold fold={fold()} folded={false} onFoldedChange={() => {}}>
        <p>Trinn</p>
      </SettledTurnFold>
    ))
    await settle()
    expect(view.container.querySelector('.verevon-chat-settled-fold__toggle')?.getAttribute('aria-expanded')).toBe('true')
    expect(view.container.querySelector('.verevon-chat-settled-fold__body')?.hasAttribute('hidden')).toBe(false)
  })

  it('keeps no folded state of its own: a click only asks, and the row waits for its owner', async () => {
    const onFoldedChange = vi.fn()
    const view = render(() => (
      <SettledTurnFold fold={fold()} folded onFoldedChange={onFoldedChange}>
        <p>Trinn</p>
      </SettledTurnFold>
    ))
    await settle()
    const toggle = view.container.querySelector<HTMLButtonElement>('.verevon-chat-settled-fold__toggle')!
    fireEvent.click(toggle)
    flush()
    expect(onFoldedChange).toHaveBeenCalledWith(false)
    expect(toggle.getAttribute('aria-expanded')).toBe('false')
    expect(view.container.querySelector('.verevon-chat-settled-fold__body')?.getAttribute('hidden')).toBe('until-found')
  })

  it('unfolds and folds again on each click, and leaves the focus on the toggle', async () => {
    const { folded, setFolded } = viewState(true)
    const view = render(() => (
      <SettledTurnFold fold={fold()} folded={folded()} onFoldedChange={setFolded}>
        <p>Trinn</p>
      </SettledTurnFold>
    ))
    await settle()
    const toggle = view.container.querySelector<HTMLButtonElement>('.verevon-chat-settled-fold__toggle')!
    const body = view.container.querySelector('.verevon-chat-settled-fold__body')!
    toggle.focus()

    fireEvent.click(toggle)
    flush()
    expect(toggle.getAttribute('aria-expanded')).toBe('true')
    expect(body.hasAttribute('hidden')).toBe(false)
    expect(document.activeElement).toBe(toggle)

    fireEvent.click(toggle)
    flush()
    expect(toggle.getAttribute('aria-expanded')).toBe('false')
    expect(body.getAttribute('hidden')).toBe('until-found')
  })

  it('is a native button, so Enter and Space act as a click', async () => {
    const view = render(() => (
      <SettledTurnFold fold={fold()} folded={false} onFoldedChange={() => {}}>
        <p>Trinn</p>
      </SettledTurnFold>
    ))
    await settle()
    const toggle = view.container.querySelector<HTMLButtonElement>('.verevon-chat-settled-fold__toggle')!
    expect(toggle.tagName).toBe('BUTTON')
    expect(toggle.type).toBe('button')
  })

  it('asks for the row to open when find in the page reveals the folded body', async () => {
    const { folded, setFolded } = viewState(true)
    const view = render(() => (
      <SettledTurnFold fold={fold()} folded={folded()} onFoldedChange={setFolded}>
        <p>Trinn</p>
      </SettledTurnFold>
    ))
    await settle()
    const toggle = view.container.querySelector('.verevon-chat-settled-fold__toggle')
    const body = view.container.querySelector('.verevon-chat-settled-fold__body')!
    body.dispatchEvent(new Event('beforematch'))
    flush()
    expect(toggle?.getAttribute('aria-expanded')).toBe('true')
    expect(body.hasAttribute('hidden')).toBe(false)
  })

  it('names the time, the steps and the files in the row', async () => {
    const view = render(() => (
      <SettledTurnFold fold={fold()} folded={false} onFoldedChange={() => {}}>
        <p>Trinn</p>
      </SettledTurnFold>
    ))
    await settle()
    const label = view.container.querySelector('.verevon-chat-settled-fold__toggle')?.textContent ?? ''
    expect(label).toContain('4 min 12 s')
    expect(label).toMatch(/2 (steg|steps)/)
    expect(label).toMatch(/(én fil|1 file)\b/)
  })

  it('keeps the folded body in the document, so its controls are still there', async () => {
    const view = render(() => (
      <SettledTurnFold fold={fold()} folded onFoldedChange={() => {}}>
        <p>Verktøykall</p>
      </SettledTurnFold>
    ))
    await settle()
    const body = view.container.querySelector('.verevon-chat-settled-fold__body')
    expect(body?.textContent).toContain('Verktøykall')
  })
})

describe('SettledTurnContent', () => {
  it('puts the process trail in the folded body and keeps the answer in view', async () => {
    const view = render(() => (
      <SettledTurnContent
        turnId="turn-1"
        fold={fold({ defaultFolded: true })}
        entries={[entry('reasoning'), entry('answer'), entry('steps')]}
        renderEntries={renderEntries}
      />
    ))
    await settle()
    const steps = view.container.querySelector('[data-kind="steps"]')
    const answer = view.container.querySelector('[data-kind="answer"]')
    expect(steps?.closest('.verevon-chat-settled-fold__body')).not.toBeNull()
    expect(answer?.closest('[hidden]')).toBeNull()
  })

  it('renders every entry in place, in order, when there is no fold', async () => {
    const view = render(() => (
      <SettledTurnContent
        turnId="turn-1"
        entries={[entry('answer'), entry('steps'), entry('files')]}
        renderEntries={renderEntries}
      />
    ))
    await settle()
    expect(view.container.querySelector('.verevon-chat-settled-fold')).toBeNull()
    const kinds = Array.from(view.container.querySelectorAll('[data-kind]')).map((node) => node.getAttribute('data-kind'))
    expect(kinds).toEqual(['answer', 'steps', 'files'])
  })

  it('keeps the reader\'s choice when the plan is re-derived', async () => {
    const [current, setCurrent] = createSignal(fold({ defaultFolded: true }))
    const foldState = createSettledFoldState()
    const view = render(() => (
      <SettledTurnContent
        turnId="turn-1"
        fold={current()}
        foldState={foldState}
        entries={[entry('reasoning'), entry('answer')]}
        renderEntries={renderEntries}
      />
    ))
    await settle()
    const toggle = view.container.querySelector<HTMLButtonElement>('.verevon-chat-settled-fold__toggle')!
    fireEvent.click(toggle)
    flush()

    setCurrent(fold({ defaultFolded: true, steps: 3 }))
    flush()
    expect(toggle.getAttribute('aria-expanded')).toBe('true')
    expect(toggle.textContent).toMatch(/3 (steg|steps)/)
  })

  it('keeps the reader\'s choice when the row mounts again, as a virtualized turn does', async () => {
    const foldState = createSettledFoldState()
    const [mounted, setMounted] = createSignal(true)
    const view = render(() => (
      <Show when={mounted()}>
        <SettledTurnContent
          turnId="turn-1"
          fold={fold({ defaultFolded: true })}
          foldState={foldState}
          entries={[entry('reasoning'), entry('answer')]}
          renderEntries={renderEntries}
        />
      </Show>
    ))
    await settle()
    fireEvent.click(view.container.querySelector<HTMLButtonElement>('.verevon-chat-settled-fold__toggle')!)
    flush()

    setMounted(false)
    flush()
    expect(view.container.querySelector('.verevon-chat-settled-fold')).toBeNull()

    setMounted(true)
    flush()
    expect(view.container.querySelector('.verevon-chat-settled-fold__toggle')?.getAttribute('aria-expanded')).toBe('true')
    expect(view.container.querySelector('.verevon-chat-settled-fold__body')?.hasAttribute('hidden')).toBe(false)
  })

  it('starts from the plan again once the view has forgotten its choices', async () => {
    const foldState = createSettledFoldState()
    const [mounted, setMounted] = createSignal(true)
    const view = render(() => (
      <Show when={mounted()}>
        <SettledTurnContent
          turnId="turn-1"
          fold={fold({ defaultFolded: true })}
          foldState={foldState}
          entries={[entry('reasoning'), entry('answer')]}
          renderEntries={renderEntries}
        />
      </Show>
    ))
    await settle()
    fireEvent.click(view.container.querySelector<HTMLButtonElement>('.verevon-chat-settled-fold__toggle')!)
    flush()

    setMounted(false)
    flush()
    expect(view.container.querySelector('.verevon-chat-settled-fold')).toBeNull()
    foldState.reset()
    setMounted(true)
    flush()
    expect(view.container.querySelector('.verevon-chat-settled-fold__toggle')?.getAttribute('aria-expanded')).toBe('false')
  })

  it('keeps the choice for one mount only when the view passes no state', async () => {
    const [mounted, setMounted] = createSignal(true)
    const view = render(() => (
      <Show when={mounted()}>
        <SettledTurnContent
          turnId="turn-1"
          fold={fold({ defaultFolded: true })}
          entries={[entry('reasoning'), entry('answer')]}
          renderEntries={renderEntries}
        />
      </Show>
    ))
    await settle()
    fireEvent.click(view.container.querySelector<HTMLButtonElement>('.verevon-chat-settled-fold__toggle')!)
    flush()
    expect(view.container.querySelector('.verevon-chat-settled-fold__toggle')?.getAttribute('aria-expanded')).toBe('true')

    setMounted(false)
    flush()
    expect(view.container.querySelector('.verevon-chat-settled-fold')).toBeNull()
    setMounted(true)
    flush()
    expect(view.container.querySelector('.verevon-chat-settled-fold__toggle')?.getAttribute('aria-expanded')).toBe('false')
  })

  it('renders the same markup as the plain list when there is no fold', async () => {
    const entries = [entry('reasoning'), entry('answer'), entry('steps'), entry('files')]
    const plain = render(() => (
      <div>
        <For each={entries}>{(item) => <i data-kind={item.node.kind}>{item.node.kind}</i>}</For>
      </div>
    ))
    const wrapped = render(() => (
      <div>
        <SettledTurnContent turnId="turn-1" entries={entries} renderEntries={renderEntries} />
      </div>
    ))
    await settle()
    expect(wrapped.container.innerHTML).toBe(plain.container.innerHTML)
  })

  it('renders no row when the entries hold nothing to fold', async () => {
    const view = render(() => (
      <SettledTurnContent turnId="turn-1" fold={fold()} entries={[entry('answer')]} renderEntries={renderEntries} />
    ))
    await settle()
    expect(view.container.querySelector('.verevon-chat-settled-fold')).toBeNull()
    expect(view.container.querySelector('[data-kind="answer"]')).not.toBeNull()
  })
})
