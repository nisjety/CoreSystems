// @vitest-environment jsdom

import { fireEvent, render } from '@solidjs/testing-library'
import { describe, expect, it, vi } from 'vitest'
import { ContextWindowPanel } from './ChatPanels'
import {
  DiffView,
  MemoryRecallNotice,
  PlanApprovalControl,
  QueuedInputStrip,
  ToolCallCard,
} from './ChatMessages'
import type { RecalledMemory } from '@/shared/api/chat-client'
import type { QueuedInput } from './chat-types'
import { diffText, parseUnifiedDiff } from '@/shared/chat-nodes'
import type { ThreadContext } from '@/shared/api/chat-client'

const context = (overrides: Partial<ThreadContext> = {}): ThreadContext => ({
  threadId: 't1',
  estimatedTokens: 1200,
  budgetTokens: 8000,
  segments: [
    { kind: 'system', content: 'You are Verevon', estimatedTokens: 40 },
    { kind: 'grounding', content: 'invoice excerpt', estimatedTokens: 900 },
  ],
  ...overrides,
})

/** Expand the collapsed inspector. */
function open(container: HTMLElement) {
  const toggle = container.querySelector('button')
  expect(toggle).toBeTruthy()
  fireEvent.click(toggle!)
}

describe('ContextWindowPanel', () => {
  it('summarises fill without expanding', () => {
    const { container, unmount } = render(() => (
      <ContextWindowPanel context={context()} loading={false} failed={false} />
    ))
    const text = container.textContent ?? ''
    expect(text).toContain('Kontekstvindu')
    // 1200 / 8000 = 15%. Norwegian locale uses a non-breaking space separator,
    // so assert on the percentage rather than the formatted thousands.
    expect(text).toContain('15%')
    // Collapsed: segment content must not be on screen yet.
    expect(text).not.toContain('invoice excerpt')
    unmount()
  })

  it('shows each segment with its own token estimate when expanded', () => {
    const { container, unmount } = render(() => (
      <ContextWindowPanel context={context()} loading={false} failed={false} />
    ))
    open(container)
    const text = container.textContent ?? ''
    expect(text).toContain('system')
    expect(text).toContain('grounding')
    // Content, not just sizes — "900 tokens of grounding" does not answer the
    // question the inspector exists for.
    expect(text).toContain('invoice excerpt')
    expect(text).toContain('900')
    unmount()
  })

  /**
   * A failed read and an empty window look identical if absence is rendered as
   * blank, and only one of them is a problem.
   */
  it('distinguishes a failed read from an empty window', () => {
    const failed = render(() => (
      <ContextWindowPanel context={undefined} loading={false} failed={true} />
    ))
    open(failed.container)
    expect(failed.container.textContent).toContain('Kunne ikke hente')
    failed.unmount()

    const empty = render(() => (
      <ContextWindowPanel
        context={context({ segments: [] })}
        loading={false}
        failed={false}
      />
    ))
    open(empty.container)
    expect(empty.container.textContent).toContain('Ingen segmenter rapportert')
    empty.unmount()
  })

  it('shows loading rather than claiming the window is empty', () => {
    const { container, unmount } = render(() => (
      <ContextWindowPanel context={undefined} loading={true} failed={false} />
    ))
    open(container)
    expect(container.textContent).toContain('Laster')
    expect(container.textContent).not.toContain('Ingen segmenter')
    unmount()
  })

  /** A zero budget must not render `NaN%`, which reads as broken, not unknown. */
  it('omits the percentage when the budget is unknown', () => {
    const { container, unmount } = render(() => (
      <ContextWindowPanel
        context={context({ budgetTokens: 0 })}
        loading={false}
        failed={false}
      />
    ))
    expect(container.textContent).not.toContain('NaN')
    expect(container.textContent).not.toContain('%')
    unmount()
  })
})

describe('DiffView', () => {
  it('shows the stat and renders each changed line with its sigil', () => {
    const { container, unmount } = render(() => (
      <DiffView result={diffText("a\nb\nc", "a\nB\nc")} />
    ))
    const text = container.textContent ?? ''
    expect(text).toContain('+1')
    expect(text).toContain('1')
    expect(
      container.querySelector('.verevon-chat-diff__line--added'),
    ).toBeTruthy()
    expect(
      container.querySelector('.verevon-chat-diff__line--removed'),
    ).toBeTruthy()
    // The sigil carries the meaning for anyone who cannot tell the tints apart.
    const sigils = [
      ...container.querySelectorAll('.verevon-chat-diff__sigil'),
    ].map((node) => node.textContent)
    expect(sigils).toContain('+')
    expect(sigils).toContain('-')
    unmount()
  })

  it('says a diff was too large rather than showing nothing', () => {
    const { container, unmount } = render(() => (
      <DiffView
        result={{ hunks: [], stat: { added: 5, removed: 0 }, truncated: true }}
      />
    ))
    // An empty hunk list would otherwise read as "no changes".
    expect(container.textContent).toContain('for stor')
    unmount()
  })

  it('renders nothing but the stat for an unchanged pair', () => {
    const { container, unmount } = render(() => (
      <DiffView result={diffText('same', 'same')} />
    ))
    expect(container.querySelector('.verevon-chat-diff__hunk')).toBeNull()
    expect(container.textContent).toContain('+0')
    unmount()
  })
})

