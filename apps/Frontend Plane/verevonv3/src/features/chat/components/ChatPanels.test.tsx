// @vitest-environment jsdom

import { cleanup, fireEvent, render, waitFor } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { flush, createSignal } from 'solid-js'
import { ChatTabs, ContextWindowPanel, RunPlanPanel, SourcesPanel, StepsPanel } from './ChatPanels'
import * as orchestration from '@/shared/api/orchestration-client'
import * as memoryClient from '@/shared/api/memory-client'
import {
  __resetPlanDraftsForTests,
  AnswerRegion,
  DiffView,
  MemoryRecallNotice,
  PlanApprovalControl,
  QueuedInputStrip,
  ThinkingDots,
  ToolCallCard,
} from './ChatMessages'
import {
  collectEvidenceSources,
  formatElapsedWait,
  partitionEvidenceSources,
  spokenSourceTally,
} from './chat-media-markdown'
import type { RecalledMemory } from '@/shared/api/chat-client'
import type { ChatGroundingSource, ChatTurn, Citation, QueuedInput } from './chat-types'
import { deriveConversationNodes, diffText, parseUnifiedDiff } from '@/shared/chat-nodes'
import type { ThreadContext } from '@/shared/api/chat-client'

// RunPlanPanel's resource fires as soon as run and thread ids exist. Stubbed so
// the panel tests stay deterministic and offline; each test sets listPlans.
vi.mock('@/shared/api/orchestration-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/orchestration-client')>()
  return {
    ...actual,
    listPlans: vi.fn(async () => []),
    listTodos: vi.fn(async () => []),
    // Runs without sub-agents answer 404 here; the panel absorbs it via allSettled.
    getLineage: vi.fn(async () => {
      throw new Error('subagent lineage not found')
    }),
  }
})

// MemoryRecallNotice's per-entry Rediger/Glem controls call the real
// mutations directly (no store round trip) — mocked here so the disclosure
// tests stay deterministic and offline, same as the orchestration mock above.
vi.mock('@/shared/api/memory-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/memory-client')>()
  return {
    ...actual,
    deleteMemory: vi.fn(async () => ({ deleted: true, degraded: false, degradationReason: '' })),
    correctMemory: vi.fn(async () => ({
      memoryId: 'mem-1-corrected',
      deleted: true,
      original: 'removed',
      degraded: false,
      degradationReason: '',
    })),
    listMemories: vi.fn(async () => ({ memories: [], degraded: false, degradationReason: '' })),
  }
})

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
  flush()
}

describe('StepsPanel terminal status', () => {
  it('keeps a cancelled run stopped even when its earlier tool completed', () => {
    const { container, unmount } = render(() => <StepsPanel turnStatus="stopped" steps={[
      { id: 'count', title: 'Count words', detail: '17 words', status: 'done', createdAt: '2026-09-20T07:00:00Z' },
    ]} onStopTask={() => {}} />)
    expect(container.querySelector('.verevon-chat-steps-header p')?.textContent).toBe('Stoppet')
    expect(container.querySelector<HTMLButtonElement>('.verevon-chat-steps-header button')?.disabled).toBe(true)
    unmount()
  })
})

