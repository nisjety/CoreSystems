import { expect, test, type Locator } from '@playwright/test'

type Envelope<T> = { data?: T }

type FeedbackConversation = {
  id: string
}

type TicketClassification = {
  id: string
  ticket?: { id: string }
}

type DraftReplyProposal = {
  id: string
  conversation_id: string
  kind: string
  status: string
  payload: { body_text?: string }
}

type SupportTicket = {
  id: string
  status: string
  work_type?: string
  severity: string
  category: string
  priority: string
  intent: string
  due_at?: string | null
  follow_up_at?: string | null
  linked_resources?: Array<{
    resource_kind: string
    resource_id?: string
    link_type?: string
    linked_ticket?: { id: string; ticket_key: string; status: string; work_type: string }
  }>
  checklists?: Array<{ id: string; name: string }>
}

function unwrap<T>(payload: Envelope<T> | T): T {
  if (payload && typeof payload === 'object' && 'data' in payload) {
    return (payload as Envelope<T>).data as T
  }
  return payload as T
}

test('Ticketing: a browser-created work-type rule routes new incidents through the canonical service', async ({ page, baseURL }) => {
  const stamp = `${Date.now()}-${Math.random().toString(16).slice(2)}`
  const origin = baseURL ?? 'http://localhost:5199'
  const ruleName = `E2E incident routing ${stamp}`
  const feedbackText = `E2E incident routing ${stamp}`

  await page.goto('/support?surface=tickets&queue=rules')
  await page.getByRole('textbox', { name: /regelnavn|rule name/i }).fill(ruleName)
  await page.getByRole('combobox', { name: /regelbetingelse|rule condition/i }).selectOption('work_type')
  await page.getByRole('combobox', { name: /arbeidstype|work type/i }).selectOption('incident')
  await page.getByRole('combobox', { name: /regelhandling|rule action/i }).selectOption('priority')
  await page.getByRole('textbox', { name: /handlingsverdi|action value/i }).fill('urgent')

  const createdRule = page.waitForResponse((response) =>
    response.url().includes('/api/v1/ticket-automation-rules')
      && response.request().method() === 'POST',
  )
  await page.getByRole('button', { name: /opprett regel|create rule/i }).click()
  expect((await createdRule).status()).toBe(201)

  const feedback = await page.request.post('/api/v1/inbox/feedback', {
    headers: { 'content-type': 'application/json', origin },
    data: {
      body_text: feedbackText,
      from_name: 'Verevon E2E',
      from_email: 'e2e@verevon.dev',
      page_url: '/support?surface=tickets',
      idempotency_key: `work-type-rule-${stamp}`,
    },
  })
  expect(feedback.status()).toBe(201)
  const conversation = unwrap<FeedbackConversation>(await feedback.json())

  const classified = await page.request.post(
    `/api/v1/tickets/conversations/${encodeURIComponent(conversation.id)}/classifications`,
    {
      headers: { 'content-type': 'application/json', origin },
      data: {
        outcome: 'auto_ticket', confidence: 0.99, reason: 'E2E incident-routing fixture.',
        suggested_fields: { work_type: 'incident', priority: 'normal', severity: 'high', category: 'operations' },
        evidence_message_ids: [],
      },
    },
  )
  expect(classified.status()).toBe(201)
  const ticketID = unwrap<TicketClassification>(await classified.json()).ticket?.id
  expect(ticketID).toBeTruthy()

  await expect.poll(async () => {
    const ticketResponse = await page.request.get(`/api/v1/tickets/${encodeURIComponent(ticketID!)}`)
    if (ticketResponse.status() !== 200) return null
    return unwrap<SupportTicket>(await ticketResponse.json())
  }, { timeout: 20_000 }).toMatchObject({ id: ticketID, work_type: 'incident', priority: 'urgent' })
})

test('Inbox: human-reviewed AI ticket promotion preserves edits and verifies canonical state', async ({ page, baseURL }) => {
  const stamp = `${Date.now()}-${Math.random().toString(16).slice(2)}`
  const feedbackText = `E2E AI ticket ${stamp}`
  const origin = baseURL ?? 'http://localhost:5199'

  // Fixture setup uses the live, authenticated feedback ingress: it creates a
  // real org-scoped conversation without relying on a provider sandbox.
  const feedback = await page.request.post('/api/v1/inbox/feedback', {
    headers: { 'content-type': 'application/json', origin },
    data: {
      body_text: feedbackText,
      from_name: 'Verevon E2E',
      from_email: 'e2e@verevon.dev',
      page_url: '/inbox?view=all',
      idempotency_key: `inbox-ticketing-${stamp}`,
    },
  })
  expect(feedback.status()).toBe(201)
  const conversation = unwrap<FeedbackConversation>(await feedback.json())
  expect(conversation.id).toBeTruthy()

  // Create a real, review-gated proposal. The operator interaction below is
  // deliberately performed in the browser; setup only supplies the pending
  // work that a model would normally submit.
  const classified = await page.request.post(
    `/api/v1/tickets/conversations/${encodeURIComponent(conversation.id)}/classifications`,
    {
      headers: { 'content-type': 'application/json', origin },
      data: {
        outcome: 'suggest_ticket',
        confidence: 0.94,
        reason: 'E2E evidence-backed support case requiring a customer follow-up.',
        suggested_fields: {
          category: 'delivery',
          intent: 'customer_follow_up',
          work_type: 'incident',
          priority: 'high',
          severity: 'high',
        },
        evidence_message_ids: [],
      },
    },
  )
  expect(classified.status()).toBe(201)
  const classification = unwrap<TicketClassification>(await classified.json())
  expect(classification.id).toBeTruthy()
  expect(classification.ticket?.id).toBeTruthy()
  const ticketId = classification.ticket!.id

  // Classification and its corresponding review-row travel through an
  // asynchronous boundary. Establish the canonical ledger precondition before
  // asking the browser to render it, rather than relying on arbitrary UI waits.
  await expect.poll(async () => {
    const pendingActions = await page.request.get(
      `/api/v1/inbox/ai-actions?conversation_id=${encodeURIComponent(conversation.id)}&status=all&limit=50`,
    )
    if (pendingActions.status() !== 200) return false
    const ledger = unwrap<Array<{ id: string; status: string }>>(await pendingActions.json())
    return ledger.some((action) => action.id === classification.id && action.status === 'suggest_ticket')
  }, { timeout: 20_000 }).toBe(true)

  await page.goto('/inbox?view=all')
  const conversationRow = page.getByRole('button', { name: `Feedback: ${feedbackText}` })
  await expect(conversationRow).toBeVisible()
  const reviewLedgerResponse = page.waitForResponse((response) =>
    response.url().includes('/api/v1/inbox/ai-actions?')
      && response.url().includes(`conversation_id=${encodeURIComponent(conversation.id)}`)
      && response.request().method() === 'GET',
  )
  await conversationRow.click()
  await page.locator('.verevon-inbox-center-tabs').getByRole('tab', { name: /sak|ticket/i }).click()
  const reviewLedger = await reviewLedgerResponse
  expect(reviewLedger.status()).toBe(200)
  await reviewLedger.finished()
  expect(unwrap<Array<{ id: string; status: string }>>(await reviewLedger.json())).toContainEqual(
    expect.objectContaining({ id: classification.id, status: 'suggest_ticket' }),
  )

  // Ticket-detail hydration can reset the center panel after the ledger read.
  // Select it again before interacting with the loaded, reviewable proposal.
  await page.locator('.verevon-inbox-center-tabs').getByRole('tab', { name: /sak|ticket/i }).click()
  const severity = page.getByRole('textbox', { name: /severity|alvorlighetsgrad/i })
  await expect(severity).toHaveValue('high')
  await severity.fill('critical')

  const approval = page.waitForResponse((response) =>
    response.url().includes(`/api/v1/inbox/ai-actions/${classification.id}/approve`)
      && response.request().method() === 'POST',
  )
  await page.getByRole('button', { name: /approve|godkjenn/i }).click()
  expect((await approval).status()).toBe(200)

  // Approval is not falsely presented as completion. The eventual ticket is
  // checked directly from its canonical owner, including the human's edit.
  await expect(page.locator('#conversation-center-panel-ticket').getByRole('status')).toContainText(/waiting for verified execution|venter på verifisert utførelse|execution verified|utførelse verifisert/i)
  await expect.poll(async () => {
    const ticketResponse = await page.request.get(`/api/v1/tickets/${encodeURIComponent(ticketId)}`)
    if (ticketResponse.status() !== 200) return null
    return unwrap<SupportTicket>(await ticketResponse.json())
  }, { timeout: 20_000 }).toMatchObject({
    id: ticketId,
    status: 'open',
    category: 'delivery',
    work_type: 'incident',
    priority: 'high',
    severity: 'critical',
    intent: 'customer_follow_up',
  })

  // The receipt is durable UI state, not a transient success toast: re-open
  // the conversation after the asynchronous executor finishes and require the
  // exact action's canonical execution receipt.
  await page.reload()
  const reloadedConversationRow = page.getByRole('button', { name: `Feedback: ${feedbackText}` })
  await expect(reloadedConversationRow).toBeVisible()
  const reloadedLedgerResponse = page.waitForResponse((response) =>
    response.url().includes('/api/v1/inbox/ai-actions?')
      && response.url().includes(`conversation_id=${encodeURIComponent(conversation.id)}`)
      && response.request().method() === 'GET',
  )
  await reloadedConversationRow.click()
  await page.locator('.verevon-inbox-center-tabs').getByRole('tab', { name: /sak|ticket/i }).click()
  await reloadedLedgerResponse
  await page.locator('.verevon-inbox-center-tabs').getByRole('tab', { name: /sak|ticket/i }).click()
  await expect(page.locator(`[data-ai-action-id="${classification.id}"]`)).toContainText(
    /execution verified|utførelse verifisert/i,
  )
})

