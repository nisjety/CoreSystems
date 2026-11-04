import { createMemo, createSignal, For, Show } from 'solid-js'
import { createResource } from '@/shared/lib/create-resource-compat'
import { listDocuments, type Document } from '@/shared/api/knowledge-client'
import { listAvailableSkills, type Skill } from '@/shared/api/skills-client'
import { useI18n } from '@/shared/i18n'
import type { ChatContextRef } from '@/shared/actions/chat-context-refs'

/**
 * The context picker (EP-3 S5, OD-4): opened by the composer's "+ Kontekst" button or `/kontekst`,
 * never by a typed character. Each tab lists what the person can already open through an existing
 * read, so the picker offers nothing they could not reach; the gateway resolves every pick again
 * under their own authority. Conversations are not offered yet: the gateway refuses a thread
 * reference until the thread tools (EP-3 S7) exist.
 */
export type ContextRefPickerSpace = { id: string; label: string }

type PickerTab = 'documents' | 'skills' | 'space'

export function ContextRefPicker(props: {
  orgId?: string
  /** The conversation's own Space. A Space reference may only name it, so no other Space is listed. */
  space?: ContextRefPickerSpace
  selected: readonly ChatContextRef[]
  onPick: (ref: ChatContextRef) => void
  onClose: () => void
}) {
  const i18n = useI18n()
  const [tab, setTab] = createSignal<PickerTab>('documents')
  const [filter, setFilter] = createSignal('')
  const [documents] = createResource(
    () => (tab() === 'documents' && props.orgId ? props.orgId : false),
    async (orgId) => {
      try {
        return await listDocuments(orgId)
      } catch {
        return [] as Document[]
      }
    },
  )
  const [skills] = createResource(
    () => tab() === 'skills',
    async () => {
      try {
        return await listAvailableSkills()
      } catch {
        return [] as Skill[]
      }
    },
  )
  const candidates = createMemo<ChatContextRef[]>(() => {
    const query = filter().trim().toLowerCase()
    const all: ChatContextRef[] = tab() === 'documents'
      ? (documents() ?? []).map((document) => ({ kind: 'document' as const, id: document.id, label: document.title }))
      : tab() === 'skills'
        ? (skills() ?? []).map((skill) => ({ kind: 'skill' as const, id: skill.id, label: skill.name }))
        : props.space
          ? [{ kind: 'space' as const, id: props.space.id, label: props.space.label }]
          : []
    return all.filter((ref) =>
      ref.id.trim().length > 0
      && !props.selected.some((item) => item.kind === ref.kind && item.id === ref.id)
      && (!query || ref.label.toLowerCase().includes(query)))
  })
  const tabs: Array<{ id: PickerTab; no: string; en: string }> = [
    { id: 'documents', no: 'Dokumenter', en: 'Documents' },
    { id: 'skills', no: 'Ferdigheter', en: 'Skills' },
    { id: 'space', no: 'Rom', en: 'Space' },
  ]

  return (
    <div
      class="dashboard-composer-context-picker"
      role="dialog"
      aria-label={i18n.tr('Legg til kontekst', 'Add context')}
      onKeyDown={(event) => {
        if (event.key === 'Escape') {
          event.preventDefault()
          props.onClose()
        }
      }}
    >
      <div role="tablist" class="flex gap-1">
        <For each={tabs.filter((item) => item.id !== 'space' || props.space)}>
          {(item) => (
            <button
              type="button"
              role="tab"
              aria-selected={tab() === item.id ? 'true' : 'false'}
              onClick={() => setTab(item.id)}
            >
              {i18n.tr(item.no, item.en)}
            </button>
          )}
        </For>
      </div>
      <input
        type="search"
        aria-label={i18n.tr('Filtrer', 'Filter')}
        value={filter()}
        onInput={(event) => setFilter(event.currentTarget.value)}
      />
      <Show
        when={candidates().length > 0}
        fallback={<p role="note">{i18n.tr('Ingenting å legge til her.', 'Nothing to add here.')}</p>}
      >
        <ul role="listbox" aria-label={i18n.tr('Kontekst', 'Context')}>
          <For each={candidates()}>
            {(ref) => (
              <li>
                <button type="button" role="option" aria-selected="false" onClick={() => props.onPick(ref)}>
                  {ref.label || ref.id}
                </button>
              </li>
            )}
          </For>
        </ul>
      </Show>
      <button type="button" onClick={() => props.onClose()}>{i18n.tr('Lukk', 'Close')}</button>
    </div>
  )
}
