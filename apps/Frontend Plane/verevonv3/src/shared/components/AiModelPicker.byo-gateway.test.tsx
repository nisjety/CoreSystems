// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { ModelInfo } from '@/shared/api/chat-client'
import { readAiModelSelection } from '@/shared/ai/model-selection'

// GAP-W4-BYO-picker / W4-BYO-6: the "Your gateway" group. A customer gateway is never zero data retention, so the
// group is offered only with VITE_BYO_GATEWAY the literal 'true', outside a temporary chat, and only once the
// organisation's settings were read and do not report zdr (hidden when they cannot be read). A pick goes up through
// onGatewayPick and is never written to the remembered platform selection.

const org = vi.hoisted(() => ({ getOrganizationAISettings: vi.fn() }))

vi.mock('@/shared/api/chatgpt-subscription-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chatgpt-subscription-client')>()
  return { ...actual, listChatGptSubscriptions: vi.fn().mockResolvedValue([]) }
})
vi.mock('@/shared/api/organization-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/organization-client')>()),
  getOrganizationAISettings: org.getOrganizationAISettings,
}))

import { AiModelPicker } from './AiModelPicker'

const catalogue: ModelInfo[] = [{ id: 'claude-sonnet-5-5', name: 'Claude Sonnet 5.5', provider: 'anthropic', modality: 'chat' }]
const gateways = [{ id: 'conn_byo_1', name: 'Acme gateway', modelIds: ['acme-large', 'acme-small'] }]

async function openPicker(props: { temporaryChat?: boolean; customerGateways?: typeof gateways } = {}) {
  const onGatewayPick = vi.fn()
  const onChange = vi.fn()
  render(() => (
    <AiModelPicker
      orgId="org-1"
      catalogue={async () => catalogue}
      customerGateways={props.customerGateways ?? gateways}
      temporaryChat={props.temporaryChat}
      onGatewayPick={onGatewayPick}
      onChange={onChange}
    />
  ))
  fireEvent.click(await screen.findByRole('button', { name: /velg ai-modell/i }))
  await screen.findByText('Claude Sonnet 5.5')
  // Lets the organisation settings read settle.
  await new Promise((resolve) => setTimeout(resolve, 10))
  return { onGatewayPick, onChange }
}

const group = () => screen.queryByText('Din gateway')

beforeEach(() => {
  vi.stubEnv('VITE_BYO_GATEWAY', 'true')
  org.getOrganizationAISettings.mockReset().mockResolvedValue({ zdr: false })
})

afterEach(() => {
  cleanup()
  vi.unstubAllEnvs()
  window.localStorage.clear()
})

describe('AiModelPicker "Your gateway" group', () => {
  it('offers each gateway model for an organisation without zero retention', async () => {
    await openPicker()
    expect(await screen.findByText('Din gateway')).toBeTruthy()
    expect(screen.getByText('acme-large')).toBeTruthy()
    expect(screen.getByText('acme-small')).toBeTruthy()
    expect(org.getOrganizationAISettings).toHaveBeenCalledWith('org-1')
  })

  it('hands a pick up with the connection and model, and never remembers it as a platform model', async () => {
    const { onGatewayPick, onChange } = await openPicker()
    fireEvent.click(await screen.findByText('acme-small'))
    expect(onGatewayPick).toHaveBeenCalledWith('conn_byo_1', 'acme-small')
    expect(onChange).not.toHaveBeenCalled()
    expect(readAiModelSelection('org-1').modelConnectionId).toBeUndefined()
    expect(readAiModelSelection('org-1').model).not.toBe('acme-small')
  })

  it('is hidden for a zero-retention organisation', async () => {
    org.getOrganizationAISettings.mockResolvedValue({ zdr: true })
    await openPicker()
    expect(group()).toBeNull()
    expect(screen.queryByText('acme-large')).toBeNull()
  })

  it('is hidden when the organisation settings cannot be read', async () => {
    org.getOrganizationAISettings.mockRejectedValue(new Error('down'))
    await openPicker()
    expect(group()).toBeNull()
  })

  it('is hidden in a temporary chat, and the settings are not even read for it', async () => {
    await openPicker({ temporaryChat: true })
    expect(group()).toBeNull()
    expect(org.getOrganizationAISettings).not.toHaveBeenCalled()
  })

  it.each([undefined, 'false', '1', 'TRUE'])('is hidden with VITE_BYO_GATEWAY %j', async (value) => {
    vi.stubEnv('VITE_BYO_GATEWAY', value as string)
    await openPicker()
    expect(group()).toBeNull()
    expect(org.getOrganizationAISettings).not.toHaveBeenCalled()
  })

  it('is hidden when no gateway serves a model', async () => {
    await openPicker({ customerGateways: [{ id: 'conn_empty', name: 'Empty', modelIds: [] }] })
    expect(group()).toBeNull()
  })
})
