import { useLocation, useNavigate } from '@solidjs/router'
import { createSignal, For, Show } from 'solid-js'

import { Folder, MessageSquarePlus, Plus } from '@/shared/icons'
import type { ChatProject } from '@/shared/actions/chat-project'
import { createRoom, getSpaceThreads, type SpaceThread } from '@/shared/api/spaces-client'
import { clearActiveChatThreadId, selectChatThread } from '@/features/chat/lib/chat-thread-history'
import {
  chatProjectsEnabled,
  listChatProjects,
  queueChatProjectStart,
  rememberProjectThread,
} from '@/features/chat/lib/chat-projects'
import { spaceDisplayName } from '@/features/spaces/lib/space-name'
import { useI18n } from '@/shared/i18n'
import { createResource } from '@/shared/lib/create-resource-compat'

/**
 * Projects above the chat list (WS18-S4): the Spaces the caller can talk to the
 * assistant in, a new chat per project, and each project's threads. Off unless the
 * build carries the project switch; with it off nothing here runs or renders.
 *
 * A project thread opens in Chat from its routing hint, and its transcript is read
 * through the room's own route, so this list never widens what a person may read.
 */
export function ChatProjectsSection() {
  // The list is built only while the switch is on: with it off nothing is read or rendered.
  return (
    <Show when={chatProjectsEnabled()}>
      <ChatProjectsList />
    </Show>
  )
}

