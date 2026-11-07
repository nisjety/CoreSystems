// @vitest-environment jsdom

// EP-11 S6: `/` templates. A template is the person's own text with `{{input}}` filled from the rest
// of the line; picking or typing one only fills the draft, and nothing is sent until the person
// sends. Only the network-facing modules are mocked.

import { createRouter, memoryHistory } from '@solidjs/router'
import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { createSignal, flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { AuthoredBehaviour } from '@/shared/api/authored-kinds-client'

const TEMPLATE: AuthoredBehaviour = {
  id: 'tpl_1',
  kind: 'template',
  name: 'Ukesrapport',
  description: 'Weekly report',
  content: 'Skriv ukesrapporten for {{input}} med tre punkter.',
  settings: {},
}

vi.mock('@/shared/api/chat-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-client')>()
  return {
    ...actual,
    listModels: vi.fn().mockResolvedValue([]),
    listChatThreads: vi.fn().mockResolvedValue([]),
    saveChatThreadSnapshot: vi.fn().mockResolvedValue(null),
  }
})

vi.mock('@/shared/api/chat-actions-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-actions-client')>()
  return {
    ...actual,
    loadSpecializedActions: vi.fn().mockResolvedValue(actual.BUILTIN_ACTIONS),
    loadSlashTemplates: vi.fn().mockResolvedValue([TEMPLATE]),
  }
})

import { loadSlashTemplates } from '@/shared/api/chat-actions-client'
import { fillTemplate } from '@/shared/api/authored-kinds-client'
import { DashboardComposer } from './DashboardComposer'

const submitted: Array<Record<string, unknown>> = []

function ComposerPage() {
  const [message, setMessage] = createSignal('')
  return (
    <DashboardComposer
      message={message()}
      onMessageChange={setMessage}
      onSubmit={(payload) => {
        submitted.push(payload as unknown as Record<string, unknown>)
      }}
    />
  )
}

function renderComposer() {
  const TestRouter = createRouter({
    routes: [{ path: '/*all', component: ComposerPage }],
    history: memoryHistory(),
    explicitLinks: true,
  })
  return render(() => <TestRouter>{(props) => <>{props.children}</>}</TestRouter>)
}

function textbox() {
  return screen.getByRole('textbox') as HTMLTextAreaElement
}

function typeText(text: string) {
  fireEvent.input(textbox(), { target: { value: text } })
  flush()
}

function pressEnter() {
  fireEvent.keyDown(textbox(), { key: 'Enter', code: 'Enter' })
  flush()
}

afterEach(() => {
  submitted.length = 0
})

describe('slash templates', () => {
  it('template_fills_input_placeholder', async () => {
    renderComposer()
    // The first `/` loads the person's templates.
    typeText('/')
    await waitFor(() => expect(loadSlashTemplates).toHaveBeenCalled())
    expect(await screen.findByText('Ukesrapport')).toBeTruthy()

    typeText('/ukesrapport uke 45')
    pressEnter()

    await waitFor(() => expect(textbox().value).toBe('Skriv ukesrapporten for uke 45 med tre punkter.'))
    expect(submitted).toHaveLength(0)
  })

  it('template_inserts_text_and_does_not_send', async () => {
    renderComposer()
    typeText('/ukes')
    expect(await screen.findByText('Ukesrapport')).toBeTruthy()

    pressEnter()

    await waitFor(() => expect(textbox().value).toBe(fillTemplate(TEMPLATE.content, '')))
    expect(submitted).toHaveLength(0)
    // The draft is ordinary text now: sending it sends exactly what the person sees.
    pressEnter()
    await waitFor(() => expect(submitted).toHaveLength(1))
  })

  it('fills every placeholder and keeps text typed after a template without one', () => {
    expect(fillTemplate('{{input}} og {{input}}', ' uke 45 ')).toBe('uke 45 og uke 45')
    expect(fillTemplate('Oppsummer møtet.', 'styret')).toBe('Oppsummer møtet.\n\nstyret')
    expect(fillTemplate('Oppsummer møtet.', '')).toBe('Oppsummer møtet.')
  })
})
