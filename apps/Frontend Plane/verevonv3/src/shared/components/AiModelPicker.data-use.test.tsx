// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { ModelInfo } from '@/shared/api/chat-client'

// A catalogue record states its data use under the model's name, one statement at a time. A
// statement the record does not make is not shown, so a model with none shows no line.

vi.mock('@/shared/api/chatgpt-subscription-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chatgpt-subscription-client')>()
  return { ...actual, listChatGptSubscriptions: vi.fn().mockResolvedValue([]) }
})

import { AiModelPicker } from './AiModelPicker'

const chat = (id: string, dataUse?: ModelInfo['dataUse']): ModelInfo => ({
  id,
  name: id,
  provider: 'anthropic',
  modality: 'chat',
  ...(dataUse ? { dataUse } : {}),
})

async function openMenu(models: ModelInfo[]): Promise<void> {
  render(() => <AiModelPicker orgId="org-1" catalogue={async () => models} />)
  fireEvent.click(await screen.findByRole('button', { name: /velg ai-modell/i }))
}

afterEach(() => {
  cleanup()
  window.localStorage.clear()
})

describe('AiModelPicker data-use line', () => {
  it('shows the attested statement under the model that carries it, and no line under the rest', async () => {
    await openMenu([chat('claude-sonnet-5-5', { zdrAttested: true }), chat('gpt-mini')])

    const line = await screen.findByText('Null datalagring, bekreftet')
    expect(line.closest('button')?.textContent).toContain('claude-sonnet-5-5')
    expect(screen.getAllByText('Null datalagring, bekreftet')).toHaveLength(1)
  })

  it('states training and retention only as the record states them', async () => {
    await openMenu([
      chat('no-training', { trainsOnInputs: false, retentionDays: 0 }),
      chat('kept', { trainsOnInputs: true, retentionDays: 30 }),
    ])

    expect(await screen.findByText('Brukes ikke til trening · Ingen lagring')).toBeTruthy()
    expect(await screen.findByText('Brukes til trening · Lagres i 30 dager')).toBeTruthy()
  })

  it('never says no: a statement the record does not make shows no line', async () => {
    await openMenu([chat('silent', {}), chat('unattested', { zdrAttested: false })])

    expect(await screen.findByText('silent')).toBeTruthy()
    expect(screen.queryByText(/Null datalagring|Brukes|Lagres|Ingen lagring/)).toBeNull()
  })
})
