import { expect, test, type Page, type Route } from '@playwright/test'

/**
 * "Send with my edits" on a paused mail send (EP-10-S6), in the browser against a mocked API.
 *
 * The chat stream, the run's approvals, the draft preview, the decision and the resume are all
 * answered by this spec (`page.route`), so no model is called and nothing is sent. What is checked:
 *
 *  - edit mode appears only when the preview says `can_edit`;
 *  - the decision body carries exactly `{decision, reason, edit: {patch, baseFingerprint}}`, the
 *    patch holding only the changed fields, and never a standing scope;
 *  - a 409 `approval_changed` refetches the draft and keeps the person's text as "Your version";
 *  - the card works at 1280 and 375 px.
 *
 * The full-email disclosure is behind the SPA switch, so run it against a second dev server started
 * with `VITE_APPROVAL_WRITE_PREVIEW=true` (5173 is the main checkout):
 *
 *   E2E_APPROVAL_EDIT=1 LOCAL_E2E_BASE_URL=http://localhost:5174 pnpm exec playwright test --project=chat tests/e2e/approval-edit.spec.ts
 */

const ENABLED = process.env.E2E_APPROVAL_EDIT === '1'
const STREAM_PATH = '/api/v1/chat/stream'
const RUN_ID = 'run_edit_1'
const APPROVAL_ID = 'appr_edit_1'
const COMPOSER = 'form textarea'

const APPROVAL = {
  id: APPROVAL_ID,
  run_id: RUN_ID,
  kind: 'APPROVAL_KIND_TOOL_CALL',
  state: 'APPROVAL_STATE_REQUESTED',
  cause: {
    code: 'provider_write',
    tool: 'execute_provider_action',
    scope_options: ['once', 'thread'],
    step_up_required: false,
  },
  reason: "send an email to kari@example.com with the subject 'Status'; it opens 'Hei Kari'",
}

const PREVIEW = {
  kind: 'mail_send',
  provider: 'google',
  operation: 'gmail.send',
  connection_id: 'conn-1',
  to: ['kari@example.com'],
  subject: 'Status',
  body_text: 'Hei Kari,\n\nHer er statusen for uke 42.\n\nHilsen',
  has_html: false,
  is_reply: false,
  raw_only: false,
  editable_fields: ['to', 'subject', 'body_text'],
  action_fingerprint: 'f'.repeat(64),
  revision: 0,
  format: 'plain_text',
  can_edit: true,
}

/** One server-sent event: `event:` names the frame, `data:` carries its JSON payload. */
function frame(event: string, data: Record<string, unknown>): string {
  return `event: ${event}\ndata: ${JSON.stringify(data)}\n\n`
}

/** A turn that drafts a mail and pauses its run for the person's approval. */
function pausedTurn(): string {
  return [
    frame('connected', { ok: true, request_id: 'edit-req-1', thread_id: 'edit-thread-1', run_id: RUN_ID }),
    frame('chunk', { delta: 'Jeg har skrevet e-posten. ' }),
    frame('step_update', { id: RUN_ID, status: 'paused', title: 'Approval' }),
    frame('done', { request_id: 'edit-req-1', done: true, model_used: 'balanced' }),
  ].join('')
}

const json = (route: Route, status: number, body: unknown) =>
  route.fulfill({ status, contentType: 'application/json', body: JSON.stringify(body) })

type Mocks = {
  previews: Record<string, unknown>[]
  decideStatus: number[]
  decisions: unknown[]
}

/** Answers every endpoint the card touches. Each preview read and each decision takes the next answer. */
async function mockApi(page: Page, mocks: Mocks) {
  let previewReads = 0
  let decides = 0
  await page.route(`**${STREAM_PATH}`, (route) => {
    if (route.request().method() !== 'POST') return route.continue()
    return route.fulfill({ status: 200, contentType: 'text/event-stream', body: pausedTurn() })
  })
  await page.route(`**/api/v1/orchestration/runs/${RUN_ID}/approvals`, (route) =>
    json(route, 200, { data: { approvals: decides > 0 && mocks.decideStatus[decides - 1] === 200 ? [] : [APPROVAL] } }),
  )
  await page.route(`**/api/v1/orchestration/approvals/${APPROVAL_ID}/preview`, (route) => {
    const preview = mocks.previews[Math.min(previewReads, mocks.previews.length - 1)]
    previewReads += 1
    return json(route, 200, { data: { preview } })
  })
  await page.route(`**/api/v1/orchestration/approvals/${APPROVAL_ID}/decide`, (route) => {
    mocks.decisions.push(route.request().postDataJSON())
    const status = mocks.decideStatus[Math.min(decides, mocks.decideStatus.length - 1)] ?? 200
    decides += 1
    if (status === 409) {
      return json(route, 409, {
        error: {
          code: 'approval_changed',
          message: 'This approval changed since you opened it. Review it again.',
          details: {},
        },
      })
    }
    return json(route, 200, { data: { approval: { ...APPROVAL, state: 'APPROVAL_STATE_GRANTED' } } })
  })
  await page.route(`**/api/v1/orchestration/runs/${RUN_ID}/resume`, (route) => json(route, 200, { data: {} }))
}