test('Inbox: durable AI reply proposal shows exact text before an operator approves it', async ({ page, baseURL }) => {
  const stamp = `${Date.now()}-${Math.random().toString(16).slice(2)}`
  const feedbackText = `E2E AI reply ${stamp}`
  const proposalText = `We have verified your request ${stamp} and will update you tomorrow.`
  const reviewedText = `Human-reviewed: we have verified your request ${stamp} and will update you tomorrow.`
  const origin = baseURL ?? 'http://localhost:5199'

  const feedback = await page.request.post('/api/v1/inbox/feedback', {
    headers: { 'content-type': 'application/json', origin },
    data: {
      body_text: feedbackText,
      from_name: 'Verevon E2E',
      from_email: 'e2e@verevon.dev',
      page_url: '/inbox?view=all',
      idempotency_key: `inbox-ai-reply-${stamp}`,
    },
  })
  expect(feedback.status()).toBe(201)
  const conversation = unwrap<FeedbackConversation>(await feedback.json())

  // The browser-facing endpoint admits only a body and conversation id. It
  // derives the org and actor from the authenticated session and fixes the
  // executable kind to draft.reply; no browser request can self-approve it.
  const created = await page.request.post('/api/v1/inbox/ai-actions', {
    headers: { 'content-type': 'application/json', origin },
    data: { conversation_id: conversation.id, body_text: proposalText },
  })
  expect(created.status()).toBe(201)
  const proposal = unwrap<DraftReplyProposal>(await created.json())
  expect(proposal).toMatchObject({
    conversation_id: conversation.id,
    kind: 'draft.reply',
    status: 'suggested',
    payload: { body_text: proposalText },
  })

  await page.goto('/inbox?view=all')
  const conversationRow = page.getByRole('button', { name: `Feedback: ${feedbackText}` })
  await expect(conversationRow).toBeVisible()
  const reviewLedgerResponse = page.waitForResponse((response) =>
    response.url().includes('/api/v1/inbox/ai-actions?')
      && response.url().includes(`conversation_id=${encodeURIComponent(conversation.id)}`)
      && response.request().method() === 'GET',
  )
  await conversationRow.click()
  await page.locator('.verevon-inbox-center-tabs').getByRole('tab', { name: /sak|ticket/i }).click()
  const reviewLedger = await reviewLedgerResponse
  expect(reviewLedger.status()).toBe(200)
  await reviewLedger.finished()
  expect(unwrap<Array<{ id: string; status: string }>>(await reviewLedger.json())).toContainEqual(
    expect.objectContaining({ id: proposal.id, status: 'suggested' }),
  )

  // The approval surface renders the exact executor payload in an editor; it
  // does not summarize or hide the outbound content behind a generic label.
  const replyEditor = page.getByRole('textbox', { name: /reply draft.*editable|svarutkast.*redigerbar/i })
  await expect(replyEditor).toHaveValue(proposalText)
  await replyEditor.fill(reviewedText)
  const approval = page.waitForResponse((response) =>
    response.url().includes(`/api/v1/inbox/ai-actions/${proposal.id}/approve`)
      && response.request().method() === 'POST',
  )
  await page.getByRole('button', { name: /approve|godkjenn/i }).click()
  const approvalResponse = await approval
  expect(approvalResponse.status()).toBe(200)
  expect(approvalResponse.request().postDataJSON()).toEqual({ edited_fields: { body_text: reviewedText } })

  // This feedback fixture intentionally has no provider send target. Approval
  // is therefore durable, but no send receipt is claimed by the test or UI.
  await expect.poll(async () => {
    const ledger = await page.request.get(
      `/api/v1/inbox/ai-actions?conversation_id=${encodeURIComponent(conversation.id)}&status=all&limit=50`,
    )
    if (ledger.status() !== 200) return null
    return unwrap<DraftReplyProposal[]>(await ledger.json()).find((action) => action.id === proposal.id)
  }, { timeout: 20_000 }).toMatchObject({ status: 'approved', payload: { body_text: reviewedText } })
})

test('Inbox: an empty provider delivery ledger is available and does not imply delivery', async ({ page, baseURL }) => {
  const stamp = `${Date.now()}-${Math.random().toString(16).slice(2)}`
  const feedbackText = `E2E delivery ledger ${stamp}`
  const origin = baseURL ?? 'http://localhost:5199'

  const feedback = await page.request.post('/api/v1/inbox/feedback', {
    headers: { 'content-type': 'application/json', origin },
    data: {
      body_text: feedbackText,
      from_name: 'Verevon E2E',
      from_email: 'e2e@verevon.dev',
      page_url: '/inbox?view=all',
      idempotency_key: `inbox-delivery-ledger-${stamp}`,
    },
  })
  expect(feedback.status()).toBe(201)
  const conversation = unwrap<FeedbackConversation>(await feedback.json())

  // A conversation without a provider submission has no receipt rows, but its
  // canonical delivery ledger must still be readable. An empty ledger is not a
  // delivery result and must not be surfaced as an operational error.
  const ledger = await page.request.get(
    `/api/v1/inbox/conversations/${encodeURIComponent(conversation.id)}/outbound-intents`,
  )
  expect(ledger.status()).toBe(200)
  expect(unwrap<unknown[]>(await ledger.json())).toEqual([])

  await page.goto('/inbox?view=all')
  const row = page.getByRole('button', { name: `Feedback: ${feedbackText}` })
  await expect(row).toBeVisible()
  const ledgerResponse = page.waitForResponse((response) =>
    response.url().includes(`/api/v1/inbox/conversations/${encodeURIComponent(conversation.id)}/outbound-intents`)
      && response.request().method() === 'GET',
  )
  await row.click()
  await page.getByRole('tab', { name: /aktivitet|activity/i }).first().click()
  expect((await ledgerResponse).status()).toBe(200)
  await expect(page.getByText(/delivery outcomes could not be loaded/i)).toHaveCount(0)
})

