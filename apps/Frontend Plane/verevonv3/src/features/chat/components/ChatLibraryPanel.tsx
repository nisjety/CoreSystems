import { useNavigate } from '@solidjs/router'
import { createMemo, createSignal, For, Show } from 'solid-js'
import { ShareDialog } from '@/features/knowledge/components/ShareDialog'
import { ChatLibraryContractError, chatLibraryDocumentIdSchema, parseChatLibraryOperation } from '@/shared/actions/chat-library'
import { createResource } from '@/shared/lib/create-resource-compat'
import {
  getComposerCapabilities,
  listChatLibrary,
  readChatLibraryText,
  type ChatLibraryItem,
  type ChatLibraryKind,
} from '@/shared/api/chat-client'
import { isGateOpen } from '@/shared/context/ownership-gate'
import { useI18n } from '@/shared/i18n'
import { getSession } from '@/shared/session/session-store'
import { selectChatThread } from '../lib/chat-thread-history'
import './chat-library.css'

/** Rows per step. "Show more" asks for one step more, so a row is never listed twice. */
const PAGE_SIZE = 20
/** The largest list the gateway and Data Plane serve in one read. */
const MAX_LIMIT = 100

function readableDate(value: string, locale: string): string {
  const date = new Date(value)
  return Number.isNaN(date.getTime()) ? '' : date.toLocaleDateString(locale, { dateStyle: 'medium' })
}

function readableSize(bytes: number): string {
  return bytes < 1024 ? `${bytes} B` : `${Math.round(bytes / 1024)} KB`
}

/**
 * The chat library on the reports page (F-t3-080): the person's saved artifact copies and uploaded
 * text as one filtered list. A row shows its text, opens the chat it was saved from, or is shared
 * through the knowledge share dialog (grants only; there are no public links). Renders nothing while
 * the gateway's artifact-promote switch is off.
 */
