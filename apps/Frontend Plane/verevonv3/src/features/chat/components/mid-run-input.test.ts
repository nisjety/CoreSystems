/**
 * The send path must never silently discard a message again.
 *
 * `sendContent` began `if (!content || state.status === 'streaming') return` —
 * a message typed while the agent was working was not queued, not refused, and
 * not shown as rejected. It was gone, and the user had to retype it.
 *
 * The replacement is behavioural and lives across three files, so a compile
 * cannot see it: the guard has to hand the message to `deliverMidRun`, the
 * strip has to be rendered, and the delivery event has to promote it. A
 * refactor that restored the bare `return` would type-check perfectly and the
 * bug would be back, invisible.
 *
 * Read as source text for the same reason `run-console-client.test.ts` does:
 * the property is a call shape inside a large hook with no seam to observe it
 * through, and it is worth pinning anyway.
 */

import { describe, expect, it } from 'vitest'

const CONTROLLER = 'src/features/chat/components/use-chat-controller.ts'
const PAGE = 'src/features/chat/components/ChatPage.tsx'

/**
 * Read a source file with its line endings normalised to LF.
 *
 * The guards below search for multi-line anchors written as LF string
 * literals. `use-chat-controller.ts` is checked out with CRLF (2,813 pairs,
 * not one bare LF), so every multi-line anchor silently failed to match and
 * the guard reported the effect it was protecting as "gone". Normalising
 * here makes these tests work under either convention instead of depending
 * on how git happened to materialise the file.
 */
async function source(relative: string): Promise<string> {
  const { readFileSync } = await import('node:fs')
  const { resolve } = await import('node:path')
  const carriageReturn = String.fromCharCode(13)
  return readFileSync(resolve(process.cwd(), relative), 'utf8')
    .split(carriageReturn)
    .join('')
}

describe('mid-run input is never dropped', () => {
  it('routes a send during a stream to delivery rather than returning', async () => {
    const text = await source(CONTROLLER)
    expect(
      text.includes("if (!content || state.status === 'streaming') return"),
      'the original silent drop is back: a message typed mid-run is discarded with no trace',
    ).toBe(false)

    const guard = text.slice(
      text.indexOf('const sendContent = async'),
      text.indexOf('let activeThreadId'),
    )
    expect(
      guard.length,
      'sendContent not found — re-point this test',
    ).toBeGreaterThan(0)
    expect(
      guard.includes("state.status === 'streaming'") &&
        guard.includes('deliverMidRun'),
      `a send during a stream must reach deliverMidRun; guard is: ${guard}`,
    ).toBe(true)
  })

  /**
   * The three outcomes each need their own handling, and the one that matters
   * most is `run_ended` — the ordinary race where the stream closed between the
   * keystroke and the request. Treating it as a refusal would lose the message
   * for a reason that is nobody's fault.
   */
  it('defers a message the run ended too early to receive instead of refusing it', async () => {
    const text = await source(CONTROLLER)
    const delivery = text.slice(
      text.indexOf('const deliverMidRun'),
      text.indexOf('const sendContent = async'),
    )
    expect(
      delivery.length,
      'deliverMidRun not found — re-point this test',
    ).toBeGreaterThan(0)
    for (const outcome of ['queued', 'run_ended']) {
      expect(
        delivery.includes(`'${outcome}'`),
        `deliverMidRun does not handle the ${outcome} outcome`,
      ).toBe(true)
    }
    expect(
      delivery.includes('setDeferredSends'),
      'a run that ended before the message landed must still send it, not drop it',
    ).toBe(true)
  })

  /**
   * The flush is a reactive effect precisely so no terminal path can forget it —
   * the stream settles through several of them. It must also clear the queue
   * BEFORE sending, or flipping the status re-runs the effect over the same text.
   */
  it('flushes deferred sends from one reactive place, clearing before it sends', async () => {
    const text = await source(CONTROLLER)
    const start = text.indexOf(
      "createEffect(\n    () => ({\n      status: state.status,\n      deferred: deferredSends(),",
    )
    expect(start, 'the deferred-send flush effect is gone').toBeGreaterThan(-1)
    const effect = text.slice(start, text.indexOf('  const addAssistantCitation', start))
    expect(effect.indexOf('setDeferredSends(remaining)')).toBeGreaterThan(-1)
    expect(
      effect.indexOf('setDeferredSends(remaining)') <
        effect.indexOf('sendContent(next.content, next.modelOverride, next.options)'),
      'advancing after sending re-runs this effect over the same item and sends it twice',
    ).toBe(true)
  })

  it('preserves the selected model and subscription route when a missed delivery is replayed', async () => {
    const text = await source(CONTROLLER)
    const delivery = text.slice(
      text.indexOf('const deliverMidRun'),
      text.indexOf('const sendContent = async'),
    )
    const start = text.indexOf(
      "createEffect(\n    () => ({\n      status: state.status,\n      deferred: deferredSends(),",
    )
    const effect = text.slice(start, text.indexOf('  const addAssistantCitation', start))

    expect(delivery).toContain('{ content, modelOverride, options: { ...options } }')
    expect(effect).toContain('sendContent(next.content, next.modelOverride, next.options)')
  })

  it('never retries a subscription selection through a platform-paid fallback provider', async () => {
    const text = await source(CONTROLLER)
    const handler = text.slice(
      text.indexOf('onError: ({ message })'),
      text.indexOf('onFrameId:', text.indexOf('onError: ({ message })')),
    )

    expect(handler).toContain('options.provider !== OPENAI_CODEX_SUBSCRIPTION_PROVIDER')
  })

  it('preserves the subscription route when regenerating the latest answer', async () => {
    const text = await source(CONTROLLER)
    const regenerate = text.slice(
      text.indexOf('const regenerateLatest'),
      text.indexOf('const rerunAsNewTurn'),
    )

    expect(regenerate).toContain('provider: lastUser.provider')
    expect(regenerate).toContain('subscriptionConnectionId: lastUser.subscriptionConnectionId')
  })

  it('renders the strip, so a queued message is visible while it is in flight', async () => {
    const page = await source(PAGE)
    expect(
      page.includes('<QueuedInputStrip entries={state.queuedInputs} />'),
      'nothing renders state.queuedInputs — the message is tracked but invisible, ' +
        'which is the bug with extra steps',
    ).toBe(true)
  })

  it('promotes a pending message to delivered when the agent is handed it', async () => {
    const text = await source(CONTROLLER)
    const handler = text.slice(
      text.indexOf('onQueuedInput:'),
      text.indexOf('onMemoryRecall:'),
    )
    expect(handler.length, 'onQueuedInput is not wired').toBeGreaterThan(0)
    expect(handler).toContain("'delivered'")
  })
})
