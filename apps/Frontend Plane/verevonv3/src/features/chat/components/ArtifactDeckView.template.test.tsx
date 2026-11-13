// @vitest-environment jsdom

// W3-EP-5-S7-ui: "Change template" on a deck artifact. Offered only when the chat page provides the
// template actions and they list at least one template; a pick is handed to the page's apply (which sends
// it as a turn of its own); while a run streams the control is disabled. With no provider (the Space
// artifact sheet) the deck renders with no control and no error.

import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import * as chatClient from '@/shared/api/chat-client'
import type { DeckTemplate } from '@/shared/api/chat-client'
import { I18nProvider } from '@/shared/i18n'
import { ArtifactDeckView } from './ArtifactDeckView'
import { DeckTemplateContext, type DeckTemplateActions } from './artifact-renderers'
import type { ChatArtifact } from './chat-types'

vi.mock('@/shared/api/chat-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-client')>()
  return { ...actual, listDeckTemplates: vi.fn() }
})

const here = dirname(fileURLToPath(import.meta.url))
const fixture = readFileSync(join(here, '..', '..', '..', 'shared', 'api', 'contracts', 'deck_manifest_v1.json'), 'utf8')

function artifact(): ChatArtifact {
  return { content: fixture, id: 'artifact-deck', kind: 'deck', title: 'Kvartalstall Q3', version: 5 }
}

function template(overrides: Partial<DeckTemplate> = {}): DeckTemplate {
  return {
    skillId: 'skill-brand',
    name: 'Aquatiq brand',
    description: 'Blue on white.',
    colors: { primary: '#0a3d62', secondary: '#3c6382', text: '#1e272e', background: '#ffffff', accent: [] },
    fonts: { heading: 'Carlito', body: 'Liberation Sans' },
    ...overrides,
  }
}

function renderDeck(actions: DeckTemplateActions | null) {
  return render(() => (
    <I18nProvider>
      <DeckTemplateContext value={actions}>
        <ArtifactDeckView artifact={artifact()} />
      </DeckTemplateContext>
    </I18nProvider>
  ))
}

const changeButton = () => screen.queryByRole('button', { name: /Bytt mal|Change template/ })

// Lets the deck's mount effect and the list promise settle.
async function settle() {
  await new Promise((resolve) => setTimeout(resolve, 0))
  flush()
}

afterEach(() => {
  cleanup()
  vi.mocked(chatClient.listDeckTemplates).mockReset()
})

describe('ArtifactDeckView template control', () => {
  it('renders the deck with no control when no page provides the actions', async () => {
    const view = render(() => (
      <I18nProvider>
        <ArtifactDeckView artifact={artifact()} />
      </I18nProvider>
    ))
    await settle()
    expect(view.container.querySelector('.verevon-deck')).not.toBeNull()
    expect(changeButton()).toBeNull()
  })

  it.each([
    ['null (switch off or a temporary thread)', null],
    ['an empty list', []],
  ])('hides the control when the list is %s', async (_label, listed) => {
    const actions = { list: vi.fn().mockResolvedValue(listed), apply: vi.fn(), busy: () => false }
    renderDeck(actions)
    await settle()
    expect(actions.list).toHaveBeenCalledTimes(1)
    expect(changeButton()).toBeNull()
  })

  it('hides the control when the list fails', async () => {
    renderDeck({ list: vi.fn().mockRejectedValue(new Error('down')), apply: vi.fn(), busy: () => false })
    await settle()
    expect(changeButton()).toBeNull()
  })

  it('hands a picked template to the page, which sends it as a turn', async () => {
    const brand = template()
    vi.mocked(chatClient.listDeckTemplates).mockResolvedValue([brand])
    const actions = { list: vi.fn().mockResolvedValue([brand]), apply: vi.fn(), busy: () => false }
    renderDeck(actions)
    await waitFor(() => expect(changeButton()).not.toBeNull())

    fireEvent.click(changeButton()!)
    flush()
    const option = await screen.findByRole('radio', { name: /Aquatiq brand/ })
    fireEvent.click(option)
    flush()

    expect(actions.apply).toHaveBeenCalledTimes(1)
    expect(actions.apply).toHaveBeenCalledWith(brand)
    expect(screen.queryByRole('radio', { name: /Aquatiq brand/ })).toBeNull()
  })

  it('disables the control while a run streams', async () => {
    const brand = template()
    renderDeck({ list: vi.fn().mockResolvedValue([brand]), apply: vi.fn(), busy: () => true })
    await waitFor(() => expect(changeButton()).not.toBeNull())
    expect((changeButton() as HTMLButtonElement).disabled).toBe(true)
  })
})