test('Inbox: a conversation can be attached to one existing ticket through the audited action boundary', async ({ page, baseURL }) => {
  const stamp = `${Date.now()}-${Math.random().toString(16).slice(2)}`
  const origin = baseURL ?? 'http://localhost:5199'
  const sourceText = `E2E attach source ${stamp}`
  const targetText = `E2E attach target ${stamp}`

  async function createFeedback(bodyText: string, key: string) {
    const response = await page.request.post('/api/v1/inbox/feedback', {
      headers: { 'content-type': 'application/json', origin },
      data: { body_text: bodyText, from_name: 'Verevon E2E', from_email: 'e2e@verevon.dev', page_url: '/inbox?view=all', idempotency_key: key },
    })
    expect(response.status()).toBe(201)
    return unwrap<FeedbackConversation>(await response.json())
  }

  const [source, target] = await Promise.all([
    createFeedback(sourceText, `inbox-attach-source-${stamp}`),
    createFeedback(targetText, `inbox-attach-target-${stamp}`),
  ])
  const classified = await page.request.post(`/api/v1/tickets/conversations/${encodeURIComponent(target.id)}/classifications`, {
    headers: { 'content-type': 'application/json', origin },
    data: { outcome: 'auto_ticket', confidence: 0.99, reason: 'E2E target case for a conversation attachment.', suggested_fields: { category: 'delivery', intent: 'customer_follow_up', priority: 'high', severity: 'medium' }, evidence_message_ids: [] },
  })
  expect(classified.status()).toBe(201)
  const targetTicket = unwrap<TicketClassification & { ticket: { id: string; ticket_key: string } }>(await classified.json()).ticket
  expect(targetTicket?.id).toBeTruthy()

  await page.goto('/inbox?view=all')
  await page.getByRole('button', { name: `Feedback: ${sourceText}` }).click()
  await page.getByRole('button', { name: /link to existing ticket|koble til eksisterende sak/i }).click()
  const selector = page.getByRole('combobox', { name: /select existing ticket|velg eksisterende sak/i })
  await selector.selectOption(targetTicket.id)
  const attachment = page.waitForResponse((response) =>
    response.url().includes('/api/v1/actions/execute') && response.request().method() === 'POST',
  )
  await page.getByRole('button', { name: /attach conversation|knytt samtale/i }).click()
  expect((await attachment).status()).toBe(200)

  await expect.poll(async () => {
    const response = await page.request.get(`/api/v1/tickets/${encodeURIComponent(targetTicket.id)}`)
    if (response.status() !== 200) return false
    return unwrap<SupportTicket>(await response.json()).linked_resources?.some((link) =>
      link.resource_kind === 'conversation_source' && link.resource_id === source.id,
    ) ?? false
  }, { timeout: 20_000 }).toBe(true)
})