async function pauseOnDraft(page: Page) {
  await page.addInitScript(() => window.localStorage.setItem('verevon.locale', 'en'))
  await page.goto('/chat')
  const composer = page.locator(COMPOSER)
  await expect(composer).toBeVisible()
  await composer.fill('Send Kari the status for week 42')
  await page.locator('form button[type="submit"]').click()
  const card = page
    .locator('.verevon-chat-approval')
    .filter({ has: page.locator('.verevon-chat-approval__disclosure') })
  await expect(card).toBeVisible()
  await card.getByRole('button', { name: 'Show the full email' }).click()
  await expect(card.getByText('Her er statusen for uke 42.', { exact: false })).toBeVisible()
  return card
}

test.describe('editable approval card (EP-10-S6)', () => {
  test.skip(!ENABLED, 'E2E_APPROVAL_EDIT=1 and a dev server with VITE_APPROVAL_WRITE_PREVIEW=true are needed')

  for (const width of [1280, 375]) {
    test(`sends only the changed fields as a once approval at ${width} px`, async ({ page }) => {
      await page.setViewportSize({ width, height: 900 })
      const mocks: Mocks = { previews: [PREVIEW], decideStatus: [200], decisions: [] }
      await mockApi(page, mocks)
      const card = await pauseOnDraft(page)

      await card.getByRole('button', { name: 'Edit before sending' }).click()
      await card.getByLabel(/Subject/).fill('Status for week 42')
      await expect(card.getByRole('button', { name: 'Approve for this thread' })).toHaveCount(0)
      // Evidence for the slice: the edit state at this width.
      await page.screenshot({ path: test.info().outputPath(`approval-edit-${width}.png`) })

      await card.getByRole('button', { name: 'Send with my edits' }).click()
      await expect.poll(() => mocks.decisions.length).toBe(1)
      expect(mocks.decisions[0]).toEqual({
        decision: 'approve',
        reason: '',
        edit: { patch: { subject: 'Status for week 42' }, baseFingerprint: 'f'.repeat(64) },
      })
      await expect(card).toHaveCount(0)
    })
  }

  test('shows no edit mode when the preview does not say can_edit', async ({ page }) => {
    const mocks: Mocks = { previews: [{ ...PREVIEW, can_edit: false }], decideStatus: [200], decisions: [] }
    await mockApi(page, mocks)
    const card = await pauseOnDraft(page)

    await expect(card.getByRole('button', { name: 'Edit before sending' })).toHaveCount(0)
    await card.getByRole('button', { name: 'Approve', exact: true }).click()
    await expect.poll(() => mocks.decisions.length).toBe(1)
    expect(mocks.decisions[0]).toEqual({ decision: 'approve', reason: '' })
  })

  test("a changed draft refetches the preview and keeps the person's text", async ({ page }) => {
    const changed = { ...PREVIEW, subject: 'Status (revised by the agent)', action_fingerprint: 'e'.repeat(64) }
    const mocks: Mocks = { previews: [PREVIEW, changed], decideStatus: [409, 200], decisions: [] }
    await mockApi(page, mocks)
    const card = await pauseOnDraft(page)

    await card.getByRole('button', { name: 'Edit before sending' }).click()
    await card.getByLabel(/Subject/).fill('My subject')
    await card.getByRole('button', { name: 'Send with my edits' }).click()

    await expect(card.getByText('Your version')).toBeVisible()
    await expect(card.getByText('My subject')).toBeVisible()
    await expect(card.getByLabel(/Subject/)).toHaveValue('Status (revised by the agent)')
    await expect(card.getByRole('alert')).toContainText('The draft changed since you opened it')

    // A second edit is made against the new draft and names its fingerprint.
    await card.getByLabel(/Subject/).fill('My subject again')
    await card.getByRole('button', { name: 'Send with my edits' }).click()
    await expect.poll(() => mocks.decisions.length).toBe(2)
    expect(mocks.decisions[1]).toEqual({
      decision: 'approve',
      reason: '',
      edit: { patch: { subject: 'My subject again' }, baseFingerprint: 'e'.repeat(64) },
    })
  })
})
