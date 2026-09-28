// @vitest-environment jsdom

// F-09 (CHAT_PARITY_AUDIT_2026-09-15.md §3.1): the composer's "slash commands"
// were two unparameterised shortcuts — and, on inspection, dead ones: the
// `slashCommands` array they lived in was matched by an id the menu builder
// never produced. These cases pin the real command system that replaced it:
// a filterable menu, descriptions, an argument taken off the rest of the line,
// keyboard navigation, and — the part that protects F-03 — a "/" typed
// mid-sentence degrading to ordinary text instead of opening anything.

import { createRouter, memoryHistory } from '@solidjs/router'
import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { createSignal, flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'

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
  return { ...actual, loadSpecializedActions: vi.fn().mockResolvedValue(actual.BUILTIN_ACTIONS) }
})

import { DashboardComposer } from './DashboardComposer'

const submitted: Array<Record<string, unknown>> = []

function ComposerPage() {
  const [message, setMessage] = createSignal('')
  const [imageMode, setImageMode] = createSignal(false)
  return (
    <DashboardComposer
      imageMode={imageMode()}
      message={message()}
      onImageModeChange={setImageMode}
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

function pressKey(key: string) {
  fireEvent.keyDown(textbox(), { key, code: key })
  flush()
}

function menu() {
  return document.querySelector('.dashboard-composer-autocomplete-menu')
}

afterEach(() => {
  submitted.length = 0
})

describe('the "/" menu is discoverable', () => {
  it('lists every command with what it does, keeping the two original shortcuts', async () => {
    renderComposer()
    typeText('/')

    expect(await screen.findByText('Upload file')).toBeTruthy()
    expect(screen.getByText('Generate image')).toBeTruthy()
    expect(screen.getByText('Web search')).toBeTruthy()
    expect(screen.getByText('Deep research')).toBeTruthy()
    // A command that does not say what it does is still just a button.
    expect(screen.getByText('Legg ved en fil eller et bilde i meldingen.')).toBeTruthy()
  })

  it('filters on a Norwegian alias and shows the argument the command takes', async () => {
    renderComposer()
    typeText('/bil')

    expect(await screen.findByText('Generate image')).toBeTruthy()
    expect(screen.queryByText('Upload file')).toBeNull()
    expect(screen.getByText('<beskrivelse>')).toBeTruthy()
  })

  it('exposes the list to assistive tech as a listbox with a selected option', async () => {
    renderComposer()
    typeText('/')

    const options = await screen.findAllByRole('option')
    expect(options.length).toBeGreaterThan(1)
    expect(options[0]?.getAttribute('aria-selected')).toBe('true')
    expect(options[1]?.getAttribute('aria-selected')).toBe('false')
  })
})

describe('commands take an argument', () => {
  it('leaves "/image " in the draft awaiting its description instead of firing immediately', async () => {
    renderComposer()
    typeText('/bil')
    expect(await screen.findByText('Generate image')).toBeTruthy()

    pressEnter()

    expect(submitted).toHaveLength(0)
    await waitFor(() => expect(textbox().value).toBe('/image '))
    // The strip says the command is armed and what it still wants.
    expect(screen.getByText('Skriv beskrivelse')).toBeTruthy()
  })

  it('will not send a command whose required argument is still empty', () => {
    renderComposer()
    typeText('/image ')
    expect(menu()).toBeNull()

    pressEnter()

    expect(submitted).toHaveLength(0)
  })

  it('strips the command prefix and sends the argument with the tool it named', async () => {
    renderComposer()
    typeText('/image en rød katt')
    // A completed command is no longer a menu, so Enter sends (F-03's rule).
    expect(menu()).toBeNull()

    pressEnter()

    await waitFor(() => expect(submitted).toHaveLength(1))
    expect(submitted[0]?.text).toBe('en rød katt')
    expect(submitted[0]?.tools).toContain('image')
  })

  it('applies a Norwegian alias typed straight off the keyboard', async () => {
    renderComposer()
    typeText('/dyp hva skjedde med Nordfjord-saken')

    pressEnter()

    await waitFor(() => expect(submitted).toHaveLength(1))
    expect(submitted[0]?.text).toBe('hva skjedde med Nordfjord-saken')
    expect(submitted[0]?.tools).toContain('research')
  })
})

describe('the menu is keyboard-navigable', () => {
  it('moves the selection with the arrow keys before confirming', async () => {
    renderComposer()
    typeText('/')
    expect(await screen.findByText('Upload file')).toBeTruthy()

    pressKey('ArrowDown')
    const options = screen.getAllByRole('option')
    expect(options[1]?.getAttribute('aria-selected')).toBe('true')

    pressEnter()

    expect(submitted).toHaveLength(0)
    await waitFor(() => expect(textbox().value).toBe('/image '))
  })

  it('closes on Escape and then lets Enter send the draft', async () => {
    renderComposer()
    typeText('/web')
    expect(await screen.findByText('Web search')).toBeTruthy()

    pressKey('Escape')
    expect(menu()).toBeNull()

    pressEnter()
    // "/web" is a known command with no argument, so it carries no body and
    // there is nothing to send — the send stays disarmed rather than shipping
    // the bare command as prose.
    expect(submitted).toHaveLength(0)
  })
})

describe('a slash that is not a command stays text', () => {
  it('does not open the menu for a slash inside a sentence, and Enter still sends', () => {
    renderComposer()
    typeText('Prisen er 200/mnd')
    expect(menu()).toBeNull()

    pressEnter()

    expect(submitted).toHaveLength(1)
    expect(submitted[0]?.text).toBe('Prisen er 200/mnd')
  })

  it('sends an unknown command verbatim rather than swallowing it', () => {
    renderComposer()
    typeText('/foobar hva er dette')

    pressEnter()

    expect(submitted).toHaveLength(1)
    expect(submitted[0]?.text).toBe('/foobar hva er dette')
  })
})