test('Inbox: independently reviewable resolution-plan proposals stay correlated after reload', async ({ page, baseURL }) => {
  const stamp = `${Date.now()}-${Math.random().toString(16).slice(2)}`
  const origin = baseURL ?? 'http://localhost:5199'
  const feedbackText = `E2E resolution plan ${stamp}`
  const proposalGroupId = `resolution-${stamp}`
  const replyText = `We are checking the carrier exception for ${stamp}.`
  const noteText = `Review the carrier exception before confirming a new delivery date (${stamp}).`

  const feedback = await page.request.post('/api/v1/inbox/feedback', {
    headers: { 'content-type': 'application/json', origin },
    data: {
      body_text: feedbackText,
      from_name: 'Verevon E2E',
      from_email: 'e2e@verevon.dev',
      page_url: '/inbox?view=all',
      idempotency_key: `inbox-resolution-plan-${stamp}`,
    },
  })
  expect(feedback.status()).toBe(201)
  const conversation = unwrap<FeedbackConversation>(await feedback.json())

  // Use the canonical automatic-ticket fixture solely to create an existing
  // work object. The three reviewed proposals below still arrive through the
  // browser-facing, allow-listed Inbox proposal boundary.
  const classified = await page.request.post(
    `/api/v1/tickets/conversations/${encodeURIComponent(conversation.id)}/classifications`,
    {
      headers: { 'content-type': 'application/json', origin },
      data: {
        outcome: 'auto_ticket',
        confidence: 0.99,
        reason: 'E2E resolution-plan fixture requires an existing support ticket.',
        suggested_fields: { category: 'delivery', intent: 'missing_delivery', priority: 'high', severity: 'medium' },
        evidence_message_ids: [],
      },
    },
  )
  expect(classified.status()).toBe(201)
  const ticketId = unwrap<TicketClassification>(await classified.json()).ticket?.id
  expect(ticketId).toBeTruthy()

  const reply = await page.request.post('/api/v1/inbox/ai-actions', {
    headers: { 'content-type': 'application/json', origin },
    data: { conversation_id: conversation.id, body_text: replyText, proposal_group_id: proposalGroupId },
  })
  expect(reply.status()).toBe(201)
  const internalNote = await page.request.post('/api/v1/inbox/ai-actions', {
    headers: { 'content-type': 'application/json', origin },
    data: { conversation_id: conversation.id, body_text: noteText, proposal_group_id: proposalGroupId, kind: 'internal.note' },
  })
  if (internalNote.status() !== 201) {
    throw new Error(`Resolution-plan internal note was rejected (${internalNote.status()}): ${await internalNote.text()}`)
  }
  expect(internalNote.status()).toBe(201)
  const ticketUpdate = await page.request.post('/api/v1/inbox/ai-actions', {
    headers: { 'content-type': 'application/json', origin },
    data: {
      conversation_id: conversation.id,
      ticket_id: ticketId,
      proposal_group_id: proposalGroupId,
      kind: 'ticket.update',
      confidence: 0.86,
      reason: 'The carrier exception warrants a higher-priority human review.',
      evidence_message_ids: [],
      suggested_fields: { category: 'delivery', intent: 'missing_delivery', priority: 'high', severity: 'medium' },
    },
  })
  if (ticketUpdate.status() !== 201) {
    throw new Error(`Resolution-plan ticket update was rejected (${ticketUpdate.status()}): ${await ticketUpdate.text()}`)
  }
  expect(ticketUpdate.status()).toBe(201)

  const replyProposal = unwrap<DraftReplyProposal>(await reply.json())
  const noteProposal = unwrap<DraftReplyProposal>(await internalNote.json())
  const ticketUpdateProposal = unwrap<DraftReplyProposal>(await ticketUpdate.json())
  const proposalIds = [replyProposal.id, noteProposal.id, ticketUpdateProposal.id]
  await expect.poll(async () => {
    const ledger = await page.request.get(
      `/api/v1/inbox/ai-actions?conversation_id=${encodeURIComponent(conversation.id)}&status=all&limit=50`,
    )
    if (ledger.status() !== 200) return []
    return unwrap<Array<DraftReplyProposal & { proposal_group_id?: string }>>(await ledger.json())
      .filter((action) => proposalIds.includes(action.id))
      .map((action) => action.proposal_group_id)
  }, { timeout: 20_000 }).toEqual([proposalGroupId, proposalGroupId, proposalGroupId])

  async function expectIndependentGroupedDecisions(panel: Locator) {
    const plan = panel.getByRole('group', { name: /AI resolution plan|AI-løsningsplan/i })
    await expect(plan).toContainText(/3 independent decisions|3 separate avgjørelser/i)
    await expect(plan).toContainText(/no approve-all action|ingen godkjenn-alt-handling/i)
    for (const proposalId of proposalIds) {
      const item = panel.locator(`[data-ai-action-id="${proposalId}"]`)
      await expect(item).toContainText(/Part of one resolution plan|Del av én løsningsplan/i)
      await expect(item.getByRole('button', { name: /Approve|Godkjenn/i })).toHaveCount(1)
      await expect(item.getByRole('button', { name: /Reject|Avvis/i })).toHaveCount(1)
    }
  }

  await page.goto('/inbox?view=all')
  const detailRead = page.waitForResponse((response) =>
    new URL(response.url()).pathname === `/api/v1/inbox/conversations/${encodeURIComponent(conversation.id)}`
      && response.request().method() === 'GET',
  )
  await page.getByRole('button', { name: `Feedback: ${feedbackText}` }).click()
  await detailRead
  const reviewLedgerRead = page.waitForResponse((response) =>
    response.url().includes('/api/v1/inbox/ai-actions?')
      && response.url().includes(`conversation_id=${encodeURIComponent(conversation.id)}`)
      && response.request().method() === 'GET',
  )
  const centerTicketTab = page.locator('.verevon-inbox-center-tabs').getByRole('tab', { name: /sak|ticket/i })
  await centerTicketTab.click()
  await reviewLedgerRead
  await centerTicketTab.click()
  const reviewPanel = page.getByLabel(/AI suggestions awaiting review|AI-forslag som venter på gjennomgang/i)
  await expectIndependentGroupedDecisions(reviewPanel)
  await expect(
    reviewPanel.locator(`[data-ai-action-id="${replyProposal.id}"]`).getByRole('textbox', { name: /reply draft.*editable|svarutkast.*redigerbar/i }),
  ).toHaveValue(replyText)
  await expect(
    reviewPanel.locator(`[data-ai-action-id="${noteProposal.id}"]`).getByRole('textbox', { name: /internal note.*editable|internt notat.*redigerbar/i }),
  ).toHaveValue(noteText)

  // A reload must preserve the canonical correlation and the three separate
  // decision controls; this test deliberately does not approve anything.
  await page.reload()
  const reloadedDetailRead = page.waitForResponse((response) =>
    new URL(response.url()).pathname === `/api/v1/inbox/conversations/${encodeURIComponent(conversation.id)}`
      && response.request().method() === 'GET',
  )
  await page.getByRole('button', { name: `Feedback: ${feedbackText}` }).click()
  await reloadedDetailRead
  const reloadedReviewLedgerRead = page.waitForResponse((response) =>
    response.url().includes('/api/v1/inbox/ai-actions?')
      && response.url().includes(`conversation_id=${encodeURIComponent(conversation.id)}`)
      && response.request().method() === 'GET',
  )
  const reloadedCenterTicketTab = page.locator('.verevon-inbox-center-tabs').getByRole('tab', { name: /sak|ticket/i })
  await reloadedCenterTicketTab.click()
  await reloadedReviewLedgerRead
  await reloadedCenterTicketTab.click()
  const reloadedReviewPanel = page.getByLabel(/AI suggestions awaiting review|AI-forslag som venter på gjennomgang/i)
  await expectIndependentGroupedDecisions(reloadedReviewPanel)
  await expect(
    reloadedReviewPanel.locator(`[data-ai-action-id="${replyProposal.id}"]`).getByRole('textbox', { name: /reply draft.*editable|svarutkast.*redigerbar/i }),
  ).toHaveValue(replyText)
})

test('Ticketing: a durable saved view applies only its canonical ticket filters', async ({ page, baseURL }) => {
  const stamp = `${Date.now()}-${Math.random().toString(16).slice(2)}`
  const origin = baseURL ?? 'http://localhost:5199'
  const feedbackText = `E2E saved view ${stamp}`
  const feedback = await page.request.post('/api/v1/inbox/feedback', {
    headers: { 'content-type': 'application/json', origin },
    data: {
      body_text: feedbackText,
      from_name: 'Verevon E2E',
      from_email: 'e2e@verevon.dev',
      page_url: '/tickets',
      idempotency_key: `ticket-view-${stamp}`,
    },
  })
  expect(feedback.status()).toBe(201)
  const conversation = unwrap<FeedbackConversation>(await feedback.json())
  const classified = await page.request.post(
    `/api/v1/tickets/conversations/${encodeURIComponent(conversation.id)}/classifications`,
    {
      headers: { 'content-type': 'application/json', origin },
      data: {
        outcome: 'auto_ticket', confidence: 0.97, reason: 'E2E saved-view fixture.',
        suggested_fields: { category: 'delivery', intent: 'customer_follow_up', priority: 'high', severity: 'high' },
        evidence_message_ids: [],
      },
    },
  )
  expect(classified.status()).toBe(201)
  const ticketId = unwrap<TicketClassification>(await classified.json()).ticket?.id
  expect(ticketId).toBeTruthy()
  await expect.poll(async () => {
    const ticket = await page.request.get(`/api/v1/tickets/${encodeURIComponent(ticketId!)}`)
    if (ticket.status() !== 200) return null
    return unwrap<SupportTicket>(await ticket.json()).status
  }, { timeout: 20_000 }).toBe('suggested')

  const name = `Suggested delivery ${stamp}`
  const created = await page.request.post('/api/v1/ticket-views', {
    headers: { 'content-type': 'application/json', origin },
    data: {
      name, scope: 'org', visibility: 'sidebar', filter: { status: 'suggested', ignored_payload: 'must-not-reach-list-api' }, sort: {}, sidebar_order: 0,
    },
  })
  expect(created.status()).toBe(201)
  const view = unwrap<{ id: string }>(await created.json())

  await page.goto('/tickets')
  const selector = page.getByRole('combobox', { name: /saved ticket view|lagret sakvisning/i })
  await expect(selector).toBeVisible()
  await selector.selectOption(view.id)
  await expect(page).toHaveURL(new RegExp(`view=${encodeURIComponent(view.id)}.*status=suggested|status=suggested.*view=${encodeURIComponent(view.id)}`))
  // The saved-view selector legitimately contains the same ticket title in a
  // hidden option. Assert the visible Ticketing queue row, not that option.
  await expect(page.locator('.verevon-ticketing-ticket-row', { hasText: feedbackText })).toBeVisible()
})

