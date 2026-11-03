import { expect, test, type Page } from '@playwright/test'

/**
 * Run-cost meter in the agent console (WS27-S4), end to end.
 *
 * The meter climbs with each `usage_progress` frame, ends on the `usage` frame, and states a
 * subscription or an unpriced turn honestly: "Included in the subscription" and "Price unavailable",
 * never a bare $0. The chat stream is answered by this spec (`page.route`), so no model is called.
 *
 * The stack is needed only to sign in and to serve the SPA and the console. Run it under the `chat`
 * project against a second dev server, as the other chat specs do:
 *
 *   LOCAL_E2E_BASE_URL=http://localhost:5174 pnpm exec playwright test --project=chat tests/e2e/chat-cost-meter.spec.ts
 *
 * The spec sets the English locale and asserts the English copy.
 */

const CONSOLE = '/agents/runs'
const STREAM_PATH = '/api/v1/chat/stream'
const METER = '.verevon-run-trust .verevon-run-cost-meter'

type Turn = {
  progress: Record<string, unknown>[]
  usage: Record<string, unknown>
}

/** One server-sent event: `event:` names the frame, `data:` carries its JSON payload. */
function frame(event: string, data: Record<string, unknown>): string {
  return `event: ${event}\ndata: ${JSON.stringify(data)}\n\n`
}

function streamBody({ progress, usage }: Turn): string {
  return [
    frame('connected', {
      ok: true,
      request_id: 'meter-req-1',
      thread_id: 'meter-thread-1',
      model_used: 'balanced',
      run_id: 'run_meter_1',
    }),
    ...progress.map((data) => frame('usage_progress', data)),
    frame('chunk', { delta: 'Svaret.' }),
    frame('usage', usage),
    frame('done', { request_id: 'meter-req-1', done: true, model_used: 'balanced' }),
  ].join('')
}

/** Answers the console's chat stream with the turn, then starts a run from the console. */
async function runTurn(page: Page, turn: Turn) {
  await page.route(`**${STREAM_PATH}`, (route) => {
    if (route.request().method() !== 'POST') return route.continue()
    return route.fulfill({ status: 200, contentType: 'text/event-stream', body: streamBody(turn) })
  })
  await page.addInitScript(() => window.localStorage.setItem('verevon.locale', 'en'))
  await page.goto(CONSOLE)
  await page.getByLabel('What should the agent do?', { exact: true }).fill('Summarise the report')
  await page.getByRole('button', { name: 'Run task', exact: true }).click()
}

test.describe('run-cost meter in the agent console', () => {
  test('climbs with each provider call and ends on the usage frame', async ({ page }) => {
    await runTurn(page, {
      progress: [
        { calls: 1, phase: 'tool_round', input_tokens: 900, output_tokens: 40, cost_usd: 0.002, cost_basis: 'metered' },
        {
          calls: 2,
          phase: 'tool_round',
          input_tokens: 1800,
          output_tokens: 95,
          cache_read_tokens: 600,
          cost_usd: 0.0046,
          cost_basis: 'metered',
        },
        {
          calls: 3,
          phase: 'answer',
          input_tokens: 2600,
          output_tokens: 180,
          cache_read_tokens: 600,
          cost_usd: 0.0071,
          cost_basis: 'metered',
        },
      ],
      usage: {
        input_tokens: 2600,
        output_tokens: 180,
        cost_usd: 0.0071,
        cost_basis: 'metered',
        latency_ms: 1234,
        calls: 3,
        unpriced_calls: 0,
        cache_read_tokens: 600,
        cost_by_category: { input_usd: 0.004, cache_read_usd: 0.0001, cache_write_usd: 0, output_usd: 0.003 },
      },
    })

    const meter = page.locator(METER)
    await expect(meter).toContainText('3 calls')
    await expect(meter).toContainText('$0.0071')
    await expect(meter).toContainText('Last call: Answer')
    await expect(meter).toContainText('23% of input from cache')
    await expect(meter.getByText('Cost by category')).toBeVisible()
    await expect(meter.locator('dd')).toHaveCount(4)
  })

  test('a subscription turn is included in the subscription and never shows $0', async ({ page }) => {
    await runTurn(page, {
      progress: [{ calls: 1, phase: 'answer', input_tokens: 500, output_tokens: 50, cost_basis: 'subscription' }],
      // The gateway's wire placeholder on the final frame; the meter must not read it as a price.
      usage: {
        input_tokens: 500,
        output_tokens: 50,
        cost_usd: 0,
        cost_basis: 'subscription',
        latency_ms: 800,
        calls: 1,
        unpriced_calls: 0,
      },
    })

    const meter = page.locator(METER)
    await expect(meter).toContainText('Included in the subscription')
    await expect(meter).not.toContainText('$')
  })

  test('an unpriced turn shows its tokens and says the price is unavailable', async ({ page }) => {
    await runTurn(page, {
      progress: [{ calls: 1, phase: 'answer', input_tokens: 900, output_tokens: 60, cost_basis: 'metered', unpriced_calls: 1 }],
      usage: {
        input_tokens: 900,
        output_tokens: 60,
        cost_usd: null,
        cost_basis: 'metered',
        latency_ms: 700,
        calls: 1,
        unpriced_calls: 1,
      },
    })

    const meter = page.locator(METER)
    await expect(meter).toContainText('900 in · 60 out')
    await expect(meter).toContainText('Price unavailable')
    await expect(meter).not.toContainText('$')
  })

  test('a ceiling warning stays shown once a frame has stated it', async ({ page }) => {
    await runTurn(page, {
      progress: [
        {
          calls: 1,
          phase: 'tool_round',
          input_tokens: 800,
          output_tokens: 40,
          cost_usd: 0.0085,
          cost_basis: 'metered',
          ceiling_usd: 0.01,
          fraction: 0.85,
          warning: 'run_ceiling_80',
        },
        {
          calls: 2,
          phase: 'answer',
          input_tokens: 900,
          output_tokens: 60,
          cost_usd: 0.0092,
          cost_basis: 'metered',
          ceiling_usd: 0.01,
          fraction: 0.92,
        },
      ],
      usage: {
        input_tokens: 900,
        output_tokens: 60,
        cost_usd: 0.0092,
        cost_basis: 'metered',
        latency_ms: 900,
        calls: 2,
        unpriced_calls: 0,
      },
    })

    const meter = page.locator(METER)
    await expect(meter.locator('[data-warning]')).toHaveAttribute('data-warning', 'run_ceiling_80')
    await expect(meter).toContainText('This run has used over 80% of its limit.')
    await expect(meter.getByRole('progressbar')).toHaveAttribute('aria-valuenow', '92')
  })
})