describe('ContextWindowPanel', () => {
  it('summarises fill without expanding', () => {
    const { container, unmount } = render(() => (
      <ContextWindowPanel context={context()} loading={false} failed={false} />
    ))
    const text = container.textContent ?? ''
    // "Samlet kontekst", not "Kontekstvindu": the budget is session-core's
    // assembly budget, and a window label made 4 096 read as a 4k model.
    expect(text).toContain('Samlet kontekst')
    expect(text).not.toContain('Kontekstvindu')
    const toggle = container.querySelector<HTMLButtonElement>('.verevon-chat-context-window__toggle')
    expect(toggle?.title).toContain('Ikke modellens kontekstvindu')
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
   * Persona spec §2.5: `kind` is a free-form wire string with no allow-list,
   * so a `"persona"`-kind segment (the BFF's synthetic prepend) needs zero
   * special-casing to display — it renders like any other segment (content +
   * token count), with only a cosmetic localized label swapped in.
   */
  it('renders a persona-kind segment like any other segment, with a localized label', () => {
    const { container, unmount } = render(() => (
      <ContextWindowPanel
        context={context({
          segments: [
            { kind: 'persona', content: 'Always answer in bullet points.', estimatedTokens: 12 },
          ],
        })}
        loading={false}
        failed={false}
      />
    ))
    open(container)
    const text = container.textContent ?? ''
    expect(text).toContain('Personlige instrukser')
    expect(text).toContain('Always answer in bullet points.')
    expect(text).toContain('12')
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

  /** chat-wave2 scope spec §2.2: the concrete "shown in the receipt" surface. */
  describe('per-turn retrieval-source filter receipt', () => {
    it('names the excluded source class and the specific tools left out', () => {
      const { container, unmount } = render(() => (
        <ContextWindowPanel
          context={context()}
          loading={false}
          failed={false}
          scopeApplied={{ scope: 'knowledge_only', excludedTools: ['web_search', 'fetch_url'] }}
        />
      ))
      open(container)
      const text = container.textContent ?? ''
      expect(text).toContain('Nett-kilder ekskludert')
      expect(text).toContain('web_search')
      expect(text).toContain('fetch_url')
      unmount()
    })

    it('names the org-side exclusion for web_only', () => {
      const { container, unmount } = render(() => (
        <ContextWindowPanel
          context={context()}
          loading={false}
          failed={false}
          scopeApplied={{ scope: 'web_only', excludedTools: ['knowledge_search'] }}
        />
      ))
      open(container)
      expect(container.textContent).toContain('Organisasjonskilder ekskludert')
      unmount()
    })

    it('shows nothing for an ordinary (auto) turn', () => {
      const { container, unmount } = render(() => (
        <ContextWindowPanel context={context()} loading={false} failed={false} />
      ))
      open(container)
      expect(container.textContent).not.toContain('ekskludert')
      unmount()
    })
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
    flush()
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
    flush()
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
    // F-05: recall runs on every turn regardless of relevance, so the count
    // is what was fetched, not proof the model "used" it — "Hentet", not
    // "Brukte" (see MemoryRecallNotice's summary()).
    expect(container.textContent).toContain('Hentet 1 minne')
    // The contents are the follow-up, not the headline.
    expect(container.textContent).not.toContain('Foretrekker metriske enheter')
    unmount()
  })

  it('shows what was remembered when opened', () => {
    const { container, unmount } = render(() => (
      <MemoryRecallNotice count={1} memories={[memory()]} />
    ))
    fireEvent.click(container.querySelector('button')!)
    flush()
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
    flush()
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
    flush()
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
    flush()
    expect(container.textContent).toContain('organisasjon')
    unmount()
  })

  /** An older backend sends only a count; the notice must still render. */
  it('degrades to the plain notice with no list', () => {
    const { container, unmount } = render(() => (
      <MemoryRecallNotice count={3} memories={[]} />
    ))
    // F-05: "Hentet", not "Brukte" — see the other MemoryRecallNotice test above.
    expect(container.textContent).toContain('Hentet 3 minner')
    expect(container.querySelector('button')).toBeNull()
    unmount()
  })
})

/**
 * Per-entry "Rediger"/"Glem" — the matching Claude/ChatGPT give their own
 * recalled-memory lists: correct or forget a wrong entry from where you see
 * it, not a trip to a separate settings page.
 */
describe('MemoryRecallNotice correct/forget', () => {
  const memory = (overrides: Partial<RecalledMemory> = {}): RecalledMemory => ({
    memoryId: 'mem-1',
    role: 'recall',
    origin: 'stated',
    label: 'USER',
    preview: 'Foretrekker metriske enheter',
    ...overrides,
  })

  afterEach(() => {
    vi.clearAllMocks()
  })

  const openList = (container: HTMLElement) => {
    fireEvent.click(container.querySelector('.verevon-chat-memory-recall__toggle')!)
    flush()
  }

  const actionButtons = (container: HTMLElement) =>
    Array.from(container.querySelectorAll('.verevon-chat-memory-recall__action'))

  it('only offers correct/forget on a personal ("USER"-topic) entry', () => {
    const { container, unmount } = render(() => (
      <MemoryRecallNotice
        count={1}
        threadId="thread-1"
        memories={[memory({ role: 'inject', label: 'POLICY' })]}
      />
    ))
    openList(container)
    // DeleteMemory/IndexMemory both filter to `scope = 'user'` server-side —
    // an org/workspace/policy entry could never actually be acted on, so the
    // button must not be offered at all rather than silently no-op later.
    expect(actionButtons(container)).toHaveLength(0)
    unmount()
  })

  it('forgets a memory after a second confirming click, and updates the list in place', async () => {
    const { container, unmount } = render(() => (
      <MemoryRecallNotice count={1} threadId="thread-1" memories={[memory()]} />
    ))
    openList(container)
    const forgetButton = actionButtons(container).find((el) => el.textContent?.includes('Glem'))!

    // First click only arms it — nothing is deleted yet.
    fireEvent.click(forgetButton)
    flush()
    expect(memoryClient.deleteMemory).not.toHaveBeenCalled()
    expect(container.textContent).toContain('Bekreft')

    // Second click actually deletes.
    fireEvent.click(forgetButton)
    await waitFor(() => expect(memoryClient.deleteMemory).toHaveBeenCalledWith('mem-1'))
    await waitFor(() =>
      expect(container.textContent).not.toContain('Foretrekker metriske enheter'),
    )
    unmount()
  })

  it('surfaces an error and keeps the entry when forgetting fails', async () => {
    vi.mocked(memoryClient.deleteMemory).mockRejectedValueOnce(new Error('nope'))
    const { container, unmount } = render(() => (
      <MemoryRecallNotice count={1} threadId="thread-1" memories={[memory()]} />
    ))
    openList(container)
    const forgetButton = actionButtons(container).find((el) => el.textContent?.includes('Glem'))!
    fireEvent.click(forgetButton)
    flush()
    fireEvent.click(forgetButton)
    await waitFor(() => expect(memoryClient.deleteMemory).toHaveBeenCalled())
    await waitFor(() =>
      expect(container.querySelector('.verevon-chat-memory-recall__error')).not.toBeNull(),
    )
    // The entry is still there — a failed delete must not vanish client-side.
    expect(container.textContent).toContain('Foretrekker metriske enheter')
    unmount()
  })

  it('corrects a memory: fetches the full content, saves under the new id, and updates the list', async () => {
    vi.mocked(memoryClient.listMemories).mockResolvedValueOnce({
      memories: [
        {
          memoryId: 'mem-1',
          topic: 'USER',
          content: 'Foretrekker metriske enheter, ikke imperial',
          updatedAt: '2026-01-01T00:00:00.000Z',
          provenance: 'stated',
        },
      ],
      degraded: false,
      degradationReason: '',
    })
    const { container, unmount } = render(() => (
      <MemoryRecallNotice count={1} threadId="thread-1" memories={[memory()]} />
    ))
    openList(container)
    const editButton = actionButtons(container).find((el) => el.textContent?.includes('Rediger'))!
    fireEvent.click(editButton)
    flush()

    // The full stored content replaces the (possibly truncated) preview once
    // it loads, so a correction can never accidentally re-truncate a longer
    // memory down to its 160-char preview.
    const textarea = await waitFor(() => {
      const el = container.querySelector<HTMLTextAreaElement>(
        '.verevon-chat-memory-recall__edit-textarea',
      )
      expect(el?.value).toBe('Foretrekker metriske enheter, ikke imperial')
      return el!
    })

    fireEvent.input(textarea, { target: { value: 'Foretrekker metriske enheter, alltid' } })
    flush()
    const saveButton = actionButtons(container).find((el) => el.textContent?.includes('Lagre'))!
    fireEvent.click(saveButton)

    await waitFor(() =>
      expect(memoryClient.correctMemory).toHaveBeenCalledWith(
        'mem-1',
        'thread-1',
        'Foretrekker metriske enheter, alltid',
      ),
    )
    await waitFor(() =>
      expect(container.textContent).toContain('Foretrekker metriske enheter, alltid'),
    )
    // The corrected entry's id changed (no update-in-place RPC exists), and a
    // second "Glem" now targets the NEW id, not the one that was replaced.
    const forgetButton = await waitFor(() => {
      const el = actionButtons(container).find((button) => button.textContent?.includes('Glem'))
      expect(el).toBeDefined()
      return el!
    })
    fireEvent.click(forgetButton)
    flush()
    fireEvent.click(forgetButton)
    await waitFor(() =>
      expect(memoryClient.deleteMemory).toHaveBeenCalledWith('mem-1-corrected'),
    )
    unmount()
  })

  it('disables saving a correction when there is no active thread to correct from', () => {
    const { container, unmount } = render(() => (
      <MemoryRecallNotice count={1} threadId={null} memories={[memory()]} />
    ))
    openList(container)
    const editButton = actionButtons(container).find((el) => el.textContent?.includes('Rediger'))!
    expect(editButton.hasAttribute('disabled')).toBe(true)
    unmount()
  })

  /**
   * A restored turn stores which memories it recalled, not their text, so a
   * memory forgotten or corrected since cannot come back from the cache. The
   * chip reads the reader's current memories once, when first opened.
   */
  it('reads the current text of a restored memory when opened, and shows only the label of one that is gone', async () => {
    vi.mocked(memoryClient.listMemories).mockResolvedValueOnce({
      memories: [
        {
          memoryId: 'mem-1',
          topic: 'USER',
          content: 'Foretrekker metriske enheter, alltid',
          updatedAt: '2026-01-01T00:00:00.000Z',
          provenance: 'stated',
        },
      ],
      degraded: false,
      degradationReason: '',
    })
    const { container, unmount } = render(() => (
      <MemoryRecallNotice
        count={2}
        threadId="thread-1"
        memories={[memory({ preview: '' }), memory({ memoryId: 'mem-forgotten', preview: '' })]}
      />
    ))
    expect(memoryClient.listMemories).not.toHaveBeenCalled()
    openList(container)
    await waitFor(() => expect(container.textContent).toContain('Foretrekker metriske enheter, alltid'))
    expect(memoryClient.listMemories).toHaveBeenCalledTimes(1)
    // The forgotten one keeps its row (this turn did recall it), with no text.
    expect(container.querySelectorAll('.verevon-chat-memory-recall__item')).toHaveLength(2)
    expect(container.querySelectorAll('.verevon-chat-memory-recall__preview')).toHaveLength(1)
    // Closing and reopening does not read again.
    openList(container)
    openList(container)
    expect(memoryClient.listMemories).toHaveBeenCalledTimes(1)
    unmount()
  })

  it('does not read memories when every row already carries its text', () => {
    const { container, unmount } = render(() => (
      <MemoryRecallNotice count={1} threadId="thread-1" memories={[memory()]} />
    ))
    openList(container)
    expect(memoryClient.listMemories).not.toHaveBeenCalled()
    unmount()
  })

  it('does not offer to save a truncated preview when the full memory cannot be loaded', async () => {
    vi.mocked(memoryClient.listMemories).mockRejectedValueOnce(new Error('unavailable'))
    const { container, unmount } = render(() => <MemoryRecallNotice count={1} threadId="thread-1" memories={[memory()]} />)
    openList(container)
    fireEvent.click(actionButtons(container).find(el => el.textContent?.includes('Rediger'))!)
    await waitFor(() => expect(container.textContent).toContain('Kunne ikke hente hele minnet'))
    expect(container.querySelector('textarea')).toBeNull()
    expect(memoryClient.correctMemory).not.toHaveBeenCalled()
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
  // A control remembers its unsent edits per plan id across a remount (see
  // `PlanDraft`), and every test below shows `plan_1`: without this the edits
  // one test leaves behind would be restored into the next.
  afterEach(() => {
    // Unmount first: tearing a control down is what stores its draft.
    cleanup()
    __resetPlanDraftsForTests()
  })

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
    flush()
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
    flush()
    const rungs = container.querySelectorAll<HTMLButtonElement>('.verevon-chat-plan__rung')
    // The widest rung, explicitly chosen.
    fireEvent.click(rungs[rungs.length - 1]!)
    flush()
    fireEvent.click(container.querySelector<HTMLButtonElement>('.verevon-chat-plan__approve')!)
    flush()

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

  // -----------------------------------------------------------------------
  // plan-gate §2.5: the editable step list, only rendered when the turn's
  // model actually proposed a structured plan (`planId` set).
  // -----------------------------------------------------------------------

  const planProps = {
    planId: 'plan_1',
    summary: 'Send the overdue invoice',
    steps: [
      { id: 'step_1', title: 'Draft the email' },
      { id: 'step_2', title: 'Send it', operation: 'email_send' },
    ],
  }

  const fillReason = (container: HTMLElement) => {
    const reason = container.querySelector('textarea')!
    fireEvent.input(reason, { target: { value: 'planen skriver rapporten til arbeidsområdet' } })
    flush()
  }

  it('adds a blank step and edits its text', () => {
    const onApprove = vi.fn()
    const { container, unmount } = render(() => (
      <PlanApprovalControl {...planProps} pending={false} onApprove={onApprove} />
    ))
    fireEvent.click(container.querySelector<HTMLButtonElement>('.verevon-chat-plan__add-step')!)
    flush()
    const inputs = container.querySelectorAll<HTMLInputElement>('.verevon-chat-plan__step-input')
    expect(inputs).toHaveLength(3)
    fireEvent.input(inputs[2]!, { target: { value: 'Follow up next week' } })
    flush()

    fillReason(container)
    fireEvent.click(container.querySelector<HTMLButtonElement>('.verevon-chat-plan__approve')!)
    flush()

    const editedSteps = onApprove.mock.calls[0]![2]
    expect(editedSteps).toHaveLength(3)
    expect(editedSteps[2]).toEqual({
      stepId: undefined,
      title: 'Follow up next week',
      operation: undefined,
      removed: false,
    })
    unmount()
  })

  it('removes a step and still approves with the remaining ones', () => {
    const onApprove = vi.fn()
    const { container, unmount } = render(() => (
      <PlanApprovalControl {...planProps} pending={false} onApprove={onApprove} />
    ))
    const removeButtons = container.querySelectorAll<HTMLButtonElement>(
      '.verevon-chat-plan__step-remove',
    )
    fireEvent.click(removeButtons[0]!)
    flush()
    // The removed row disappears from the VISIBLE list …
    expect(container.querySelectorAll('.verevon-chat-plan__step-input')).toHaveLength(1)

    fillReason(container)
    fireEvent.click(container.querySelector<HTMLButtonElement>('.verevon-chat-plan__approve')!)
    flush()

    // … but is still SENT, marked removed=true, so ordinals stay auditable
    // server-side (plan-gate §2.2) — it is never silently dropped.
    const editedSteps = onApprove.mock.calls[0]![2]
    expect(editedSteps).toEqual([
      { stepId: 'step_1', title: 'Draft the email', operation: undefined, removed: true },
      { stepId: 'step_2', title: 'Send it', operation: 'email_send', removed: false },
    ])
    unmount()
  })

  it('moves a step up and down and preserves the new order on approve', () => {
    const onApprove = vi.fn()
    const { container, unmount } = render(() => (
      <PlanApprovalControl {...planProps} pending={false} onApprove={onApprove} />
    ))
    // Move the SECOND step up, ahead of the first.
    const rows = container.querySelectorAll('.verevon-chat-plan__step')
    const moveUp = rows[1]!.querySelector<HTMLButtonElement>('button[aria-label="Flytt opp"]')!
    fireEvent.click(moveUp)
    flush()

    const titlesAfterMove = [
      ...container.querySelectorAll<HTMLInputElement>('.verevon-chat-plan__step-input'),
    ].map((el) => el.value)
    expect(titlesAfterMove).toEqual(['Send it', 'Draft the email'])

    fillReason(container)
    fireEvent.click(container.querySelector<HTMLButtonElement>('.verevon-chat-plan__approve')!)
    flush()

    const editedSteps = onApprove.mock.calls[0]![2]
    expect(editedSteps.map((s: { stepId?: string }) => s.stepId)).toEqual(['step_2', 'step_1'])
    unmount()
  })

  it('moves past a removed row instead of swapping into it invisibly', () => {
    // A removed row stays in the underlying array (never deleted, see the
    // `<For>` doc comment above) but renders nothing. A naive "swap with the
    // raw adjacent index" reorder would swap the last VISIBLE row with that
    // hidden removed row — changing the array but nothing the user can see,
    // so the button looks unresponsive the first time this happens.
    const threeSteps = {
      planId: 'plan_1',
      summary: 'Send the overdue invoice',
      steps: [
        { id: 'step_1', title: 'Draft the email' },
        { id: 'step_2', title: 'Middle step' },
        { id: 'step_3', title: 'Send it', operation: 'email_send' },
      ],
    }
    const onApprove = vi.fn()
    const { container, unmount } = render(() => (
      <PlanApprovalControl {...threeSteps} pending={false} onApprove={onApprove} />
    ))
    // Remove the middle row, leaving "Draft the email" and "Send it" visible
    // with a hidden removed row sitting between them in the array.
    const removeButtons = container.querySelectorAll<HTMLButtonElement>(
      '.verevon-chat-plan__step-remove',
    )
    fireEvent.click(removeButtons[1]!)
    flush()
    expect(
      [...container.querySelectorAll<HTMLInputElement>('.verevon-chat-plan__step-input')].map(
        (el) => el.value,
      ),
    ).toEqual(['Draft the email', 'Send it'])

    // Move the last VISIBLE row ("Send it") up: it must land ahead of
    // "Draft the email", not silently swap with the hidden removed row.
    const rows = container.querySelectorAll('.verevon-chat-plan__step')
    const moveUp = rows[1]!.querySelector<HTMLButtonElement>('button[aria-label="Flytt opp"]')!
    fireEvent.click(moveUp)
    flush()
    expect(
      [...container.querySelectorAll<HTMLInputElement>('.verevon-chat-plan__step-input')].map(
        (el) => el.value,
      ),
    ).toEqual(['Send it', 'Draft the email'])
    unmount()
  })

  it('rejects the plan with a reason, granting nothing', () => {
    const onApprove = vi.fn()
    const onReject = vi.fn()
    const { container, unmount } = render(() => (
      <PlanApprovalControl {...planProps} pending={false} onApprove={onApprove} onReject={onReject} />
    ))
    const reject = container.querySelector<HTMLButtonElement>('.verevon-chat-plan__reject')!
    fireEvent.click(reject)
    flush()

    expect(onReject).toHaveBeenCalledWith('plan_1', expect.any(String))
    expect(onApprove).not.toHaveBeenCalled()
    unmount()
  })

  it('disables approve once every step has been removed', () => {
    const onApprove = vi.fn()
    const { container, unmount } = render(() => (
      <PlanApprovalControl {...planProps} pending={false} onApprove={onApprove} />
    ))
    fillReason(container)
    // Re-query fresh each time rather than looping over a snapshot: removing
    // a row re-keys the visible list (each `{row, index}` wrapper is a new
    // object), so a stale button reference from before the first removal is
    // no longer live.
    let remove = container.querySelector<HTMLButtonElement>('.verevon-chat-plan__step-remove')
    while (remove) {
      fireEvent.click(remove)
      flush()
      remove = container.querySelector<HTMLButtonElement>('.verevon-chat-plan__step-remove')
    }
    const approve = container.querySelector<HTMLButtonElement>('.verevon-chat-plan__approve')!
    expect(approve.disabled).toBe(true)
    fireEvent.click(approve)
    flush()
    expect(onApprove).not.toHaveBeenCalled()
    expect(container.textContent).toContain('minst ett steg')
    unmount()
  })

  it('does not send a step that was added and never filled in', () => {
    const onApprove = vi.fn()
    const { container, unmount } = render(() => (
      <PlanApprovalControl {...planProps} pending={false} onApprove={onApprove} />
    ))
    fireEvent.click(container.querySelector<HTMLButtonElement>('.verevon-chat-plan__add-step')!)
    flush()
    expect(container.querySelectorAll('.verevon-chat-plan__step-input')).toHaveLength(3)

    fillReason(container)
    fireEvent.click(container.querySelector<HTMLButtonElement>('.verevon-chat-plan__approve')!)
    flush()

    // The server refuses a step with no title; a blank new row never belonged
    // to the proposal, so it is dropped, not sent.
    const editedSteps = onApprove.mock.calls[0]![2]
    expect(editedSteps.map((s: { stepId?: string }) => s.stepId)).toEqual(['step_1', 'step_2'])
    unmount()
  })

  it('does not count a blank new row as the plan\'s remaining step', () => {
    const onApprove = vi.fn()
    const { container, unmount } = render(() => (
      <PlanApprovalControl {...planProps} pending={false} onApprove={onApprove} />
    ))
    fillReason(container)
    fireEvent.click(container.querySelector<HTMLButtonElement>('.verevon-chat-plan__add-step')!)
    flush()
    let remove = container.querySelector<HTMLButtonElement>('.verevon-chat-plan__step-remove')
    // Remove both real steps (the blank new row's remove button is last).
    for (let i = 0; i < 2 && remove; i += 1) {
      fireEvent.click(remove)
      flush()
      remove = container.querySelector<HTMLButtonElement>('.verevon-chat-plan__step-remove')
    }
    // Only the blank row is left: that is not a plan.
    expect(container.querySelectorAll('.verevon-chat-plan__step-input')).toHaveLength(1)
    expect(container.querySelector<HTMLButtonElement>('.verevon-chat-plan__approve')!.disabled).toBe(true)
    expect(container.textContent).toContain('minst ett steg')

    // Typing a title makes it a step, and approving becomes possible.
    fireEvent.input(container.querySelector<HTMLInputElement>('.verevon-chat-plan__step-input')!, {
      target: { value: 'Call the customer instead' },
    })
    flush()
    expect(container.querySelector<HTMLButtonElement>('.verevon-chat-plan__approve')!.disabled).toBe(false)
    unmount()
  })

  it('will not approve while a kept step has been emptied', () => {
    const onApprove = vi.fn()
    const { container, unmount } = render(() => (
      <PlanApprovalControl {...planProps} pending={false} onApprove={onApprove} />
    ))
    fillReason(container)
    const approve = () =>
      container.querySelector<HTMLButtonElement>('.verevon-chat-plan__approve')!
    expect(approve().disabled).toBe(false)

    const first = container.querySelector<HTMLInputElement>('.verevon-chat-plan__step-input')!
    fireEvent.input(first, { target: { value: '   ' } })
    flush()
    expect(approve().disabled).toBe(true)
    expect(container.textContent).toMatch(/Hvert steg trenger en tittel|Every step needs a title/)
    fireEvent.click(approve())
    flush()
    expect(onApprove).not.toHaveBeenCalled()

    fireEvent.input(first, { target: { value: 'Draft the email again' } })
    flush()
    expect(approve().disabled).toBe(false)
    unmount()
  })

  it('shows a rejected plan as rejected, with nothing left to decide', () => {
    const { container, unmount } = render(() => (
      <PlanApprovalControl {...planProps} rejected pending={false} onApprove={() => {}} />
    ))
    expect(container.querySelector('.verevon-chat-plan')).toBeNull()
    expect(container.querySelector('.verevon-chat-plan__approve')).toBeNull()
    expect(container.textContent).toMatch(/Planen ble avvist|The plan was rejected/)
    unmount()
  })
})

/**
 * F-16. A deep-research run lists every hit it found and reads a handful of
 * them, and the panel used to render both as the same card — so "24 kilder"
 * read as "24 pages were read" when three had been.
 */
const read = (overrides: Partial<Citation> = {}): Citation & { kind: 'web' } => ({
  kind: 'web',
  id: 'cite-1',
  title: 'Mattilsynet om fiskehelse',
  url: 'https://mattilsynet.no/fiskehelse',
  snippet: 'Regelverket krever journalføring ved behandling.',
  ...overrides,
})
// How the backend marks a hit it never fetched: both the id prefix and the
// snippet marker, which is what the panel groups on.
const unread = (overrides: Partial<Citation> = {}): Citation & { kind: 'web' } => ({
  kind: 'web',
  id: 'dr-unread-7',
  title: 'Oppdrett i Rogaland',
  url: 'https://example.no/rogaland',
  snippet: '[not read: fetch timed out] Oversikt over anlegg i Rogaland.',
  ...overrides,
})
/** An assistant turn carrying only the citations under test. */
const turn = (citations: Citation[]): ChatTurn => ({
  id: 'turn-1',
  role: 'assistant',
  content: 'Svar.',
  createdAt: '2026-09-15T08:00:00.000Z',
  streaming: false,
  tools: [],
  attachments: [],
  citations,
})

describe('SourcesPanel read/unread grouping', () => {
  it('separates the leads from the evidence and states both counts', () => {
    const { container, unmount } = render(() => (
      <SourcesPanel
        sources={[
          read(),
          read({ id: 'cite-2', url: 'https://fiskeridir.no/a' }),
          unread(),
          unread({ id: 'dr-unread-8', url: 'https://example.no/b' }),
          unread({ id: 'dr-unread-9', url: 'https://example.no/c' }),
        ]}
      />
    ))
    const text = container.textContent ?? ''
    // The reader must never be able to believe five pages were read.
    expect(text).toContain('2 av 5 kilder er lest')
    expect(text).toContain('Ikke lest')
    expect(text).toContain('ikke hentet')
    // And the grouping has to be structural, not just a sentence: the three
    // leads sit inside the secondary group, the two read sources outside it.
    const grouped = container.querySelectorAll(
      '.verevon-chat-source-group--unread .verevon-chat-source-card',
    )
    expect(grouped).toHaveLength(3)
    expect(container.querySelectorAll('.verevon-chat-source-card')).toHaveLength(5)
    unmount()
  })

  it('drops the wire marker from the snippet the reader sees', () => {
    const { container, unmount } = render(() => (
      <SourcesPanel sources={[read(), unread()]} />
    ))
    const text = container.textContent ?? ''
    // The heading now carries what the prefix used to say, in Norwegian.
    expect(text).toContain('Oversikt over anlegg i Rogaland')
    expect(text).not.toContain('not read')
    unmount()
  })

  /** A lead whose snippet was nothing but the marker must render no paragraph. */
  it('renders no snippet at all rather than an empty one', () => {
    const { container, unmount } = render(() => (
      <SourcesPanel sources={[unread({ snippet: '[not read: 403]' })]} />
    ))
    expect(
      container.querySelector('.verevon-chat-source-group--unread .verevon-chat-source-card p'),
    ).toBeNull()
    unmount()
  })

  /** The other boundary: a run that fetched nothing it found. */
  it('states a zero tally rather than implying the list is evidence', () => {
    const { container, unmount } = render(() => (
      <SourcesPanel
        sources={[unread(), unread({ id: 'dr-unread-8', url: 'https://example.no/b' })]}
      />
    ))
    const text = container.textContent ?? ''
    expect(text).toContain('0 av 2 kilder er lest')
    // Nothing above the "Ikke lest" group, so no heading claiming read evidence.
    expect(text).not.toContain('Lest og brukt')
    expect(container.querySelectorAll(
      '.verevon-chat-source-group--unread .verevon-chat-source-card',
    )).toHaveLength(2)
    unmount()
  })

  /** The ordinary case — a web-search turn that read what it cited — is untouched. */
  it('stays a flat list with no headings when every source was read', () => {
    const { container, unmount } = render(() => (
      <SourcesPanel sources={[read(), read({ id: 'cite-2', url: 'https://fiskeridir.no/a' })]} />
    ))
    const text = container.textContent ?? ''
    expect(text).not.toContain('Ikke lest')
    expect(text).not.toContain('kilder er lest')
    expect(container.querySelector('.verevon-chat-source-group')).toBeNull()
    unmount()
  })

  /** chat-wave2 citations spec §2.6: domain/provider sub-grouping within the
   * read section, keyed on `sourceGroupId` (falling back to hostname). */
  it('renders a domain heading per group when read sources carry distinct sourceGroupId', () => {
    const { container, unmount } = render(() => (
      <SourcesPanel
        sources={[
          read({ id: 'cite-kb-1', url: 'https://intranet/refunds', sourceGroupId: 'kb:sharepoint', sourceGroupLabel: 'Microsoft 365' }),
          read({ id: 'cite-kb-2', url: 'https://intranet/faq', sourceGroupId: 'kb:sharepoint', sourceGroupLabel: 'Microsoft 365' }),
          read({ id: 'cite-web-1', url: 'https://www.aquatiq.com/refunds', sourceGroupId: 'aquatiq.com' }),
        ]}
      />
    ))
    const headings = Array.from(container.querySelectorAll('.verevon-chat-source-domain-group__heading'))
      .map((node) => node.textContent)
    expect(headings).toEqual(['Microsoft 365', 'aquatiq.com'])
    const groups = container.querySelectorAll('.verevon-chat-source-domain-group')
    expect(groups).toHaveLength(2)
    expect(groups[0]?.querySelectorAll('.verevon-chat-source-card')).toHaveLength(2)
    expect(groups[1]?.querySelectorAll('.verevon-chat-source-card')).toHaveLength(1)
    unmount()
  })

  it('omits the domain heading when every read source falls into a single group', () => {
    const { container, unmount } = render(() => (
      <SourcesPanel
        sources={[
          read({ id: 'cite-1', url: 'https://example.com/a', sourceGroupId: 'example.com' }),
          read({ id: 'cite-2', url: 'https://www.example.com/b', sourceGroupId: 'example.com' }),
        ]}
      />
    ))
    expect(container.querySelector('.verevon-chat-source-domain-group__heading')).toBeNull()
    expect(container.querySelectorAll('.verevon-chat-source-card')).toHaveLength(2)
    unmount()
  })

  it('falls back to hostname() grouping for a knowledge source with a provider and a web source with no sourceGroupId', () => {
    const knowledgeSource: ChatGroundingSource = {
      id: 'k1',
      kind: 'knowledge',
      title: 'Internrutine',
      snippet: 'Rutine for avvik.',
      provider: 'Notion',
      sourceType: 'document',
      documentId: 'doc-1',
      href: '/knowledge/doc-1',
      score: 0.81,
    }
    const { container, unmount } = render(() => (
      <SourcesPanel sources={[knowledgeSource, read({ id: 'cite-web-1', url: 'https://example.com/a' })]} />
    ))
    const headings = Array.from(container.querySelectorAll('.verevon-chat-source-domain-group__heading'))
      .map((node) => node.textContent)
    expect(headings).toEqual(['Notion', 'example.com'])
    unmount()
  })

  /** Knowledge grounding is retrieved as text — there is no fetch that could fail. */
  it('never files internal knowledge as unread', () => {
    expect(partitionEvidenceSources([
      {
        id: 'k1',
        kind: 'knowledge',
        title: 'Internrutine',
        snippet: 'Rutine for avvik.',
        provider: 'sharepoint',
        sourceType: 'document',
        documentId: 'doc-1',
        href: '/knowledge/doc-1',
        score: 0.81,
      },
    ]).unread).toHaveLength(0)
  })

  it('promotes a lead to read once the same page is fetched', () => {
    // `collectEvidenceSources` dedupes by URL, and a page is normally cited as a
    // lead before it is fetched — so plain first-write-wins would pin a source
    // that WAS read under "Ikke lest" for the rest of the thread.
    const url = 'https://example.no/rogaland'
    const collected = collectEvidenceSources([
      turn([unread({ url })]),
      turn([read({ id: 'cite-9', url, snippet: 'Hentet og lest.' })]),
    ])
    expect(collected).toHaveLength(1)
    expect(partitionEvidenceSources(collected).unread).toHaveLength(0)
    expect(collected[0]?.snippet).toBe('Hentet og lest.')
  })

  it('does not let a later lead demote a source already read', () => {
    const url = 'https://example.no/rogaland'
    const collected = collectEvidenceSources([
      turn([read({ id: 'cite-9', url, snippet: 'Hentet og lest.' })]),
      turn([unread({ url })]),
    ])
    expect(partitionEvidenceSources(collected).read).toHaveLength(1)
  })
})

describe('ChatTabs Kilder badge', () => {
  it('counts the sources that were read, not the leads that were only found', () => {
    const { container, unmount } = render(() => (
      <ChatTabs
        active="chat"
        artifactCount={0}
        sourceCount={24}
        readSourceCount={3}
        stepCount={0}
        onChange={() => {}}
      />
    ))
    const sources = [...container.querySelectorAll('[role="tab"]')].find((tab) =>
      tab.textContent?.includes('Kilder'),
    )
    expect(sources?.querySelector('em')?.textContent).toBe('3')
    expect(container.textContent).not.toContain('24')
    unmount()
  })

  /** Without the read count — every other caller — the badge is unchanged. */
  it('falls back to the total when no read count is supplied', () => {
    const { container, unmount } = render(() => (
      <ChatTabs active="chat" artifactCount={0} sourceCount={4} stepCount={0} onChange={() => {}} />
    ))
    const sources = [...container.querySelectorAll('[role="tab"]')].find((tab) =>
      tab.textContent?.includes('Kilder'),
    )
    expect(sources?.querySelector('em')?.textContent).toBe('4')
    unmount()
  })
})

/**
 * The number itself, end to end: what `ChatPage` derives for the badge is
 * `partitionEvidenceSources(...).read.length`, and what the tab strip renders
 * from it. Availability stays on the full list — an unread lead is still worth
 * offering — so across these four cases only the NUMBER differs.
 */
describe('Kilder count', () => {
  /** The count ChatPage's `readSourceCount` memo derives. */
  const count = (sources: Array<Citation & { kind: 'web' }>) =>
    partitionEvidenceSources(sources).read.length

  /** What the tab strip renders for a given pair, or null when it renders none. */
  const badge = (sourceCount: number, readSourceCount: number) => {
    const { container, unmount } = render(() => (
      <ChatTabs
        active="chat"
        artifactCount={0}
        sourceCount={sourceCount}
        readSourceCount={readSourceCount}
        stepCount={0}
        onChange={() => {}}
      />
    ))
    const tab = [...container.querySelectorAll('[role="tab"]')].find((item) =>
      item.textContent?.includes('Kilder'),
    )
    expect(tab, 'the Kilder tab must stay available whatever the read count').toBeTruthy()
    const text = tab?.querySelector('em')?.textContent ?? null
    unmount()
    return text
  }

  it('counts every source when the turn read everything it cited', () => {
    const sources = [read(), read({ id: 'cite-2', url: 'https://fiskeridir.no/a' })]
    expect(count(sources)).toBe(2)
    expect(badge(sources.length, count(sources))).toBe('2')
  })

  it('renders no number at all when the turn read none of its hits', () => {
    // A `0` beside "Kilder" would read as "this panel is empty" — it is not,
    // it holds two leads. The tab stays offered; only the claim goes away.
    const sources = [unread(), unread({ id: 'dr-unread-8', url: 'https://example.no/b' })]
    expect(count(sources)).toBe(0)
    expect(badge(sources.length, 0)).toBeNull()
  })

  it('counts only the read half of a mixed list', () => {
    const sources = [read(), unread(), unread({ id: 'dr-unread-8', url: 'https://example.no/b' })]
    expect(count(sources)).toBe(1)
    expect(badge(sources.length, count(sources))).toBe('1')
  })

  it('counts a legacy "[not read: …]" snippet as unread without the id prefix', () => {
    // The id prefix arrived after the snippet marker, so a deep-research frame
    // from an older gateway carries only the marker. Reading just one of the two
    // markers inflates the badge straight back to the number F-16 was about.
    const legacy = read({
      id: 'cite-4',
      url: 'https://example.no/legacy',
      snippet: '[not read: 404] Fant ikke siden.',
    })
    expect(count([read(), legacy])).toBe(1)
    expect(badge(2, count([read(), legacy]))).toBe('1')
  })

  /**
   * The same count, spoken. `ChatPage`'s live region announced only that
   * Verevon was working, so a screen-reader user got no equivalent of the
   * visible "3 av 24" — the one number that says the other twenty-one were
   * never read.
   */
  describe('spoken in the stream announcement', () => {
    it('names both numbers while something went unread', () => {
      const sources = [read(), unread(), unread({ id: 'dr-unread-8', url: 'https://example.no/b' })]
      expect(spokenSourceTally(count(sources), sources.length)).toBe(' 1 av 3 kilder lest.')
    })

    it('names one number when every source was read', () => {
      const sources = [read(), read({ id: 'cite-2', url: 'https://fiskeridir.no/a' })]
      expect(spokenSourceTally(count(sources), sources.length)).toBe(' 2 kilder lest.')
    })

    it('says none of them were read rather than staying silent about it', () => {
      const sources = [unread(), unread({ id: 'dr-unread-8', url: 'https://example.no/b' })]
      expect(spokenSourceTally(count(sources), sources.length)).toBe(' 0 av 2 kilder lest.')
    })

    it('says nothing at all on a turn with no sources', () => {
      // Otherwise the heartbeat reads "0 kilder lest" every ten seconds on every
      // plain Ask turn, which is a metronome rather than information.
      expect(spokenSourceTally(0, 0)).toBe('')
    })
  })
})

/**
 * F-07. Some routes take 60-95 seconds before the first token, and a static
 * "Tenker" over that long reads as a hung page.
 */
describe('ThinkingDots elapsed wait', () => {
  afterEach(() => {
    vi.useRealTimers()
  })

  it('stays wordless for an ordinary wait', () => {
    vi.useFakeTimers()
    const { container, unmount } = render(() => <ThinkingDots />)
    vi.advanceTimersByTime(9_000)
    flush()
    expect(container.textContent).toContain('Tenker')
    expect(container.querySelector('.verevon-chat-thinking__elapsed')).toBeNull()
    unmount()
  })

  it('names how long it has waited once the wait is long enough to read as a hang', () => {
    vi.useFakeTimers()
    const { container, unmount } = render(() => <ThinkingDots />)
    vi.advanceTimersByTime(12_000)
    flush()
    expect(container.querySelector('.verevon-chat-thinking__elapsed')?.textContent).toBe('12 s')
    vi.advanceTimersByTime(63_000)
    flush()
    expect(container.querySelector('.verevon-chat-thinking__elapsed')?.textContent).toBe('1 min 15 s')
    // Nothing fabricated alongside it: no bar, no estimate, no invented phase.
    expect(container.textContent).not.toContain('%')
    unmount()
  })

  it('stops ticking when the answer arrives', () => {
    vi.useFakeTimers()
    const { unmount } = render(() => <ThinkingDots />)
    expect(vi.getTimerCount()).toBe(1)
    unmount()
    expect(vi.getTimerCount()).toBe(0)
  })

  /**
   * The remount. `<For>` keys on item identity and every derivation of the node
   * list builds fresh objects, so a streaming turn rebuilds this indicator on
   * each tool call — measured in this tree: two rows became four after a single
   * re-derivation. A rebuilt indicator that restarted its own clock reset the
   * counter every few hundred milliseconds on exactly the deep-research routes
   * the counter exists for, so it never survived to the ten-second threshold.
   */
  it('counts from the turn it was handed, not from its own mount', () => {
    vi.useFakeTimers()
    vi.setSystemTime(new Date('2026-09-15T08:00:42.000Z'))
    const { container, unmount } = render(() => (
      <ThinkingDots since="2026-09-15T08:00:00.000Z" />
    ))
    flush()
    // No tick has run: a rebuilt indicator has to be right on its first frame,
    // or it blinks back through zero once per rebuild.
    expect(container.querySelector('.verevon-chat-thinking__elapsed')?.textContent).toBe('42 s')
    vi.advanceTimersByTime(20_000)
    flush()
    expect(container.querySelector('.verevon-chat-thinking__elapsed')?.textContent).toBe('1 min 2 s')
    unmount()
  })

  it('falls back to its own mount when the turn carries no usable start', () => {
    vi.useFakeTimers()
    const { container, unmount } = render(() => <ThinkingDots since="not a timestamp" />)
    vi.advanceTimersByTime(12_000)
    flush()
    expect(container.querySelector('.verevon-chat-thinking__elapsed')?.textContent).toBe('12 s')
    unmount()
  })

  /**
   * The wiring, not just the component: the turn's start has to reach the
   * indicator. Rendering the answer region from a derived `pending` node is what
   * fails if the region ever goes back to a bare `<ThinkingDots />` — both the
   * derivation test and the component tests above still pass in that state.
   */
  it('reaches the indicator from a derived pending answer node', () => {
    vi.useFakeTimers()
    vi.setSystemTime(new Date('2026-09-15T08:01:15.000Z'))
    const node = deriveConversationNodes({
      ...turn([]),
      content: '',
      status: 'waiting',
      createdAt: '2026-09-15T08:00:00.000Z',
    }).find((candidate) => candidate.kind === 'answer')
    expect(node).toMatchObject({ answer: { state: 'pending' } })
    const { container, unmount } = render(() => (
      <AnswerRegion
        answer={(node as Extract<typeof node, { kind: 'answer' }>).answer}
        onRegenerate={() => {}}
      />
    ))
    flush()
    expect(container.querySelector('.verevon-chat-thinking__elapsed')?.textContent).toBe('1 min 15 s')
    unmount()
  })

  /**
   * The progress half of F-07. Elapsed seconds say the page is not frozen; they
   * do not say what is happening, which is what DeepSeek and ChatGPT put on
   * screen while Verevon showed a spinner label.
   */
  it('names the tool that is running, beside the indicator', () => {
    vi.useFakeTimers()
    const { container, unmount } = render(() => (
      <ThinkingDots activity={{ kind: 'tool-running', tool: 'Web search' }} />
    ))
    flush()
    expect(container.querySelector('.verevon-chat-thinking__activity')?.textContent).toBe(
      'Kjører Web search',
    )
    // Not gated behind the elapsed threshold: a running tool is worth showing
    // from the first second, and it cannot be mistaken for a stall.
    expect(container.querySelector('.verevon-chat-thinking__elapsed')).toBeNull()
    unmount()
  })

  /**
   * The honest fallback, and the case the audit's own repro sits in: the
   * subscription route reports no tool calls for its entire 90-second wait.
   * Elapsed time is then all that is known, and the line must stay empty rather
   * than fill with an invented phase.
   */
  it('shows elapsed time alone when no step information has arrived', () => {
    vi.useFakeTimers()
    const { container, unmount } = render(() => <ThinkingDots />)
    vi.advanceTimersByTime(45_000)
    flush()
    expect(container.querySelector('.verevon-chat-thinking__activity')).toBeNull()
    expect(container.querySelector('.verevon-chat-thinking__elapsed')?.textContent).toBe('45 s')
    expect(container.textContent).toBe('Tenker45 s')
    unmount()
  })

  /**
   * The wiring: a real streamed tool call has to reach the indicator through the
   * derived node, not just through a hand-built prop.
   */
  it('carries the live tool call through the derived pending node', () => {
    vi.useFakeTimers()
    const node = deriveConversationNodes({
      ...turn([]),
      content: '',
      status: 'waiting',
      toolCalls: [
        { id: 'tc-1', name: 'web_search', status: 'done' },
        { id: 'tc-2', name: 'knowledge_search', status: 'running' },
      ],
    }).find((candidate) => candidate.kind === 'answer')
    const { container, unmount } = render(() => (
      <AnswerRegion
        answer={(node as Extract<typeof node, { kind: 'answer' }>).answer}
        onRegenerate={() => {}}
      />
    ))
    flush()
    expect(container.querySelector('.verevon-chat-thinking__activity')?.textContent).toBe(
      'Kjører Knowledge search',
    )
    unmount()
  })

  it('derives no activity for a pending turn that has run nothing', () => {
    const node = deriveConversationNodes({
      ...turn([]),
      content: '',
      status: 'waiting',
    }).find((candidate) => candidate.kind === 'answer')
    expect(node).toMatchObject({ answer: { state: 'pending' } })
    // The absent key, not an `activity: undefined` placeholder — `toEqual`
    // treats those alike, so the shape is asserted directly.
    const answer = (node as Extract<typeof node, { kind: 'answer' }>).answer
    expect(Object.keys(answer)).toEqual(['state', 'since'])
  })

  it('does not count towards a start timestamp in the future', () => {
    // Clock skew, not a wait that has not begun. Left unclamped this counts up
    // to zero and the label stays hidden for however far ahead the stamp is.
    vi.useFakeTimers()
    vi.setSystemTime(new Date('2026-09-15T08:00:00.000Z'))
    const { container, unmount } = render(() => (
      <ThinkingDots since="2026-09-15T08:05:00.000Z" />
    ))
    vi.advanceTimersByTime(12_000)
    flush()
    expect(container.querySelector('.verevon-chat-thinking__elapsed')?.textContent).toBe('12 s')
    unmount()
  })
})

describe('formatElapsedWait', () => {
  it('floors seconds so it never claims one that has not elapsed', () => {
    expect(formatElapsedWait(0)).toBe('0 s')
    expect(formatElapsedWait(10_900)).toBe('10 s')
  })

  it('breaks a long wait into minutes rather than counting to 95', () => {
    expect(formatElapsedWait(60_000)).toBe('1 min 0 s')
    expect(formatElapsedWait(95_400)).toBe('1 min 35 s')
  })
})

describe('RunPlanPanel', () => {
  const wait = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms))

  it('shows the plan during step churn and does not refetch per step', async () => {
    // The panel used to key its resource on the step count, so every step
    // restarted the fetch. Under a deep-research run (a step every ~400ms, a
    // ~450ms fetch) almost no fetch survived to resolve: the panel read
    // "Laster plan og oppgaver …" for the whole run and fired three requests
    // per step. Here the fetch takes 40ms and steps arrive every 10ms.
    const listPlans = vi.mocked(orchestration.listPlans)
    listPlans.mockReset()
    listPlans.mockImplementation(() => new Promise((resolve) => {
      setTimeout(() => resolve([{
        id: 'plan_1',
        runId: 'r1',
        summary: 'Kartlegg leverandører',
        state: 'PLAN_STATE_EXECUTING',
        steps: [{ id: 's1', title: '', operation: 'tool_execution', state: 'PLAN_STEP_STATE_RUNNING' }],
      }]), 40)
    }))
    const [progress, setProgress] = createSignal(0)
    const { container, unmount } = render(() => (
      <RunPlanPanel runId="r1" threadId="t1" progressKey={progress()} />
    ))

    for (let step = 1; step <= 30; step++) {
      setProgress(step)
      await wait(10)
      if (step === 15) {
        // Mid-churn: the initial fetch (40ms) must have landed and stayed.
        const text = container.textContent ?? ''
        expect(text).toContain('Kartlegg leverandører')
        expect(text).not.toContain('Laster plan og oppgaver')
      }
    }
    await wait(60)

    const text = container.textContent ?? ''
    expect(text).toContain('Kartlegg leverandører')
    expect(text).not.toContain('Laster plan og oppgaver')
    // 30 step changes are not 30 fetches. The initial load, plus at most one
    // throttled refresh if the 1.5s interval happened to elapse.
    expect(listPlans.mock.calls.length).toBeLessThanOrEqual(2)
    unmount()
  })

  it('keeps the last plan on screen while a refresh runs', async () => {
    const listPlans = vi.mocked(orchestration.listPlans)
    listPlans.mockReset()
    listPlans.mockImplementation(async () => [{ id: 'plan_1', runId: 'r1', summary: 'Første plan', state: 'PLAN_STATE_EXECUTING' }])
    const [progress, setProgress] = createSignal(0)
    const { container, unmount } = render(() => (
      <RunPlanPanel runId="r1" threadId="t1" progressKey={progress()} refreshIntervalMs={200} />
    ))
    await wait(20)
    expect(container.textContent).toContain('Første plan')
    // A slow refresh must not blank the panel back to the loading note.
    listPlans.mockImplementation(() => new Promise((resolve) => {
      setTimeout(() => resolve([{ id: 'plan_1', runId: 'r1', summary: 'Andre plan', state: 'PLAN_STATE_COMPLETE' }]), 150)
    }))
    setProgress(1)
    // The throttle holds the refresh until the interval has elapsed since mount
    // (~200ms); by 270ms it is in flight and 80ms from landing.
    await wait(250)
    expect(listPlans.mock.calls.length).toBe(2)
    const during = container.textContent ?? ''
    expect(during).toContain('Første plan')
    expect(during).not.toContain('Laster plan og oppgaver')
    await wait(200)
    // …and the refreshed plan replaces it once the fetch lands.
    expect(container.textContent).toContain('Andre plan')
    unmount()
  })

  it('holds the refresh interval even when fetches are slow and steps keep arriving', async () => {
    // Observed live: a timer that fired mid-fetch cleared its gate, later steps
    // re-armed zero-wait timers from a stale timestamp, and refetches ran
    // back-to-back at fetch-duration cadence (28 requests in a 74s run at a 4s
    // interval). Interval 200ms, fetch 120ms, a step every 20ms for ~1s: the
    // interval must win, so roughly 1 + 1000/200 fetches, never 1 + 1000/120.
    const listPlans = vi.mocked(orchestration.listPlans)
    listPlans.mockReset()
    listPlans.mockImplementation(() => new Promise((resolve) => {
      setTimeout(() => resolve([{ id: 'plan_1', runId: 'r1', summary: 'Plan', state: 'PLAN_STATE_EXECUTING' }]), 120)
    }))
    const [progress, setProgress] = createSignal(0)
    const started = performance.now()
    const { unmount } = render(() => (
      <RunPlanPanel runId="r1" threadId="t1" progressKey={progress()} refreshIntervalMs={200} />
    ))
    for (let step = 1; step <= 50; step++) {
      setProgress(step)
      await wait(20)
    }
    await wait(150)
    // jsdom timers overrun, so bound by the measured window rather than a fixed
    // count: one fetch per interval (plus the initial load and one of slack),
    // and strictly fewer than back-to-back at fetch duration would produce.
    const elapsed = performance.now() - started
    const calls = listPlans.mock.calls.length
    expect(calls).toBeLessThanOrEqual(1 + Math.ceil(elapsed / 200) + 1)
    expect(calls).toBeLessThan(1 + Math.floor(elapsed / 120))
    expect(calls).toBeGreaterThanOrEqual(3)
    unmount()
  })
})