describe('ToolCallCard diff intent', () => {
  const patch = [
    '--- a/f',
    '+++ b/f',
    '@@ -1,2 +1,2 @@',
    ' keep',
    '-old',
    '+new',
  ].join("\n")

  it('shows the add/remove counts on the collapsed row', () => {
    const { container, unmount } = render(() => (
      <ToolCallCard
        call={{
          id: 't1',
          name: 'mcp__git__diff',
          status: 'completed',
          output: patch,
        }}
      />
    ))
    // Readable without expanding — the point of recomputed presentation.
    expect(container.textContent).toContain('+1')
    unmount()
  })

  it('renders the patch as a diff, not as raw text, when expanded', () => {
    const { container, unmount } = render(() => (
      <ToolCallCard
        call={{
          id: 't1',
          name: 'mcp__git__diff',
          status: 'completed',
          output: patch,
        }}
      />
    ))
    fireEvent.click(container.querySelector('button')!)
    expect(container.querySelector('.verevon-chat-diff')).toBeTruthy()
    expect(parseUnifiedDiff(patch).stat).toEqual({ added: 1, removed: 1 })
    unmount()
  })

  it('leaves non-patch output as raw text', () => {
    const { container, unmount } = render(() => (
      <ToolCallCard
        call={{
          id: 't1',
          name: 'shell',
          status: 'completed',
          output: 'total 4',
        }}
      />
    ))
    fireEvent.click(container.querySelector('button')!)
    expect(container.querySelector('.verevon-chat-diff')).toBeNull()
    expect(container.querySelector('pre')?.textContent).toContain('total 4')
    unmount()
  })
})

describe('MemoryRecallNotice disclosure', () => {
  const memory = (overrides: Partial<RecalledMemory> = {}): RecalledMemory => ({
    memoryId: 'mem-1',
    role: 'recall',
    origin: 'stated',
    label: 'USER',
    preview: 'Foretrekker metriske enheter',
    ...overrides,
  })

  it('stays a one-line count until opened', () => {
    const { container, unmount } = render(() => (
      <MemoryRecallNotice count={1} memories={[memory()]} />
    ))
    expect(container.textContent).toContain('Brukte 1 minne')
    // The contents are the follow-up, not the headline.
    expect(container.textContent).not.toContain('Foretrekker metriske enheter')
    unmount()
  })

  it('shows what was remembered when opened', () => {
    const { container, unmount } = render(() => (
      <MemoryRecallNotice count={1} memories={[memory()]} />
    ))
    fireEvent.click(container.querySelector('button')!)
    expect(container.textContent).toContain('Foretrekker metriske enheter')
    expect(container.textContent).toContain('USER')
    unmount()
  })

  /**
   * The honesty rule as a reader sees it: an unrecorded origin must not be
   * phrased as something the user asked for.
   */
  it('never phrases an unrecorded origin as something the user said', () => {
    const { container, unmount } = render(() => (
      <MemoryRecallNotice
        count={1}
        memories={[memory({ origin: 'unrecorded' })]}
      />
    ))
    fireEvent.click(container.querySelector('button')!)
    const text = container.textContent ?? ''
    expect(text).toContain('ukjent kilde')
    expect(text).not.toContain('du ba meg huske')
    unmount()
  })

  it('distinguishes stated from inferred', () => {
    const { container, unmount } = render(() => (
      <MemoryRecallNotice
        count={2}
        memories={[memory(), memory({ memoryId: 'mem-2', origin: 'inferred' })]}
      />
    ))
    fireEvent.click(container.querySelector('button')!)
    const text = container.textContent ?? ''
    expect(text).toContain('du ba meg huske')
    expect(text).toContain('utledet')
    unmount()
  })

  it('marks org-level context as not coming from a conversation', () => {
    const { container, unmount } = render(() => (
      <MemoryRecallNotice
        count={1}
        memories={[memory({ role: 'inject', label: 'POLICY' })]}
      />
    ))
    fireEvent.click(container.querySelector('button')!)
    expect(container.textContent).toContain('organisasjon')
    unmount()
  })

  /** An older backend sends only a count; the notice must still render. */
  it('degrades to the plain notice with no list', () => {
    const { container, unmount } = render(() => (
      <MemoryRecallNotice count={3} memories={[]} />
    ))
    expect(container.textContent).toContain('Brukte 3 minner')
    expect(container.querySelector('button')).toBeNull()
    unmount()
  })
})

