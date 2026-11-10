import { cleanup, fireEvent, render } from '@solidjs/testing-library'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { createSignal, For, flush } from 'solid-js'
import { afterEach, describe, expect, it } from 'vitest'
import { SettledTurnContent, SettledTurnFold } from './SettledTurnFold'
import type { ConversationNode, ConversationNodeContext, ConversationNodeEntry } from '@/shared/chat-nodes'
import type { SettledFold } from '../lib/settled-fold'
import { createSettledFoldChoices } from '../lib/settled-fold-choices'

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

const toggleOf = (root: ParentNode) => root.querySelector<HTMLButtonElement>('.verevon-chat-settled-fold__toggle')
const bodyOf = (root: ParentNode) => root.querySelector<HTMLElement>('.verevon-chat-settled-fold__body')

describe('SettledTurnFold', () => {
  it('starts folded when the plan says so, with the body hidden until found', async () => {
    const view = render(() => (
      <SettledTurnFold fold={fold({ defaultFolded: true })} turnId="t1">
        <p>Trinn</p>
      </SettledTurnFold>
    ))
    await settle()
    expect(toggleOf(view.container)?.getAttribute('aria-expanded')).toBe('false')
    expect(bodyOf(view.container)?.getAttribute('hidden')).toBe('until-found')
  })

  it('starts open when the plan says so', async () => {
    const view = render(() => (
      <SettledTurnFold fold={fold({ defaultFolded: false })} turnId="t1">
        <p>Trinn</p>
      </SettledTurnFold>
    ))
    await settle()
    expect(toggleOf(view.container)?.getAttribute('aria-expanded')).toBe('true')
    expect(bodyOf(view.container)?.hasAttribute('hidden')).toBe(false)
  })

  it('unfolds and folds again on each click, and leaves the focus on the toggle', async () => {
    const view = render(() => (
      <SettledTurnFold fold={fold({ defaultFolded: true })} turnId="t1">
        <p>Trinn</p>
      </SettledTurnFold>
    ))
    await settle()
    const toggle = toggleOf(view.container)!
    const body = bodyOf(view.container)!
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
    const view = render(() => <SettledTurnFold fold={fold()} turnId="t1"><p>Trinn</p></SettledTurnFold>)
    await settle()
    const toggle = toggleOf(view.container)!
    expect(toggle.tagName).toBe('BUTTON')
    expect(toggle.type).toBe('button')
  })

  it('opens the row when find in the page reveals the folded body', async () => {
    const view = render(() => (
      <SettledTurnFold fold={fold({ defaultFolded: true })} turnId="t1">
        <p>Trinn</p>
      </SettledTurnFold>
    ))
    await settle()
    const body = bodyOf(view.container)!
    body.dispatchEvent(new Event('beforematch'))
    flush()
    expect(toggleOf(view.container)?.getAttribute('aria-expanded')).toBe('true')
    expect(body.hasAttribute('hidden')).toBe(false)
  })

  it('names the time, the steps and the files in the row', async () => {
    const view = render(() => <SettledTurnFold fold={fold()} turnId="t1"><p>Trinn</p></SettledTurnFold>)
    await settle()
    const label = toggleOf(view.container)?.textContent ?? ''
    expect(label).toContain('4 min 12 s')
    expect(label).toMatch(/2 (steg|steps)/)
    expect(label).toMatch(/(én fil|1 file)\b/)
  })

  it('keeps the folded body in the document, so its controls are still there', async () => {
    const view = render(() => (
      <SettledTurnFold fold={fold({ defaultFolded: true })} turnId="t1">
        <p>Verktøykall</p>
      </SettledTurnFold>
    ))
    await settle()
    expect(bodyOf(view.container)?.textContent).toContain('Verktøykall')
  })

  it('keeps the reader\'s choice in the view when the row mounts again, as virtualisation does', async () => {
    const choices = createSettledFoldChoices()
    const first = render(() => (
      <SettledTurnFold fold={fold({ defaultFolded: true })} turnId="t1" choices={choices}>
        <p>Trinn</p>
      </SettledTurnFold>
    ))
    await settle()
    fireEvent.click(toggleOf(first.container)!)
    flush()
    first.unmount()

    const second = render(() => (
      <SettledTurnFold fold={fold({ defaultFolded: true })} turnId="t1" choices={choices}>
        <p>Trinn</p>
      </SettledTurnFold>
    ))
    await settle()
    expect(toggleOf(second.container)?.getAttribute('aria-expanded')).toBe('true')
  })

  it('records a find-in-page reveal in the view\'s choices', async () => {
    const choices = createSettledFoldChoices()
    const view = render(() => (
      <SettledTurnFold fold={fold({ defaultFolded: true })} turnId="t1" choices={choices}>
        <p>Trinn</p>
      </SettledTurnFold>
    ))
    await settle()
    bodyOf(view.container)!.dispatchEvent(new Event('beforematch'))
    flush()
    expect(choices.isFolded('t1', true)).toBe(false)
  })

  it('follows a changed plan default until the reader acts on the row', async () => {
    const [current, setCurrent] = createSignal(fold({ defaultFolded: true }))
    const view = render(() => <SettledTurnFold fold={current()} turnId="t1"><p>Trinn</p></SettledTurnFold>)
    await settle()
    expect(toggleOf(view.container)?.getAttribute('aria-expanded')).toBe('false')

    setCurrent(fold({ defaultFolded: false }))
    flush()
    expect(toggleOf(view.container)?.getAttribute('aria-expanded')).toBe('true')

    fireEvent.click(toggleOf(view.container)!)
    flush()
    setCurrent(fold({ defaultFolded: true }))
    flush()
    expect(toggleOf(view.container)?.getAttribute('aria-expanded')).toBe('false')
  })

  it('keeps the choices of two turns in one view apart', async () => {
    const choices = createSettledFoldChoices()
    const view = render(() => (
      <>
        <div data-testid="first">
          <SettledTurnFold fold={fold({ defaultFolded: true })} turnId="t1" choices={choices}>
            <p>Første</p>
          </SettledTurnFold>
        </div>
        <div data-testid="second">
          <SettledTurnFold fold={fold({ defaultFolded: true })} turnId="t2" choices={choices}>
            <p>Andre</p>
          </SettledTurnFold>
        </div>
      </>
    ))
    await settle()
    fireEvent.click(toggleOf(view.getByTestId('first'))!)
    flush()
    expect(toggleOf(view.getByTestId('first'))?.getAttribute('aria-expanded')).toBe('true')
    expect(toggleOf(view.getByTestId('second'))?.getAttribute('aria-expanded')).toBe('false')
  })

  it('never hides its own row: only the body leaves the layout', async () => {
    const view = render(() => (
      <SettledTurnFold fold={fold({ defaultFolded: true })} turnId="t1">
        <p>Trinn</p>
      </SettledTurnFold>
    ))
    await settle()
    const toggle = toggleOf(view.container)!
    expect(toggle.closest('[hidden]')).toBeNull()
    expect(bodyOf(view.container)?.getAttribute('hidden')).toBe('until-found')
  })

  it('adds no planner item of its own: the row sits inside its turn\'s element, as it does in the window', async () => {
    const view = render(() => (
      <div role="article" data-turn-id="t1">
        <SettledTurnFold fold={fold()} turnId="t1">
          <p>Trinn</p>
        </SettledTurnFold>
      </div>
    ))
    await settle()
    expect(view.container.querySelectorAll('[role="article"]')).toHaveLength(1)
    expect(toggleOf(view.container)?.closest('[data-turn-id="t1"]')).not.toBeNull()
  })
})

