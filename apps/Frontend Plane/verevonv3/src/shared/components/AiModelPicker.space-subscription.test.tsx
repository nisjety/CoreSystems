// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { ModelInfo } from '@/shared/api/chat-client'

// The ChatGPT subscription route on a Space (DEC-V-R8). With the BYO gateway switch on, the picker offers no subscription
// model in a Space, as it offers no "Your gateway" model there. Outside a Space, and with the switch off, the Subscription
// group is offered as before, and a pick carries its connection as before.

vi.mock('@/shared/api/chatgpt-subscription-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chatgpt-subscription-client')>()
  return {
    ...actual,
    listChatGptSubscriptions: vi.fn().mockResolvedValue([
      { id: 'conn-sub-1', providerKey: 'openai-codex-subscription', status: 'active' },
    ]),
  }
})

import { listChatGptSubscriptions } from '@/shared/api/chatgpt-subscription-client'
import { AiModelPicker } from './AiModelPicker'

const catalogue: ModelInfo[] = [
  { id: 'claude-sonnet-5-5', name: 'Claude Sonnet 5.5', provider: 'anthropic', modality: 'chat' },
  { id: 'gpt-5.6-luna', name: 'GPT 5.6 Luna Subscription', provider: 'openai-codex-subscription', modality: 'chat' },
]

async function openPicker(props: { spaceScoped?: boolean } = {}) {
  const onChange = vi.fn()
  render(() => (
    <AiModelPicker
      orgId="org-1"
      catalogue={() => Promise.resolve(catalogue)}
      spaceScoped={props.spaceScoped}
      onChange={onChange}
    />
  ))
  fireEvent.click(await screen.findByRole('button', { name: /velg ai-modell/i }))
  await screen.findByText('Claude Sonnet 5.5')
  // The subscription lookup answers after the catalogue: let it land before the group is read.
  await waitFor(() => expect(listChatGptSubscriptions).toHaveBeenCalled())
  await new Promise((resolve) => setTimeout(resolve, 10))
  return { onChange }
}

const subscriptionGroup = () => screen.queryByText('Subscription')
const subscriptionModel = () => screen.queryByText('GPT 5.6 Luna Subscription')

afterEach(() => {
  cleanup()
  vi.unstubAllEnvs()
  vi.clearAllMocks()
  window.localStorage.clear()
})

describe('AiModelPicker subscription route on a Space', () => {
  it('offers no subscription model in a Space while the BYO gateway switch is the literal true', async () => {
    vi.stubEnv('VITE_BYO_GATEWAY', 'true')
    await openPicker({ spaceScoped: true })
    expect(screen.getByText('Claude Sonnet 5.5')).toBeTruthy()
    expect(subscriptionGroup()).toBeNull()
    expect(subscriptionModel()).toBeNull()
  })

  it('offers the subscription models outside a Space while the switch is on, as before', async () => {
    vi.stubEnv('VITE_BYO_GATEWAY', 'true')
    await openPicker({ spaceScoped: false })
    expect(await screen.findByText('GPT 5.6 Luna Subscription')).toBeTruthy()
    expect(subscriptionGroup()).not.toBeNull()
  })

  it('offers the subscription models in a Space while the switch is off, as before', async () => {
    vi.stubEnv('VITE_BYO_GATEWAY', '')
    await openPicker({ spaceScoped: true })
    expect(await screen.findByText('GPT 5.6 Luna Subscription')).toBeTruthy()
    expect(subscriptionGroup()).not.toBeNull()
  })

  it('hands a subscription pick outside a Space up with its connection, as before', async () => {
    vi.stubEnv('VITE_BYO_GATEWAY', 'true')
    const { onChange } = await openPicker({ spaceScoped: false })
    fireEvent.click(await screen.findByText('GPT 5.6 Luna Subscription'))
    expect(onChange).toHaveBeenCalledWith(expect.objectContaining({
      model: 'gpt-5.6-luna',
      provider: 'openai-codex-subscription',
      subscriptionConnectionId: 'conn-sub-1',
    }))
  })
})