export function ChatLibraryPanel() {
  const i18n = useI18n()
  const navigate = useNavigate()
  const session = getSession()
  const userId = createMemo(() => session.user?.id ?? '')
  const locale = () => (i18n.locale() === 'no' ? 'nb-NO' : 'en-US')
  const [kind, setKind] = createSignal<ChatLibraryKind>('all')
  const [limit, setLimit] = createSignal(PAGE_SIZE)
  const [openId, setOpenId] = createSignal<string | null>(null)
  const [sharingId, setSharingId] = createSignal<string | null>(null)
  // Set when an open or a share is refused by its contract. Nothing was sent, and the person is told why.
  const [refused, setRefused] = createSignal(false)

  const [enabled] = createResource(async () => (await getComposerCapabilities()).artifactPromote)
  // Each read passes its contract before it is sent. A refusal rejects the resource with a ChatLibraryContractError.
  const [page] = createResource(
    () => (enabled() === true ? { kind: kind(), limit: limit() } : undefined),
    async (query) => {
      const list = parseChatLibraryOperation('library.list', { kind: query.kind, limit: query.limit, offset: 0 })
      return listChatLibrary({ kind: list.kind, limit: list.limit, offset: list.offset })
    },
  )
  const [text] = createResource(openId, async (id) => {
    const read = parseChatLibraryOperation('library.read_text', { documentId: id })
    return readChatLibraryText(read.documentId)
  })

  const refusedText = () =>
    i18n.tr('Forespørselen ble ikke sendt, fordi den ikke er gyldig.', 'The request was not sent because it is not valid.')

  const chooseKind = (next: ChatLibraryKind) => {
    setKind(next)
    setLimit(PAGE_SIZE)
    setOpenId(null)
  }

  const toggleText = (documentId: string) => {
    setOpenId(openId() === documentId ? null : documentId)
  }

  const openThread = (threadId: string | undefined) => {
    if (!threadId) return
    let target: string
    try {
      target = parseChatLibraryOperation('library.open_origin', { threadId }).threadId
    } catch {
      setRefused(true)
      return
    }
    setRefused(false)
    selectChatThread(target)
    navigate('/chat')
  }

  // The share dialog opens only for a document id the contract accepts. The grants themselves are
  // checked by the library.share input in the registry; the dialog submits its own grant.
  const startSharing = (documentId: string) => {
    const target = chatLibraryDocumentIdSchema.safeParse(documentId)
    if (!target.success) {
      setRefused(true)
      return
    }
    setRefused(false)
    setSharingId(target.data)
  }

  const kindLabel = (item: ChatLibraryItem) =>
    item.kind === 'generated'
      ? i18n.tr('Generert i chat', 'Generated in chat')
      : i18n.tr('Lastet opp i chat', 'Uploaded in chat')

  return (
    <Show when={enabled() === true}>
      <section class="chat-library" aria-labelledby="chat-library-heading">
        <header class="chat-library__header">
          <h2 id="chat-library-heading">{i18n.tr('Filer fra chat', 'Chat files')}</h2>
          <p>
            {i18n.tr(
              'Tekst du har lagret fra en chat, og tekst du har lastet opp. Bare du ser dem til du deler dem.',
              'Text you saved from a chat and text you uploaded. Only you see them until you share them.',
            )}
          </p>
        </header>
        <div class="chat-library__filters" role="group" aria-label={i18n.tr('Filtrer filer', 'Filter files')}>
          <button type="button" aria-pressed={kind() === 'all' ? 'true' : 'false'} onClick={() => chooseKind('all')}>
            {i18n.tr('Alle', 'All')}
          </button>
          <button
            type="button"
            aria-pressed={kind() === 'generated' ? 'true' : 'false'}
            onClick={() => chooseKind('generated')}
          >
            {i18n.tr('Generert', 'Generated')}
          </button>
          <button
            type="button"
            aria-pressed={kind() === 'uploaded' ? 'true' : 'false'}
            onClick={() => chooseKind('uploaded')}
          >
            {i18n.tr('Lastet opp', 'Uploaded')}
          </button>
        </div>
        <Show when={refused()}>
          <p role="alert">{refusedText()}</p>
        </Show>
        <Show when={!page.loading} fallback={<p role="status">{i18n.tr('Laster filer…', 'Loading files…')}</p>}>
          <Show
            when={!page.error}
            fallback={
              <p role="alert">
                {page.error instanceof ChatLibraryContractError
                  ? refusedText()
                  : i18n.tr('Kunne ikke hente filer akkurat nå.', 'Files could not be loaded right now.')}
              </p>
            }
          >
            <Show
              when={(page()?.items.length ?? 0) > 0}
              fallback={
                <p class="chat-library__empty">
                  {i18n.tr(
                    'Ingen filer ennå. Lagre en artefakt eller last opp en tekstfil i en chat.',
                    'No files yet. Save an artifact or upload a text file in a chat.',
                  )}
                </p>
              }
            >
              <ul class="chat-library__list">
                <For each={page()?.items ?? []}>
                  {(item) => (
                    <li class="chat-library__item">
                      <div class="chat-library__row">
                        <strong>{item.title}</strong>
                        <small>
                          {kindLabel(item)}
                          {' · '}
                          {readableDate(item.updatedAt, locale())}
                          {' · '}
                          {readableSize(item.contentBytes)}
                          <Show when={item.ownerId !== '' && item.ownerId !== userId()}>
                            {' · '}
                            {i18n.tr('Delt med deg', 'Shared with you')}
                          </Show>
                        </small>
                      </div>
                      <div class="chat-library__actions">
                        <button
                          type="button"
                          aria-expanded={openId() === item.documentId ? 'true' : 'false'}
                          onClick={() => toggleText(item.documentId)}
                        >
                          {openId() === item.documentId
                            ? i18n.tr('Skjul tekst', 'Hide text')
                            : i18n.tr('Vis tekst', 'Show text')}
                        </button>
                        <Show when={item.threadId !== undefined}>
                          <button type="button" onClick={() => openThread(item.threadId)}>
                            {i18n.tr('Åpne samtalen', 'Open the chat')}
                          </button>
                        </Show>
                        <Show when={item.ownerId === userId() && isGateOpen()}>
                          <button type="button" onClick={() => startSharing(item.documentId)}>
                            {i18n.tr('Del', 'Share')}
                          </button>
                        </Show>
                      </div>
                      <Show when={openId() === item.documentId}>
                        <Show
                          when={!text.loading}
                          fallback={<p role="status">{i18n.tr('Laster tekst…', 'Loading text…')}</p>}
                        >
                          <Show
                            when={!text.error}
                            fallback={
                              <p role="alert">
                                {text.error instanceof ChatLibraryContractError
                                  ? refusedText()
                                  : i18n.tr('Teksten kunne ikke hentes akkurat nå.', 'The text could not be loaded right now.')}
                              </p>
                            }
                          >
                            <pre class="chat-library__text">{text() ?? ''}</pre>
                          </Show>
                        </Show>
                      </Show>
                    </li>
                  )}
                </For>
              </ul>
              <Show when={(page()?.total ?? 0) > (page()?.items.length ?? 0)}>
                <Show
                  when={limit() < MAX_LIMIT}
                  fallback={
                    <p class="chat-library__empty">
                      {i18n.tr('Viser de nyeste 100 filene.', 'Showing the newest 100 files.')}
                    </p>
                  }
                >
                  <button
                    type="button"
                    class="chat-library__more"
                    onClick={() => setLimit(Math.min(limit() + PAGE_SIZE, MAX_LIMIT))}
                  >
                    {i18n.tr('Vis flere', 'Show more')}
                  </button>
                </Show>
              </Show>
            </Show>
          </Show>
        </Show>
        <Show when={sharingId() !== null}>
          <ShareDialog docId={sharingId() ?? ''} onClose={() => setSharingId(null)} />
        </Show>
      </section>
    </Show>
  )
}