describe('SettledTurnContent', () => {
  it('puts the process trail in the folded body and keeps the answer in view', async () => {
    const view = render(() => (
      <SettledTurnContent
        fold={fold({ defaultFolded: true })}
        turnId="t1"
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
        turnId="t1"
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
    const view = render(() => (
      <SettledTurnContent
        fold={current()}
        turnId="t1"
        entries={[entry('reasoning'), entry('answer')]}
        renderEntries={renderEntries}
      />
    ))
    await settle()
    const toggle = toggleOf(view.container)!
    fireEvent.click(toggle)
    flush()

    setCurrent(fold({ defaultFolded: true, steps: 3 }))
    flush()
    expect(toggle.getAttribute('aria-expanded')).toBe('true')
    expect(toggle.textContent).toMatch(/3 (steg|steps)/)
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
        <SettledTurnContent turnId="t1" entries={entries} renderEntries={renderEntries} />
      </div>
    ))
    await settle()
    expect(wrapped.container.innerHTML).toBe(plain.container.innerHTML)
  })

  it('renders no row when the entries hold nothing to fold', async () => {
    const view = render(() => (
      <SettledTurnContent fold={fold()} turnId="t1" entries={[entry('answer')]} renderEntries={renderEntries} />
    ))
    await settle()
    expect(view.container.querySelector('.verevon-chat-settled-fold')).toBeNull()
    expect(view.container.querySelector('[data-kind="answer"]')).not.toBeNull()
  })

  it('draws the same fold inside a virtual window\'s turn element as in the plain list', async () => {
    const plain = render(() => (
      <SettledTurnContent
        fold={fold({ defaultFolded: true })}
        turnId="t1"
        entries={[entry('reasoning'), entry('answer')]}
        renderEntries={renderEntries}
      />
    ))
    const windowed = render(() => (
      <div role="article" data-turn-id="t1">
        <SettledTurnContent
          fold={fold({ defaultFolded: true })}
          turnId="t1"
          entries={[entry('reasoning'), entry('answer')]}
          renderEntries={renderEntries}
        />
      </div>
    ))
    await settle()
    // Each render mints its own unique ids for the body, so only the ids are allowed to differ.
    const markup = (root: HTMLElement) =>
      root.querySelector('.verevon-chat-settled-fold')?.outerHTML.replace(/(id|aria-controls)="[^"]*"/g, '$1="_"')
    expect(markup(windowed.container)).toBe(markup(plain.container))
    expect(markup(windowed.container)).toContain('verevon-chat-settled-fold__toggle')
  })
})