test('Ticketing: human links two canonical tickets as an audited dependency', async ({ page, baseURL }) => {
  const stamp = `${Date.now()}-${Math.random().toString(16).slice(2)}`
  const origin = baseURL ?? 'http://localhost:5199'

  async function createAutoTicket(label: string): Promise<string> {
    const feedback = await page.request.post('/api/v1/inbox/feedback', {
      headers: { 'content-type': 'application/json', origin },
      data: {
        body_text: `${label} ${stamp}`,
        from_name: 'Verevon E2E',
        from_email: 'e2e@verevon.dev',
        page_url: '/tickets',
        idempotency_key: `ticket-dependency-${label}-${stamp}`,
      },
    })
    expect(feedback.status()).toBe(201)
    const conversation = unwrap<FeedbackConversation>(await feedback.json())
    const classified = await page.request.post(
      `/api/v1/tickets/conversations/${encodeURIComponent(conversation.id)}/classifications`,
      {
        headers: { 'content-type': 'application/json', origin },
        data: {
          outcome: 'auto_ticket',
          confidence: 0.97,
          reason: 'E2E dependency fixture with evidence-backed operator triage.',
          suggested_fields: {
            category: 'delivery',
            intent: 'internal_work',
            priority: 'high',
            severity: 'high',
          },
          evidence_message_ids: [],
        },
      },
    )
    expect(classified.status()).toBe(201)
    const classification = unwrap<TicketClassification>(await classified.json())
    expect(classification.ticket?.id).toBeTruthy()
    return classification.ticket!.id
  }

  const sourceTicketId = await createAutoTicket('Dependency source')
  const targetTicketId = await createAutoTicket('Dependency target')

  await page.goto(`/tickets?ticketId=${encodeURIComponent(sourceTicketId)}`)
  // This is team work, not an operator-only calendar reminder or an SLA
  // change: the quick action must retain a separate canonical follow-up time.
  const followUpAction = page.waitForResponse((response) =>
    response.url().includes('/api/v1/actions/execute')
      && response.request().method() === 'POST'
      && response.request().postData()?.includes('tickets.update') === true
      && response.request().postData()?.includes('followUpAt') === true,
  )
  await page.getByRole('button', { name: /oppfølging i morgen|follow up tomorrow/i }).click()
  expect((await followUpAction).status()).toBe(200)
  await expect.poll(async () => {
    const source = await page.request.get(`/api/v1/tickets/${encodeURIComponent(sourceTicketId)}`)
    if (source.status() !== 200) return null
    const ticket = unwrap<SupportTicket>(await source.json())
    return ticket.follow_up_at && !ticket.due_at ? ticket.follow_up_at : null
  }, { timeout: 20_000 }).toMatch(/^\d{4}-\d{2}-\d{2}T/)

  await page.getByRole('tab', { name: /relatert|related/i }).click()
  const targetSelector = page.getByRole('combobox', { name: /sak som skal lenkes|ticket to link/i })
  await expect(targetSelector).toBeVisible()
  await targetSelector.selectOption(targetTicketId)
  await page.getByRole('combobox', { name: /type saksforhold|ticket relationship type/i }).selectOption('child')

  const linkAction = page.waitForResponse((response) =>
    response.url().includes('/api/v1/actions/execute')
      && response.request().method() === 'POST'
      && response.status() === 200,
  )
  await page.getByRole('button', { name: /koble sak|link ticket/i }).click()
  expect((await linkAction).status()).toBe(200)

  await expect.poll(async () => {
    const source = await page.request.get(`/api/v1/tickets/${encodeURIComponent(sourceTicketId)}`)
    if (source.status() !== 200) return false
    const ticket = unwrap<SupportTicket>(await source.json())
    return ticket.linked_resources?.some((link) =>
      link.resource_kind === 'ticket'
        && link.resource_id === targetTicketId
        && link.link_type === 'child'
        && link.linked_ticket?.id === targetTicketId
        && link.linked_ticket.status === 'suggested',
    ) ?? false
  }, { timeout: 20_000 }).toBe(true)

  await page.reload()
  await page.getByRole('tab', { name: /relatert|related/i }).click()
  await expect(page.locator('.verevon-ticketing-links')).toContainText(/TCK-[A-F0-9]+.*Underordnet sak.*Foreslått.*Kundesaker/)

  // Checklist work is an independently audited mutation, not a direct UI
  // write. Confirm the action gateway receives it and conversation-core keeps
  // the durable checklist after the browser state is refreshed.
  await page.locator('.verevon-ticketing-detail-tabs').getByRole('tab', { name: /sak|ticket/i }).click()
  const checklistAction = page.waitForResponse((response) =>
    response.url().includes('/api/v1/actions/execute')
      && response.request().method() === 'POST'
      && response.request().postData()?.includes('tickets.create_checklist') === true,
  )
  await page.getByRole('button', { name: /legg til sjekkliste|add checklist/i }).click()
  expect((await checklistAction).status()).toBe(200)
  await expect.poll(async () => {
    const source = await page.request.get(`/api/v1/tickets/${encodeURIComponent(sourceTicketId)}`)
    if (source.status() !== 200) return false
    return (unwrap<SupportTicket>(await source.json()).checklists?.length ?? 0) > 0
  }, { timeout: 20_000 }).toBe(true)

  // A dependency must survive the ordinary lifecycle: resolve and reopen are
  // distinct audited operations. An open child requires an explicit operator
  // decision; resolution never cascades into the linked ticket.
  await page.getByRole('button', { name: /^(løs|resolve)$/i }).click()
  const dependencyReview = page.getByRole('dialog', { name: /bekreft løsning med åpne underordnede saker|confirm resolution with open child tickets/i })
  await expect(dependencyReview).toContainText(/underordnede saker er fortsatt åpne|child tickets are still open/i)
  await expect(dependencyReview).toContainText(/foreslått|suggested/i)
  const statusBeforeResolution = await page.request.get(`/api/v1/tickets/${encodeURIComponent(sourceTicketId)}`)
  expect(unwrap<SupportTicket>(await statusBeforeResolution.json()).status).not.toBe('resolved')
  await dependencyReview.getByRole('button', { name: /løs likevel|resolve anyway/i }).click()
  await expect.poll(async () => {
    const source = await page.request.get(`/api/v1/tickets/${encodeURIComponent(sourceTicketId)}`)
    if (source.status() !== 200) return null
    return unwrap<SupportTicket>(await source.json()).status
  }, { timeout: 20_000 }).toBe('resolved')
  await expect(page.getByRole('button', { name: /åpne på nytt|reopen/i })).toBeVisible()
  await page.getByRole('button', { name: /åpne på nytt|reopen/i }).click()
  await expect.poll(async () => {
    const source = await page.request.get(`/api/v1/tickets/${encodeURIComponent(sourceTicketId)}`)
    if (source.status() !== 200) return null
    return unwrap<SupportTicket>(await source.json()).status
  }, { timeout: 20_000 }).toBe('open')
  const reopened = unwrap<SupportTicket>(await (await page.request.get(`/api/v1/tickets/${encodeURIComponent(sourceTicketId)}`)).json())
  expect(reopened.linked_resources?.some((link) => link.resource_id === targetTicketId)).toBe(true)
})

