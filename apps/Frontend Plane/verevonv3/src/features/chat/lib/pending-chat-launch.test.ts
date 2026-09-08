// @vitest-environment jsdom

import { beforeEach, describe, expect, it } from 'vitest'
import { consumePendingChatLaunch, writePendingChatLaunch } from './pending-chat-launch'

beforeEach(() => {
  window.localStorage.clear()
  window.sessionStorage.clear()
})

describe('pending chat launch routing', () => {
  it('preserves a subscription route across dashboard-to-chat navigation', async () => {
    await writePendingChatLaunch({
      text: 'hello from the subscription',
      model: 'gpt-5.6-luna',
      provider: 'openai-codex-subscription',
      subscriptionConnectionId: ' conn_123 ',
      minPrivacyTier: 'global',
      effort: 'deep',
      tone: 'concise',
    })

    expect(consumePendingChatLaunch()).toMatchObject({
      text: 'hello from the subscription',
      model: 'gpt-5.6-luna',
      provider: 'openai-codex-subscription',
      subscriptionConnectionId: 'conn_123',
      minPrivacyTier: 'global',
      effort: 'deep',
      tone: 'concise',
    })
  })
})