const chatFoldCss = readFileSync(resolve(process.cwd(), 'src/features/chat/components/chat-fold.css'), 'utf8')

/** The declarations of one rule in a stylesheet, from its selector to the closing brace. */
function ruleBody(css: string, selector: string): string {
  const start = css.indexOf(`${selector} {`)
  if (start < 0) return ''
  return css.slice(start, css.indexOf('}', start))
}

describe('chat-fold.css: the row keeps one line of fixed height', () => {
  it('gives the row a fixed height on one line, whatever its label says', () => {
    const toggle = ruleBody(chatFoldCss, '.verevon-chat-settled-fold__toggle')
    expect(toggle).toMatch(/\bheight: 32px;/)
    expect(toggle).toMatch(/white-space: nowrap;/)
  })

  it('truncates the label rather than wrapping it', () => {
    const label = ruleBody(chatFoldCss, '.verevon-chat-settled-fold__label')
    expect(label).toMatch(/overflow: hidden;/)
    expect(label).toMatch(/text-overflow: ellipsis;/)
    expect(label).toMatch(/white-space: nowrap;/)
  })

  it('never animates the row\'s height, so the planner\'s measurement stays true', () => {
    const toggle = ruleBody(chatFoldCss, '.verevon-chat-settled-fold__toggle')
    expect(toggle).not.toMatch(/transition:[^;]*\bheight\b/)
  })
})