test('Inbox: an active canonical macro changes ticket work through the audited action gateway', async ({ page, baseURL }) => {
  const stamp = `${Date.now()}-${Math.random().toString(16).slice(2)}`
  const origin = baseURL ?? 'http://localhost:5199'
  const feedbackText = `E2E Inbox macro ${stamp}`
  const macroName = `E2E route macro ${stamp}`

  const feedback = await page.request.post('/api/v1/inbox/feedback', {
    headers: { 'content-type': 'application/json', origin },
    data: {
      body_text: feedbackText,
      from_name: 'Verevon E2E',
      from_email: 'e2e@verevon.dev',
      page_url: '/inbox?view=all',
      idempotency_key: `inbox-macro-${stamp}`,
    },
  })
  expect(feedback.status()).toBe(201)
  const conversation = unwrap<FeedbackConversation>(await feedback.json())

  const classified = await page.request.post(
    `/api/v1/tickets/conversations/${encodeURIComponent(conversation.id)}/classifications`,
    {
      headers: { 'content-type': 'application/json', origin },
      data: {
        outcome: 'auto_ticket', confidence: 0.99, reason: 'E2E Inbox macro fixture.',
        suggested_fields: { category: 'operations', intent: 'route_to_team', priority: 'normal', severity: 'medium' },
        evidence_message_ids: [],
      },
    },
  )
  expect(classified.status()).toBe(201)
  const ticketId = unwrap<TicketClassification>(await classified.json()).ticket?.id
  expect(ticketId).toBeTruthy()

  // Macro configuration is an owner-only administrative operation. Applying it
  // below is intentionally performed by the Inbox UI and must travel through
  // tickets.run_macro, not the admin configuration endpoint.
  const createdMacro = await page.request.post('/api/v1/ticket-macros', {
    headers: { 'content-type': 'application/json', origin },
    data: {
      name: macroName,
      visibility: 'org',
      active: true,
      actions: { status: 'waiting_team' },
      conditions: {},
    },
  })
  expect(createdMacro.status()).toBe(201)

  await page.goto('/inbox?view=all')
  await page.getByRole('button', { name: `Feedback: ${feedbackText}` }).click()
  await page.getByRole('tab', { name: 'Verevon', exact: true }).click()
  await page.getByRole('button', { name: /makroer|macros/i }).click()

  const macroRow = page.locator('.verevon-inbox-macros li', { hasText: macroName })
  await expect(macroRow).toBeVisible()
  const macroAction = page.waitForResponse((response) =>
    response.url().includes('/api/v1/actions/execute')
      && response.request().method() === 'POST'
      && response.request().postData()?.includes('tickets.run_macro') === true,
  )
  await macroRow.getByRole('button', { name: /bruk|apply/i }).click()
  const macroReview = page.getByRole('dialog', { name: /bekreft makro|confirm macro/i })
  await expect(macroReview).toContainText(/gjennomgå makro før kjøring|review macro before running/i)
  await expect(macroReview).toContainText('waiting_team')
  await macroReview.getByRole('button', { name: /kjør makro|run macro/i }).click()
  expect((await macroAction).status()).toBe(200)

  await expect.poll(async () => {
    const ticket = await page.request.get(`/api/v1/tickets/${encodeURIComponent(ticketId!)}`)
    if (ticket.status() !== 200) return null
    return unwrap<SupportTicket>(await ticket.json()).status
  }, { timeout: 20_000 }).toBe('waiting_team')

  // Inbox row snooze is a real Ticketing lifecycle action, not a local list
  // hide. The canonical ticket must carry both the state and wake timestamp.
  const snoozeAction = page.waitForResponse((response) =>
    response.url().includes('/api/v1/actions/execute')
      && response.request().postData()?.includes('tickets.update') === true,
  )
  await page.locator('.verevon-inbox-ticket-row', { hasText: feedbackText }).getByRole('button', { name: /utsett samtale|snooze conversation/i }).click()
  expect((await snoozeAction).status()).toBe(200)
  await expect.poll(async () => {
    const ticket = await page.request.get(`/api/v1/tickets/${encodeURIComponent(ticketId!)}`)
    if (ticket.status() !== 200) return null
    const canonical = unwrap<SupportTicket>(await ticket.json())
    return canonical.status === 'snoozed' && Boolean(canonical.snoozed_until)
  }, { timeout: 20_000 }).toBe(true)

  // A ticket-backed Inbox close is now explicitly a Ticketing resolution,
  // never a conversation-only state change disguised as case closure.
  const resolveAction = page.waitForResponse((response) =>
    response.url().includes('/api/v1/actions/execute')
      && response.request().postData()?.includes('tickets.resolve') === true,
  )
  await page.getByRole('button', { name: /løs sak|resolve ticket/i }).first().click()
  expect((await resolveAction).status()).toBe(200)
  await expect.poll(async () => {
    const ticket = await page.request.get(`/api/v1/tickets/${encodeURIComponent(ticketId!)}`)
    if (ticket.status() !== 200) return null
    return unwrap<SupportTicket>(await ticket.json()).status
  }, { timeout: 20_000 }).toBe('resolved')
})

test('Inbox: a personal follow-up persists through user-core and never changes ticket lifecycle', async ({ page, baseURL }) => {
  const stamp = `${Date.now()}-${Math.random().toString(16).slice(2)}`
  const origin = baseURL ?? 'http://localhost:5199'
  const feedbackText = `E2E Inbox personal follow-up ${stamp}`
  const reminderTitle = `E2E personal reminder ${stamp}`

  const feedback = await page.request.post('/api/v1/inbox/feedback', {
    headers: { 'content-type': 'application/json', origin },
    data: {
      body_text: feedbackText,
      from_name: 'Verevon E2E',
      from_email: 'e2e@verevon.dev',
      page_url: '/inbox?view=all',
      idempotency_key: `inbox-calendar-${stamp}`,
    },
  })
  expect(feedback.status()).toBe(201)
  const conversation = unwrap<FeedbackConversation>(await feedback.json())
  const classified = await page.request.post(
    `/api/v1/tickets/conversations/${encodeURIComponent(conversation.id)}/classifications`,
    {
      headers: { 'content-type': 'application/json', origin },
      data: {
        outcome: 'auto_ticket', confidence: 0.99, reason: 'E2E personal reminder fixture.',
        suggested_fields: { category: 'operations', intent: 'follow_up', priority: 'normal', severity: 'medium' },
        evidence_message_ids: [],
      },
    },
  )
  expect(classified.status()).toBe(201)
  const ticketId = unwrap<TicketClassification>(await classified.json()).ticket?.id
  expect(ticketId).toBeTruthy()
  const ticketStatusBeforeReminder = unwrap<SupportTicket>(await (await page.request.get(`/api/v1/tickets/${encodeURIComponent(ticketId!)}`)).json()).status

  await page.goto('/inbox?view=all')
  await page.getByRole('button', { name: `Feedback: ${feedbackText}` }).click()
  const inboxAside = page.locator('aside[aria-label]')
  await inboxAside.getByRole('tab', { name: /handlinger|actions/i }).click()
  await expect(inboxAside.getByText(/personlige oppfølginger|personal follow-ups/i)).toBeVisible()
  await inboxAside.getByRole('textbox', { name: /tittel på oppfølging|follow-up title/i }).fill(reminderTitle)

  const createReminder = page.waitForResponse((response) =>
    response.url().includes('/api/v1/navbar/calendar')
      && response.request().method() === 'POST'
      && response.status() === 201,
  )
  await inboxAside.getByRole('button', { name: /^(legg til|add)$/i }).click()
  expect((await createReminder).status()).toBe(201)

  // Both the write and the re-read are against Control Plane's user-scoped
  // calendar. The support ticket remains untouched by this personal reminder.
  await expect.poll(async () => {
    const calendar = await page.request.get('/api/v1/navbar/calendar')
    if (calendar.status() !== 200) return false
    const state = unwrap<{ events: Array<{ title: string }> }>(await calendar.json())
    return state.events.some((event) => event.title === reminderTitle)
  }, { timeout: 20_000 }).toBe(true)
  const ticket = unwrap<SupportTicket>(await (await page.request.get(`/api/v1/tickets/${encodeURIComponent(ticketId!)}`)).json())
  expect(ticket.status).toBe(ticketStatusBeforeReminder)

  await page.reload()
  await page.getByRole('button', { name: `Feedback: ${feedbackText}` }).click()
  await page.locator('aside[aria-label]').getByRole('tab', { name: /handlinger|actions/i }).click()
  await expect(page.locator('aside[aria-label]').getByText(reminderTitle).first()).toBeVisible()
})

