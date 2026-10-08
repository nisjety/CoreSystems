// @vitest-environment jsdom

/**
 * The small line under an answer saying where it was processed (J14). It is
 * rendered from the gateway's stamp on `usage` alone: `Behandlet: EU (Azure),
 * ZDR` where the server asserted EU and a retention contract, `Behandlet:
 * global, ingen EU- eller ZDR-garanti` on the ChatGPT-subscription route, and
 * nothing at all when the gateway sent no stamp (an older gateway, a thread
 * restored from storage). The model name is never consulted.
 */

import { render } from '@solidjs/testing-library'
import { describe, expect, it, vi } from 'vitest'
import { AssistantMessage } from './ChatMessages'
import type { ChatTurn } from './chat-types'

const baseTurn = (overrides: Partial<ChatTurn> = {}): ChatTurn => ({
  id: 'asst-1',
  role: 'assistant',
  content: 'Svaret er ferdig.',
  createdAt: '2026-10-01T08:00:00.000Z',
  streaming: false,
  tools: [],
  attachments: [],
  ...overrides,
})

const renderTurn = (message: ChatTurn) =>
  render(() => (
    <AssistantMessage
      copied={false}
      message={message}
      onBranch={vi.fn()}
      onCopy={vi.fn()}
      onFeedback={vi.fn(async () => true)}
      onRegenerate={vi.fn()}
      onApprovalDecision={vi.fn()}
      onApprovePlan={vi.fn()}
      onViewSteps={vi.fn()}
    />
  ))

const line = () => document.querySelector('.verevon-chat-processing-line')

describe('AssistantMessage processing line', () => {
  it('reads EU, the provider and ZDR when the server stamped them', () => {
    const { unmount } = renderTurn(baseTurn({
      modelUsed: 'gpt-5.6-terra',
      servedBy: { providerUsed: 'azure-openai', residency: 'eu', privacyTier: 'zdr_contractual' },
    }))
    expect(line()?.textContent).toBe('Behandlet: EU (Azure), ZDR')
    expect(line()?.getAttribute('title')).toContain('Ikke uavhengig verifisert')
    unmount()
  })

  it('reads global with no EU or ZDR guarantee for a subscription answer', () => {
    const { unmount } = renderTurn(baseTurn({
      modelUsed: 'gpt-5.6-terra',
      costBasis: 'subscription',
      servedBy: { providerUsed: 'openai-codex-subscription', residency: 'global', privacyTier: 'global' },
    }))
    const text = line()?.textContent ?? ''
    expect(text).toBe('Behandlet: global, ingen EU- eller ZDR-garanti')
    expect(text).not.toMatch(/EU \(|, ZDR$/)
    unmount()
  })

  it('renders nothing when the gateway sent no stamp, whatever the model name says', () => {
    // A model that sounds European is no evidence of where it ran.
    const { unmount } = renderTurn(baseTurn({ modelUsed: 'azure-eu-gpt-4o-mini', model: 'azure-eu-gpt-4o-mini' }))
    expect(line()).toBeNull()
    unmount()
  })

  it('renders nothing for a stamp that names a provider but asserts no residency or retention', () => {
    const { unmount } = renderTurn(baseTurn({ servedBy: { providerUsed: 'azure-openai' } }))
    expect(line()).toBeNull()
    unmount()
  })

  it('is not shown while the answer is still streaming', () => {
    const { unmount } = renderTurn(baseTurn({
      streaming: true,
      status: 'waiting',
      servedBy: { residency: 'eu', privacyTier: 'eu_resident' },
    }))
    expect(line()).toBeNull()
    unmount()
  })
})
