// @vitest-environment jsdom

// The knowledge import notice under the composer (R18). The page starts the import and the composer shows
// whatever it fails with: a sentence the page wrote (LocalizedError) in the reader's language, a server error
// by its code, and an error that was not written for the reader (a plain Error) never as its own text.

import { createRouter, memoryHistory } from '@solidjs/router'
import { fireEvent, render, screen } from '@solidjs/testing-library'
import { createSignal, flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { ApiError } from '@/shared/api/api-error'
import { I18nProvider, localeStorageKey } from '@/shared/i18n'
import { LocalizedError } from '@/shared/i18n/errors'

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

import { DashboardComposer, PASTE_TEXT_ATTACHMENT_THRESHOLD, type DashboardComposerAttachment } from './DashboardComposer'

type KnowledgeImport = (attachments: DashboardComposerAttachment[]) => Promise<void>

function ComposerPage(props: { onKnowledgeImport: KnowledgeImport }) {
  const [message, setMessage] = createSignal('')
  return (
    <I18nProvider>
      <DashboardComposer
        message={message()}
        onMessageChange={setMessage}
        onSubmit={() => {}}
        onKnowledgeImport={props.onKnowledgeImport}
      />
    </I18nProvider>
  )
}

function renderComposer(onKnowledgeImport: KnowledgeImport) {
  const TestRouter = createRouter({
    routes: [{ path: '/*all', component: () => <ComposerPage onKnowledgeImport={onKnowledgeImport} /> }],
    history: memoryHistory(),
    explicitLinks: true,
  })
  return render(() => <TestRouter>{(props) => <>{props.children}</>}</TestRouter>)
}

/** A long paste becomes a text attachment, and the knowledge import button then saves it. */
function attachTextAndImport(locale: 'no' | 'en') {
  const text = 'a'.repeat(PASTE_TEXT_ATTACHMENT_THRESHOLD + 1)
  fireEvent.paste(screen.getByRole('textbox'), {
    clipboardData: {
      getData: (type: string) => (type === 'text/plain' ? text : ''),
      files: [] as File[],
    },
  })
  flush()
  fireEvent.click(screen.getByRole('button', { name: locale === 'en' ? 'Add to knowledge base' : 'Legg til i kunnskapsbasen' }))
}

afterEach(() => {
  flush()
  window.localStorage.removeItem(localeStorageKey)
})

describe('knowledge import notice under the composer', () => {
  it('shows the fallback for an error that was not written for the reader, never its own text', async () => {
    const onKnowledgeImport = vi.fn().mockRejectedValue(new Error('Upload failed: row 42 is locked by another job'))
    renderComposer(onKnowledgeImport)

    attachTextAndImport('no')

    expect(await screen.findByText('Kunne ikke starte importen.')).toBeTruthy()
    expect(onKnowledgeImport).toHaveBeenCalledTimes(1)
    expect(screen.queryByText(/row 42/)).toBeNull()
  })

  it('shows the sentence the page wrote for a refusal, in Norwegian', async () => {
    renderComposer(vi.fn().mockRejectedValue(new LocalizedError({ no: 'Ingen filer er valgt.', en: 'No files are selected.' })))

    attachTextAndImport('no')

    expect(await screen.findByText('Ingen filer er valgt.')).toBeTruthy()
    expect(screen.queryByText('No files are selected.')).toBeNull()
  })

  it('shows the same refusal in English for an English reader', async () => {
    window.localStorage.setItem(localeStorageKey, 'en')
    renderComposer(vi.fn().mockRejectedValue(new LocalizedError({ no: 'Ingen filer er valgt.', en: 'No files are selected.' })))

    attachTextAndImport('en')

    expect(await screen.findByText('No files are selected.')).toBeTruthy()
    expect(screen.queryByText('Ingen filer er valgt.')).toBeNull()
  })

  it('answers a server error by its code, never with the server text', async () => {
    renderComposer(vi.fn().mockRejectedValue(new ApiError('database connection reset by peer', 500, 'internal_error')))

    attachTextAndImport('no')

    expect(await screen.findByText('Forespørselen kunne ikke fullføres. Prøv igjen, eller kontakt support hvis problemet vedvarer.')).toBeTruthy()
    expect(screen.queryByText(/database connection/)).toBeNull()
  })
})