test('Inbox: personal pin and read state persists without changing shared conversation work', async ({ page, baseURL }) => {
  const stamp = `${Date.now()}-${Math.random().toString(16).slice(2)}`
  const origin = baseURL ?? 'http://localhost:5199'
  const feedbackText = `E2E Inbox workspace preferences ${stamp}`

  const feedback = await page.request.post('/api/v1/inbox/feedback', {
    headers: { 'content-type': 'application/json', origin },
    data: {
      body_text: feedbackText,
      from_name: 'Verevon E2E',
      from_email: 'e2e@verevon.dev',
      page_url: '/inbox?view=all',
      idempotency_key: `inbox-workspace-${stamp}`,
    },
  })
  expect(feedback.status()).toBe(201)
  const conversation = unwrap<FeedbackConversation>(await feedback.json())
  expect(conversation.id).toBeTruthy()

  await page.goto('/inbox?view=all')
  const row = page.locator('.verevon-inbox-ticket-row', { hasText: feedbackText })
  await expect(row).toBeVisible()
  const pinWrite = page.waitForResponse((response) =>
    response.url().includes('/api/v1/inbox/workspace/pins')
      && response.request().method() === 'POST'
      && response.status() === 200,
  )
  await row.getByRole('button', { name: /fest samtale|pin conversation/i }).click()
  expect((await pinWrite).status()).toBe(200)

  await expect.poll(async () => {
    const workspace = await page.request.get('/api/v1/inbox/workspace')
    if (workspace.status() !== 200) return false
    const state = unwrap<{ pinnedConversationIds: string[] }>(await workspace.json())
    return state.pinnedConversationIds.includes(conversation.id)
  }, { timeout: 20_000 }).toBe(true)

  const readWrite = page.waitForResponse((response) =>
    response.url().includes('/api/v1/inbox/workspace/read')
      && response.request().method() === 'POST'
      && response.status() === 200,
  )
  await row.getByRole('button', { name: `Feedback: ${feedbackText}` }).click()
  expect((await readWrite).status()).toBe(200)
  await expect.poll(async () => {
    const workspace = await page.request.get('/api/v1/inbox/workspace')
    if (workspace.status() !== 200) return false
    const state = unwrap<{ readConversationIds: string[] }>(await workspace.json())
    return state.readConversationIds.includes(conversation.id)
  }, { timeout: 20_000 }).toBe(true)

  await page.reload()
  const reloadedRow = page.locator('.verevon-inbox-ticket-row', { hasText: feedbackText })
  await expect(reloadedRow).toBeVisible()
  await expect(reloadedRow.getByRole('button', { name: /løsne samtale|unpin conversation/i })).toBeVisible()
  await expect(reloadedRow.locator('[aria-label="Ulest samtale"], [aria-label="Unread conversation"]')).toHaveCount(0)
})

test('Inbox: a browser reply composer claims and releases its canonical draft lease', async ({ page, baseURL }) => {
  const stamp = `${Date.now()}-${Math.random().toString(16).slice(2)}`
  const origin = baseURL ?? 'http://localhost:5199'
  const feedbackText = `E2E Inbox draft lease ${stamp}`

  const feedback = await page.request.post('/api/v1/inbox/feedback', {
    headers: { 'content-type': 'application/json', origin },
    data: {
      body_text: feedbackText,
      from_name: 'Verevon E2E',
      from_email: 'e2e@verevon.dev',
      page_url: '/inbox?view=all',
      idempotency_key: `inbox-draft-lease-${stamp}`,
    },
  })
  expect(feedback.status()).toBe(201)
  const conversation = unwrap<FeedbackConversation>(await feedback.json())
  expect(conversation.id).toBeTruthy()

  await page.goto('/inbox?view=all')
  const row = page.getByRole('button', { name: `Feedback: ${feedbackText}` })
  await row.click()
  const composer = page.getByRole('textbox', { name: /svar til verevon e2e|reply to verevon e2e/i })
  await expect(composer).toBeVisible()

  const claim = page.waitForResponse((response) =>
    response.url().includes(`/api/v1/inbox/conversations/${conversation.id}/draft-lease`)
      && response.request().method() === 'POST',
  )
  await composer.focus()
  expect((await claim).status()).toBe(200)
  await expect.poll(async () => {
    const lease = await page.request.get(`/api/v1/inbox/conversations/${encodeURIComponent(conversation.id)}/draft-lease`)
    if (lease.status() !== 200) return null
    return unwrap<{ conversation_id: string }>(await lease.json()).conversation_id
  }, { timeout: 20_000 }).toBe(conversation.id)

  const release = page.waitForResponse((response) =>
    response.url().includes(`/api/v1/inbox/conversations/${conversation.id}/draft-lease`)
      && response.request().method() === 'DELETE',
  )
  await row.click()
  expect((await release).status()).toBe(204)
  await expect.poll(async () => (await page.request.get(
    `/api/v1/inbox/conversations/${encodeURIComponent(conversation.id)}/draft-lease`,
  )).status(), { timeout: 20_000 }).toBe(404)
})

test('Inbox: an operator draft survives a reload without sending an outbound message', async ({ page, baseURL }) => {
  const stamp = `${Date.now()}-${Math.random().toString(16).slice(2)}`
  const origin = baseURL ?? 'http://localhost:5199'
  const feedbackText = `E2E Inbox personal draft ${stamp}`
  const draftText = `Private operator draft ${stamp}`

  const feedback = await page.request.post('/api/v1/inbox/feedback', {
    headers: { 'content-type': 'application/json', origin },
    data: {
      body_text: feedbackText,
      from_name: 'Verevon E2E',
      from_email: 'e2e@verevon.dev',
      page_url: '/inbox?view=all',
      idempotency_key: `inbox-personal-draft-${stamp}`,
    },
  })
  expect(feedback.status()).toBe(201)
  const conversation = unwrap<FeedbackConversation>(await feedback.json())

  await page.goto('/inbox?view=all')
  await page.getByRole('button', { name: `Feedback: ${feedbackText}` }).click()
  const composer = page.getByRole('textbox', { name: /svar til verevon e2e|reply to verevon e2e/i })
  await expect(composer).toBeVisible()

  const save = page.waitForResponse((response) =>
    response.url().includes(`/api/v1/inbox/conversations/${conversation.id}/draft`)
      && response.request().method() === 'PUT',
  )
  await composer.fill(draftText)
  expect((await save).status()).toBe(200)
  await expect(page.getByText(/personlig utkast lagret|personal draft saved/i)).toBeVisible()

  // A reload must recover only the unsent operator text. This test never
  // presses Send and additionally verifies the canonical draft body directly.
  await expect.poll(async () => {
    const draft = await page.request.get(`/api/v1/inbox/conversations/${encodeURIComponent(conversation.id)}/draft`)
    if (draft.status() !== 200) return null
    return unwrap<{ body_text: string }>(await draft.json()).body_text
  }, { timeout: 20_000 }).toBe(draftText)

  await page.reload()
  const reloadedRow = page.getByRole('button', { name: `Feedback: ${feedbackText}` })
  await reloadedRow.click()
  const recoveredComposer = page.getByRole('textbox', { name: /svar til verevon e2e|reply to verevon e2e/i })
  await expect(recoveredComposer).toHaveValue(draftText)
  await expect(page.getByText(/ditt lagrede utkast er gjenopprettet|your saved draft has been restored/i)).toBeVisible()

  // Clean up the test-only private record through the same authenticated BFF.
  const deleted = await page.request.delete(`/api/v1/inbox/conversations/${encodeURIComponent(conversation.id)}/draft`)
  expect(deleted.status()).toBe(204)
})