describe('QueuedInputStrip', () => {
  const entry = (overrides: Partial<QueuedInput> = {}): QueuedInput => ({
    id: 'q1',
    content: 'bruk EUR i stedet',
    state: 'pending',
    ...overrides,
  })

  /**
   * The bug this replaces was silence: a message typed mid-run was discarded
   * with no trace. Nothing at all must render only when there is nothing.
   */
  it('renders nothing when no message is in flight', () => {
    const { container, unmount } = render(() => (
      <QueuedInputStrip entries={[]} />
    ))
    expect(container.querySelector('.verevon-chat-queued')).toBeNull()
    unmount()
  })

  it('shows the message and that it is still waiting', () => {
    const { container, unmount } = render(() => (
      <QueuedInputStrip entries={[entry()]} />
    ))
    expect(container.textContent).toContain('bruk EUR i stedet')
    expect(container.textContent).toContain('venter')
    unmount()
  })

  /**
   * "The agent has it" and "the agent never will" are the two facts a reader
   * needs, and they must be readable as WORDS. A tint alone would make the
   * difference depend on telling two shades apart.
   */
  it('states delivery and refusal in words, not only in colour', () => {
    const { container, unmount } = render(() => (
      <QueuedInputStrip
        entries={[
          entry({ state: 'delivered' }),
          entry({ id: 'q2', state: 'refused', note: 'for mange i kø' }),
        ]}
      />
    ))
    const text = container.textContent ?? ''
    expect(text).toContain('levert til agenten')
    expect(text).toContain('ikke levert')
    expect(text).toContain('for mange i kø')
    unmount()
  })

  it('carries the state on the element so the styling cannot disagree with the label', () => {
    const { container, unmount } = render(() => (
      <QueuedInputStrip entries={[entry({ state: 'delivered' })]} />
    ))
    expect(
      container
        .querySelector('.verevon-chat-queued-item')
        ?.getAttribute('data-state'),
    ).toBe('delivered')
    unmount()
  })
})

describe('PlanApprovalControl', () => {
  /**
   * The whole reason the control exists: a plan approval must name what is
   * granted AND why. An Approve button that works with an empty reason is a
   * rubber stamp, and the grant it makes is unreviewable afterwards.
   */
  it('will not approve until the reason is a reason', () => {
    const onApprove = vi.fn()
    const { container, unmount } = render(() => (
      <PlanApprovalControl pending={false} onApprove={onApprove} />
    ))
    const approve = container.querySelector<HTMLButtonElement>('.verevon-chat-plan__approve')!
    expect(approve.disabled).toBe(true)
    fireEvent.click(approve)
    expect(onApprove).not.toHaveBeenCalled()

    // And the minimum is STATED, not enforced silently — a disabled button with
    // no reason given is indistinguishable from a broken one.
    expect(container.textContent).toContain('Minst 12 tegn')
    unmount()
  })

  it('approves with the chosen rung and the typed reason', () => {
    const onApprove = vi.fn()
    const { container, unmount } = render(() => (
      <PlanApprovalControl pending={false} onApprove={onApprove} />
    ))
    const reason = container.querySelector('textarea')!
    fireEvent.input(reason, { target: { value: 'planen skriver rapporten til arbeidsområdet' } })
    const rungs = container.querySelectorAll<HTMLButtonElement>('.verevon-chat-plan__rung')
    // The widest rung, explicitly chosen.
    fireEvent.click(rungs[rungs.length - 1]!)
    fireEvent.click(container.querySelector<HTMLButtonElement>('.verevon-chat-plan__approve')!)

    expect(onApprove).toHaveBeenCalledWith(
      'danger_full_access',
      'planen skriver rapporten til arbeidsområdet',
    )
    unmount()
  })

  /**
   * `read_only` is what a plan-mode run already has. Offering it as a grant
   * would be an approval that changes nothing while still taking the run out of
   * plan mode — the worst of both.
   */
  it('never offers a rung that grants nothing', () => {
    const { container, unmount } = render(() => (
      <PlanApprovalControl pending={false} onApprove={() => {}} />
    ))
    const labels = [...container.querySelectorAll('.verevon-chat-plan__rung')].map(
      (node) => node.textContent,
    )
    expect(labels).not.toContain('bare undersøke')
    expect(labels).toHaveLength(2)
    unmount()
  })

  it('reports a granted rung instead of asking again', () => {
    const { container, unmount } = render(() => (
      <PlanApprovalControl grantedRung="workspace_write" pending={false} onApprove={() => {}} />
    ))
    expect(container.querySelector('.verevon-chat-plan')).toBeNull()
    expect(container.textContent).toContain('skrive i arbeidsområdet')
    unmount()
  })

  it('surfaces a failed approval rather than looking idle', () => {
    const { container, unmount } = render(() => (
      <PlanApprovalControl pending={false} error="Fullmakten ble avvist" onApprove={() => {}} />
    ))
    expect(container.textContent).toContain('Fullmakten ble avvist')
    unmount()
  })
})