function ChatProjectsList() {
  const i18n = useI18n()
  const navigate = useNavigate()
  const location = useLocation()
  // `null` means the listing could not be read; an empty list means there are no projects.
  const [projects, { refetch }] = createResource(async () => {
    try {
      return await listChatProjects()
    } catch {
      return null
    }
  })
  const [openSpaceRef, setOpenSpaceRef] = createSignal<string | null>(null)
  const [threads] = createResource(openSpaceRef, async (spaceRef) => (await getSpaceThreads(spaceRef)).threads)
  const [formOpen, setFormOpen] = createSignal(false)
  const [name, setName] = createSignal('')
  const [creating, setCreating] = createSignal(false)
  const [notice, setNotice] = createSignal('')

  // A named room the caller starts alone. It becomes a project once Control registers it,
  // so the list is refetched and may not show it yet.
  async function createProject(event: Event) {
    event.preventDefault()
    const value = name().trim()
    if (!value || creating()) return
    setCreating(true)
    setNotice('')
    try {
      await createRoom(value)
      setName('')
      setFormOpen(false)
      setNotice(i18n.tr(
        'Prosjektet er opprettet. Det vises i listen når det er klart.',
        'The project was created. It appears in the list once it is ready.',
      ))
      await refetch()
    } catch {
      setNotice(i18n.tr(
        'Prosjektet kunne ikke opprettes. Ingenting ble endret.',
        'The project could not be created. Nothing changed.',
      ))
    } finally {
      setCreating(false)
    }
  }

  // Starting a project chat clears the open chat first; the chat page then takes the
  // project for the new one, so nothing from the previous thread can be sent into it.
  function startProjectChat(project: ChatProject) {
    clearActiveChatThreadId()
    const queued = queueChatProjectStart(project.spaceRef)
    if (location.pathname === '/chat') return
    // A page that cannot receive the project would open an ordinary chat, so refuse instead.
    if (!queued) {
      setNotice(i18n.tr(
        'Prosjektsamtalen kunne ikke åpnes. Åpne prosjektet på nytt.',
        'The project chat could not be opened. Open the project again.',
      ))
      return
    }
    navigate('/chat')
  }

  function openProjectThread(project: ChatProject, thread: SpaceThread) {
    rememberProjectThread(thread.thread_id, project.spaceRef)
    selectChatThread(thread.thread_id)
    if (location.pathname !== '/chat') navigate('/chat')
  }

  const threadLabel = (thread: SpaceThread) => (
    thread.title?.trim() || thread.preview?.trim() || i18n.tr('Uten tittel', 'Untitled')
  )

  return (
    <section class="core-chat-projects" aria-labelledby="core-chat-projects-title">
      <div class="core-chat-projects__head">
        <h3 id="core-chat-projects-title" class="verevon-sidebar-group-title">{i18n.tr('Prosjekter', 'Projects')}</h3>
      </div>
      <button
        type="button"
        class="core-chat-sidebar__archive-toggle"
        aria-expanded={formOpen() ? 'true' : 'false'}
        onClick={() => setFormOpen((open) => !open)}
      >
        <Plus class="size-3.5" />
        {i18n.tr('Nytt prosjekt', 'New project')}
      </button>
      <Show when={formOpen()}>
        <form class="core-chat-projects__form" onSubmit={createProject}>
          <label class="core-chat-projects__label">
            <span>{i18n.tr('Navn på prosjektet', 'Project name')}</span>
            <input
              type="text"
              maxlength={120}
              value={name()}
              onInput={(event) => { setName(event.currentTarget.value) }}
            />
          </label>
          <button type="submit" class="core-chat-sidebar__archive-toggle" disabled={creating() || !name().trim()}>
            {i18n.tr('Opprett', 'Create')}
          </button>
        </form>
      </Show>
      <Show when={notice()}>
        <p class="core-sidebar-empty verevon-sidebar-row-normal" role="status">{notice()}</p>
      </Show>
      <Show when={projects() !== null} fallback={
        <p class="core-sidebar-empty verevon-sidebar-row-normal" role="alert">
          {i18n.tr('Prosjektene kunne ikke lastes.', 'Projects could not be loaded.')}
        </p>
      }>
        <Show when={projects() !== undefined} fallback={
          <p class="core-sidebar-empty verevon-sidebar-row-normal">{i18n.tr('Laster prosjekter ...', 'Loading projects ...')}</p>
        }>
          <Show when={(projects() ?? []).length > 0} fallback={
            <p class="core-sidebar-empty verevon-sidebar-row-normal">
              {i18n.tr('Ingen prosjekter ennå.', 'No projects yet.')}
            </p>
          }>
            <ul class="core-chat-projects__list">
              <For each={projects() ?? []}>
                {(project) => (
                  <li class="core-chat-projects__item">
                    <div class="core-chat-projects__row">
                      <button
                        type="button"
                        class="core-chat-sidebar__archive-toggle"
                        aria-expanded={openSpaceRef() === project.spaceRef ? 'true' : 'false'}
                        onClick={() => setOpenSpaceRef(openSpaceRef() === project.spaceRef ? null : project.spaceRef)}
                      >
                        <Folder class="size-3.5" aria-hidden="true" />
                        <span>{spaceDisplayName(project, i18n.tr)}</span>
                        <small>{project.kind === 'personal' ? i18n.tr('Personlig', 'Personal') : i18n.tr('Delt rom', 'Shared room')}</small>
                      </button>
                      <button
                        type="button"
                        class="core-chat-sidebar__archive-toggle"
                        aria-label={i18n.tr(
                          `Ny samtale i ${spaceDisplayName(project, i18n.tr)}`,
                          `New chat in ${spaceDisplayName(project, i18n.tr)}`,
                        )}
                        title={i18n.tr('Ny samtale i prosjektet', 'New chat in project')}
                        onClick={() => startProjectChat(project)}
                      >
                        <MessageSquarePlus class="size-3.5" />
                      </button>
                    </div>
                    <Show when={openSpaceRef() === project.spaceRef}>
                      <Show when={(threads() ?? []).length > 0} fallback={
                        <p class="core-sidebar-empty verevon-sidebar-row-normal">
                          {threads.error
                            ? i18n.tr('Samtalene kunne ikke lastes.', 'Conversations could not be loaded.')
                            : threads.loading
                              ? i18n.tr('Laster samtaler ...', 'Loading conversations ...')
                              : i18n.tr('Ingen samtaler i prosjektet ennå.', 'No conversations in this project yet.')}
                        </p>
                      }>
                        <ul class="core-chat-projects__threads">
                          <For each={threads() ?? []}>
                            {(thread) => (
                              <li>
                                <button
                                  type="button"
                                  class="core-chat-sidebar__archive-toggle"
                                  onClick={() => openProjectThread(project, thread)}
                                >
                                  {threadLabel(thread)}
                                </button>
                              </li>
                            )}
                          </For>
                        </ul>
                      </Show>
                    </Show>
                  </li>
                )}
              </For>
            </ul>
          </Show>
        </Show>
      </Show>
    </section>
  )
}