test('Inbox: AI assist forwards the canonical organization retention posture', async ({ page, baseURL }) => {
  const stamp = `${Date.now()}-${Math.random().toString(16).slice(2)}`
  const origin = baseURL ?? 'http://localhost:5199'
  const feedbackText = `E2E Inbox retention posture ${stamp}`

  const feedback = await page.request.post('/api/v1/inbox/feedback', {
    headers: { 'content-type': 'application/json', origin },
    data: {
      body_text: feedbackText,
      from_name: 'Verevon E2E',
      from_email: 'e2e@verevon.dev',
      page_url: '/inbox?view=all',
      idempotency_key: `inbox-retention-${stamp}`,
    },
  })
  expect(feedback.status()).toBe(201)

  await page.goto('/inbox?view=all')
  await page.getByRole('button', { name: `Feedback: ${feedbackText}` }).click()
  const aside = page.locator('aside[aria-label]')
  await aside.getByRole('tab', { name: 'Verevon', exact: true }).click()

  const postureRead = page.waitForResponse((response) =>
    /\/api\/v1\/orgs\/[^/]+$/.test(new URL(response.url()).pathname)
      && response.request().method() === 'GET',
  )
  const modelInvoke = page.waitForRequest((request) =>
    new URL(request.url()).pathname === '/api/v1/chat/invoke'
      && request.method() === 'POST',
  )
  await aside.getByRole('button', { name: /generer|generate/i }).click()

  const [postureResponse, invoke] = await Promise.all([postureRead, modelInvoke])
  expect(postureResponse.status()).toBe(200)
  const posturePayload = unwrap<{ metadata?: { interactiveRetention?: { zdr?: boolean } } }>(await postureResponse.json())
  const expectedZdr = posturePayload.metadata?.interactiveRetention?.zdr === true
  expect(JSON.parse(invoke.postData() ?? '{}')).toMatchObject({ zdr: expectedZdr })
})

test('Inbox: activity panel reads the canonical empty lease and does not invent SLA state', async ({ page, baseURL }) => {
  const stamp = `${Date.now()}-${Math.random().toString(16).slice(2)}`
  const origin = baseURL ?? 'http://localhost:5199'
  const feedbackText = `E2E Inbox honest activity ${stamp}`
  const feedback = await page.request.post('/api/v1/inbox/feedback', {
    headers: { 'content-type': 'application/json', origin },
    data: {
      body_text: feedbackText,
      from_name: 'Verevon E2E',
      from_email: 'e2e@verevon.dev',
      page_url: '/inbox?view=all',
      idempotency_key: `inbox-activity-${stamp}`,
    },
  })
  expect(feedback.status()).toBe(201)
  const conversation = unwrap<FeedbackConversation>(await feedback.json())

  await page.goto('/inbox?view=all')
  await page.getByRole('button', { name: `Feedback: ${feedbackText}` }).click()
  const aside = page.locator('aside[aria-label]')
  const leaseRead = page.waitForResponse((response) =>
    response.url().includes(`/api/v1/inbox/conversations/${conversation.id}/draft-lease`)
      && response.request().method() === 'GET',
  )
  await aside.getByRole('tab', { name: /revisjon|audit/i }).click()
  expect((await leaseRead).status()).toBe(404)
  await expect(aside.getByText(/no active draft lease|ingen aktiv utkastleie/i)).toBeVisible()
  await expect(aside.getByText(/no automation rule or SLA signal is verified|ingen automatiseringsregel eller SLA-signal er verifisert/i)).toBeVisible()
  await expect(aside.getByText(/on track|på sporet/i)).toHaveCount(0)
  await expect(aside.getByText(/collaboration state is not connected yet|samarbeidsstatus er ikke koblet til ennå/i)).toHaveCount(0)
})

test('Ticketing: a human changes the canonical work type through the audited action boundary', async ({ page, baseURL }) => {
  const stamp = `${Date.now()}-${Math.random().toString(16).slice(2)}`
  const origin = baseURL ?? 'http://localhost:5199'
  const feedback = await page.request.post('/api/v1/inbox/feedback', {
    headers: { 'content-type': 'application/json', origin },
    data: { body_text: `E2E work type ${stamp}`, from_name: 'Verevon E2E', from_email: 'e2e@verevon.dev', page_url: '/tickets', idempotency_key: `ticket-work-type-${stamp}` },
  })
  expect(feedback.status()).toBe(201)
  const conversation = unwrap<FeedbackConversation>(await feedback.json())
  const classified = await page.request.post(`/api/v1/tickets/conversations/${encodeURIComponent(conversation.id)}/classifications`, {
    headers: { 'content-type': 'application/json', origin },
    data: { outcome: 'auto_ticket', confidence: 0.99, reason: 'E2E incident work type fixture.', suggested_fields: { category: 'operations', intent: 'incident_response', priority: 'high', severity: 'critical' }, evidence_message_ids: [] },
  })
  expect(classified.status()).toBe(201)
  const ticketId = unwrap<TicketClassification>(await classified.json()).ticket?.id
  expect(ticketId).toBeTruthy()

  await page.goto(`/tickets?ticketId=${encodeURIComponent(ticketId!)}`)
  const selector = page.getByRole('combobox', { name: /sakens arbeidstype|ticket work type/i })
  await expect(selector).toHaveValue('customer_case')
  const action = page.waitForResponse((response) => response.url().includes('/api/v1/actions/execute') && response.request().method() === 'POST' && response.request().postData()?.includes('"workType":"incident"') === true)
  await selector.selectOption('incident')
  const actionResponse = await action
  expect(actionResponse.status()).toBe(200)
  const actionExecution = unwrap<{ result?: { data?: { work_type?: string } } }>(await actionResponse.json())
  expect(actionExecution.result?.data?.work_type).toBe('incident')
  await expect.poll(async () => {
    const ticket = await page.request.get(`/api/v1/tickets/${encodeURIComponent(ticketId!)}`)
    if (ticket.status() !== 200) return null
    return unwrap<{ work_type?: string }>(await ticket.json()).work_type
  }, { timeout: 20_000 }).toBe('incident')

  const incidentQueue = await page.request.get('/api/v1/tickets?work_type=incident&limit=100')
  expect(incidentQueue.status()).toBe(200)
  const incidentTickets = unwrap<Array<{ id: string; work_type?: string }>>(await incidentQueue.json())
  expect(incidentTickets).toContainEqual(expect.objectContaining({ id: ticketId, work_type: 'incident' }))
  expect(incidentTickets.every((ticket) => ticket.work_type === 'incident')).toBe(true)

  await page.goto('/support?surface=tickets&work_type=incident')
  await expect(page.getByRole('heading', { name: /incidents|hendelser/i })).toBeVisible()
})
