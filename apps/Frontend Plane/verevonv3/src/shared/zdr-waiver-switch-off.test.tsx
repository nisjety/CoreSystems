// @vitest-environment jsdom

// Verification plan 3.4: VITE_ZDR_WAIVER_UI (ZDR-Z-19d, ZDR-Z-20-to-Z-21b) on the surfaces other than the
// acknowledgement dialog (ConnectionZdrAcknowledgement.test.tsx pins the dialog). Only the literal "true" turns it on.
// Off: a continuation names no waived connection (byte-identical wire), the admin acknowledgement record renders and
// reads nothing, and a zero-retention refusal offers no in-chat review. Each case has an "on" control.

import { render, screen } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'

const spies = vi.hoisted(() => ({
  listOrgZdrAcknowledgements: vi.fn(async () => ({
    items: [],
    counts: { active: 0, superseded: 0, revoked: 0 },
    truncated: false,
  })),
  getConnectionZdr: vi.fn(async () => ({ requiresAcknowledgement: false })),
}))

vi.mock('@/shared/api/connection-zdr-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/connection-zdr-client')>()
  return {
    ...actual,
    listOrgZdrAcknowledgements: spies.listOrgZdrAcknowledgements,
    getConnectionZdr: spies.getConnectionZdr,
  }
})

vi.mock('@/shared/session/session-store', () => ({
  getSession: () => ({
    status: 'authenticated',
    user: { id: 'user-1', role: 'member' },
    activeOrg: { id: 'org-1', name: 'Test', role: 'owner' },
    permissions: [],
  }),
}))

import { AssistantMessage, ErrorNotice } from '@/features/chat/components/ChatMessages'
import { OrgZdrAcknowledgementsSection } from '@/features/settings/components/OrgZdrAcknowledgementsSection'
import { waivedContinuationFields } from '@/shared/api/waived-continuation'
import { en } from '@/shared/i18n/en'
import { no } from '@/shared/i18n/no'

const NOT_TRUE = [undefined, '', 'false', '1', 'TRUE', 'True', ' true', 'yes'] as const

function stub(value: string | undefined) {
  vi.stubEnv('VITE_ZDR_WAIVER_UI', value as string)
}

const tick = () => new Promise((resolve) => setTimeout(resolve, 10))

afterEach(() => {
  vi.unstubAllEnvs()
  vi.clearAllMocks()
})

describe('VITE_ZDR_WAIVER_UI: waived continuation fields', () => {
  const waived = { subscriptionConnectionId: 'conn-abc', provider: 'chatgpt-subscription' }

  it.each(NOT_TRUE)('off for %j: a continuation names no connection', (value) => {
    stub(value)
    expect(waivedContinuationFields(waived)).toEqual({})
  })

  it('on for the literal "true" (control): both fields are named', () => {
    stub('true')
    expect(waivedContinuationFields(waived)).toEqual({
      subscription_connection_id: 'conn-abc',
      provider: 'chatgpt-subscription',
    })
  })
})

describe('VITE_ZDR_WAIVER_UI: the organisation acknowledgement record', () => {
  it.each(NOT_TRUE)('off for %j: renders nothing and reads nothing, even for an owner', async (value) => {
    stub(value)
    const view = render(() => <OrgZdrAcknowledgementsSection />)
    flush()
    await tick()
    expect(view.container.querySelector('section')).toBeNull()
    expect(spies.listOrgZdrAcknowledgements).not.toHaveBeenCalled()
  })

  it('on for the literal "true" (control): an owner sees the section and it reads the record once', async () => {
    stub('true')
    const view = render(() => <OrgZdrAcknowledgementsSection />)
    flush()
    await tick()
    expect(view.container.querySelector('section.verevon-settings-zdr-acknowledgements')).not.toBeNull()
    expect(spies.listOrgZdrAcknowledgements).toHaveBeenCalledTimes(1)
  })
})

describe('VITE_ZDR_WAIVER_UI: the in-chat review on a zero-retention refusal', () => {
  const reviewNames: string[] = [no.zdr.waiver.chat_review, en.zdr.waiver.chat_review]
  const reviewButton = () =>
    screen.queryAllByRole('button').find((button) => reviewNames.includes(button.textContent?.trim() ?? ''))

  function renderRefusal() {
    render(() => (
      <ErrorNotice
        message="refused"
        action="open_integrations"
        connectionId="conn-abc"
        onRetry={() => undefined}
      />
    ))
    flush()
  }

  it.each(NOT_TRUE)('off for %j: no review control, only the link and the retry', async (value) => {
    stub(value)
    renderRefusal()
    await tick()
    expect(reviewButton()).toBeUndefined()
    expect(spies.getConnectionZdr).not.toHaveBeenCalled()
  })

  it('on for the literal "true" (control): the review control is offered', () => {
    stub('true')
    renderRefusal()
    expect(reviewButton()).toBeDefined()
  })
})

describe('VITE_ZDR_WAIVER_UI: the retention line under a waived answer', () => {
  const lineTexts: string[] = [no.zdr.waiver.answer_line, en.zdr.waiver.answer_line]
  const waivedTurn = {
    id: 'asst-1',
    role: 'assistant' as const,
    content: 'Svaret.',
    createdAt: '2026-11-12T08:00:00.000Z',
    streaming: false,
    tools: [],
    attachments: [],
    servedBy: { retentionClass: 'zdr_waived' as const },
  }

  function renderWaivedAnswer() {
    const view = render(() => (
      <AssistantMessage
        copied={false}
        message={waivedTurn as never}
        onBranch={() => undefined}
        onCopy={() => undefined}
        onFeedback={() => Promise.resolve(true)}
        onRegenerate={() => undefined}
        onApprovalDecision={() => undefined}
        onApprovePlan={() => undefined}
        onViewSteps={() => undefined}
      />
    ))
    flush()
    return Array.from(view.container.querySelectorAll('p.verevon-chat-processing-line')).map((p) => p.textContent?.trim())
  }

  it.each(NOT_TRUE)('off for %j: no retention line, though the usage frame says zdr_waived', (value) => {
    stub(value)
    expect(renderWaivedAnswer().filter((text) => lineTexts.includes(text ?? ''))).toEqual([])
  })

  it('on for the literal "true" (control): the waived answer says so', () => {
    stub('true')
    expect(renderWaivedAnswer().filter((text) => lineTexts.includes(text ?? ''))).toHaveLength(1)
  })
})
