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

async function source(relative: string): Promise<string> {
  const { readFileSync } = await import('node:fs')
  const { resolve } = await import('node:path')
  return readFileSync(resolve(process.cwd(), relative), 'utf8')
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
      "createEffect(() => {\n    if (state.status === 'streaming') return",
    )
    expect(start, 'the deferred-send flush effect is gone').toBeGreaterThan(-1)
    const effect = text.slice(start, text.indexOf('})', start))
    expect(effect.indexOf('setDeferredSends([])')).toBeGreaterThan(-1)
    expect(
      effect.indexOf('setDeferredSends([])') <
        effect.indexOf('void sendContent'),
      'clearing after sending re-runs this effect over the same text and sends it twice',
    ).toBe(true)
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
