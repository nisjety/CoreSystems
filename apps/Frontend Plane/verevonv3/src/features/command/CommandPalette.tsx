import { createEffect, createMemo, createSignal, For, onCleanup, Show } from 'solid-js'
import { Search } from '@/shared/icons'
import { selectChatThread } from '@/features/chat/lib/chat-thread-history'
import { searchNavbar, type NavbarSearchResult } from '@/shared/api/navbar-client'
import type { SpaceRosterMember, SpaceSummary, AgentDefinitionInstallations } from '@/shared/api/spaces-client'
import { translateApiError, useI18n } from '@/shared/i18n'
import { currentMessageSearchOpener, listCommands, type CommandContext } from './command-registry'
import { recordPaletteRecent, readPaletteRecents } from './palette-recents'
import {
  actionItems,
  agentItems,
  chatItems,
  knowledgeItems,
  loadAgents,
  loadPeople,
  loadRooms,
  matchesQuery,
  PALETTE_GROUP_ORDER,
  peopleItems,
  roomItems,
  threadItems,
  type PaletteActionOutcome,
  type PaletteGroup,
  type PaletteItem,
} from './palette-providers'
import './command.css'

type Row = { readonly type: 'header'; readonly key: string; readonly label: string } | { readonly type: 'item'; readonly key: string; readonly item: PaletteItem; readonly index: number }

const FOCUSABLE = 'button:not([disabled]), [href], input:not([disabled]), [tabindex]:not([tabindex="-1"])'

/**
 * The command palette (WS13-S6, D9): Cmd/Ctrl+K and `/`. Replaces the
 * knowledge-only search dialog behind `space_command_palette_ui`.
 *
 * Accessibility: a modal dialog with a real focus trap and focus return, a
 * combobox input that drives a listbox through `aria-activedescendant`, and
 * ArrowUp/Down, Home/End, Enter and Esc. Nothing typed is stored; picks may be
 * remembered as recents only while local retention is allowed.
 */
export function CommandPalette(props: {
  readonly context: CommandContext
  readonly initialQuery?: string
  readonly onClose: () => void
}) {
  const i18n = useI18n()
  const tr = i18n.tr
  const listboxId = `command-palette-list-${Math.random().toString(36).slice(2, 8)}`
  const opener = typeof document !== 'undefined' ? (document.activeElement as HTMLElement | null) : null
  let panelRef: HTMLDivElement | undefined
  let inputRef: HTMLInputElement | undefined
  let searchTimer: number | undefined
  let searchController: AbortController | undefined

  const [query, setQuery] = createSignal(props.initialQuery ?? '')
  const [activeIndex, setActiveIndex] = createSignal(0)
  const [rooms, setRooms] = createSignal<readonly SpaceSummary[]>([])
  const [agents, setAgents] = createSignal<readonly AgentDefinitionInstallations[]>([])
  const [people, setPeople] = createSignal<readonly SpaceRosterMember[]>([])
  const [knowledge, setKnowledge] = createSignal<readonly NavbarSearchResult[]>([])
  const [knowledgeLoading, setKnowledgeLoading] = createSignal(false)
  const [knowledgeError, setKnowledgeError] = createSignal<string | null>(null)
  const [status, setStatus] = createSignal<string | null>(null)

  const groupLabel = (group: PaletteGroup): string => {
    switch (group) {
      case 'recent': return tr('Nylig', 'Recent')
      case 'messages': return tr('Meldinger', 'Messages')
      case 'commands': return tr('Kommandoer', 'Commands')
      case 'rooms': return tr('Rom', 'Rooms')
      case 'threads': return tr('Tråder', 'Threads')
      case 'agents': return tr('Agenter', 'Agents')
      case 'people': return tr('Personer', 'People')
      case 'actions': return tr('Handlinger', 'Actions')
      case 'knowledge': return tr('Kunnskap', 'Knowledge')
      case 'chats': return tr('Dine samtaler', 'Your chats')
    }
  }

  const outcomeText = (outcome: PaletteActionOutcome): string | null => {
    switch (outcome) {
      case 'done': return tr('Utført.', 'Done.')
      case 'needs_input': return tr('Handlingen trenger mer informasjon.', 'The action needs more input.')
      case 'needs_approval': return tr('Handlingen krever godkjenning og kan ikke kjøres herfra.', 'The action needs approval and cannot run from here.')
      case 'failed': return tr('Handlingen mislyktes.', 'The action failed.')
    }
  }

  // Reads the palette indexes once on open, each through its own plane's gate. A failed read is an empty group.
  void loadRooms().then(setRooms)
  void loadAgents().then(setAgents)
  const roomRef = props.context.spaceRef
  if (roomRef) void loadPeople(roomRef).then(setPeople)

  // Knowledge search (Data Plane): debounced, abortable, never stored.
  createEffect(
    () => query().trim(),
    (trimmed) => {
      window.clearTimeout(searchTimer)
      searchController?.abort()
      if (trimmed.length < 2) {
        setKnowledge([])
        setKnowledgeError(null)
        setKnowledgeLoading(false)
        return
      }
      const controller = new AbortController()
      searchController = controller
      searchTimer = window.setTimeout(() => {
        setKnowledgeLoading(true)
        setKnowledgeError(null)
        searchNavbar(trimmed, controller.signal)
          .then((payload) => {
            if (!controller.signal.aborted) setKnowledge(payload.results)
          })
          .catch((error: unknown) => {
            if (controller.signal.aborted) return
            setKnowledge([])
            setKnowledgeError(translateApiError(error, tr, { no: 'Søk feilet.', en: 'Search failed.' }))
          })
          .finally(() => {
            if (!controller.signal.aborted) setKnowledgeLoading(false)
          })
      }, 220)
    },
  )

  const items = createMemo<readonly PaletteItem[]>(() => {
    const q = query().trim()
    const ctx = props.context
    const out: PaletteItem[] = []

    if (!q) {
      for (const recent of readPaletteRecents()) {
        out.push({ id: `recent:${recent.kind}:${recent.id}`, group: 'recent', label: recent.label, href: recent.href })
      }
    }

    const openSearch = currentMessageSearchOpener()
    if (q && ctx.spaceRef && openSearch) {
      out.push({
        id: 'messages:search',
        group: 'messages',
        label: tr(`Søk i meldinger etter «${q}» i dette rommet`, `Search messages for "${q}" in this room`),
        run: () => openSearch(q),
      })
    }

    for (const command of listCommands(ctx)) {
      if (command.group === 'navigate' || command.id.startsWith('palette.')) continue
      const label = tr(command.labelNo, command.labelEn)
      if (!q || !matchesQuery(q, label)) continue
      out.push({ id: `command:${command.id}`, group: 'commands', label, hint: command.hotkey, run: () => command.run(ctx) })
    }

    out.push(...roomItems(rooms(), q))
    out.push(...threadItems(ctx.spaceRef, q))
    out.push(...agentItems(agents(), ctx.spaceRef, q))
    out.push(...peopleItems(people(), ctx.spaceRef, q))
    out.push(...actionItems(ctx, q, tr, (outcome) => setStatus(outcomeText(outcome))))
    out.push(...knowledgeItems(knowledge()))
    out.push(...chatItems(q))

    // Stable group order regardless of provider push order.
    return out.slice().sort((a, b) => PALETTE_GROUP_ORDER.indexOf(a.group) - PALETTE_GROUP_ORDER.indexOf(b.group))
  })

  const rows = createMemo<readonly Row[]>(() => {
    const out: Row[] = []
    let last: PaletteGroup | undefined
    items().forEach((item, index) => {
      if (item.group !== last) {
        last = item.group
        out.push({ type: 'header', key: `h:${item.group}`, label: groupLabel(item.group) })
      }
      out.push({ type: 'item', key: item.id, item, index })
    })
    return out
  })

  // Keep the highlighted row valid as results change.
  createEffect(
    () => items().length,
    (count) => {
      setActiveIndex((current) => (count === 0 ? 0 : Math.min(current, count - 1)))
    },
  )
  createEffect(
    () => query(),
    () => {
      setActiveIndex(0)
    },
  )

  createEffect(
    () => undefined,
    () => {
      inputRef?.focus()
      inputRef?.select()
    },
  )

  onCleanup(() => {
    window.clearTimeout(searchTimer)
    searchController?.abort()
    // Focus return: back to whatever opened the palette, if it is still on the page.
    if (opener && opener.isConnected) opener.focus()
  })

  const activeItem = () => items()[activeIndex()]

  const pick = (item: PaletteItem | undefined) => {
    if (!item) return
    if (item.recentKind && item.href) {
      recordPaletteRecent({ kind: item.recentKind, id: item.id, label: item.label, href: item.href })
    }
    if (item.run) {
      const result = item.run()
      // Actions report their outcome in the status line, so the palette stays open for them.
      if (item.group === 'actions') {
        void Promise.resolve(result)
        return
      }
      props.onClose()
      void Promise.resolve(result).catch(() => undefined)
      return
    }
    if (item.href) {
      props.onClose()
      if (item.selectChatThreadId) selectChatThread(item.selectChatThreadId)
      props.context.navigate(item.href)
    }
  }

  const onKeyDown = (event: KeyboardEvent) => {
    if (event.isComposing) return
    const count = items().length
    switch (event.key) {
      case 'ArrowDown':
        event.preventDefault()
        if (count) setActiveIndex((current) => (current + 1) % count)
        break
      case 'ArrowUp':
        event.preventDefault()
        if (count) setActiveIndex((current) => (current - 1 + count) % count)
        break
      case 'Home':
        if (event.target === inputRef && query()) return
        event.preventDefault()
        setActiveIndex(0)
        break
      case 'End':
        if (event.target === inputRef && query()) return
        event.preventDefault()
        if (count) setActiveIndex(count - 1)
        break
      case 'Enter':
        event.preventDefault()
        pick(activeItem())
        break
      case 'Escape':
        event.preventDefault()
        event.stopPropagation()
        props.onClose()
        break
      case 'Tab': {
        const focusable = panelRef ? Array.from(panelRef.querySelectorAll<HTMLElement>(FOCUSABLE)) : []
        if (focusable.length === 0) {
          event.preventDefault()
          break
        }
        const first = focusable[0]!
        const last = focusable[focusable.length - 1]!
        const active = document.activeElement
        if (event.shiftKey && active === first) {
          event.preventDefault()
          last.focus()
        } else if (!event.shiftKey && active === last) {
          event.preventDefault()
          first.focus()
        }
        break
      }
    }
  }

  return (
    <div class="command-palette" role="dialog" aria-modal="true" aria-label={tr('Kommandopalett', 'Command palette')} onKeyDown={onKeyDown}>
      <button type="button" class="command-palette__scrim" tabindex="-1" aria-label={tr('Lukk kommandopaletten', 'Close the command palette')} onClick={() => props.onClose()} />
      <div class="command-palette__panel" ref={panelRef}>
        <div class="command-palette__header">
          <Search class="size-4 command-palette__icon" strokeWidth={1.8} aria-hidden="true" />
          <input
            ref={inputRef}
            role="combobox"
            aria-expanded="true"
            aria-controls={listboxId}
            aria-autocomplete="list"
            aria-activedescendant={items().length ? `${listboxId}-${activeIndex()}` : undefined}
            aria-label={tr('Søk etter rom, tråder, agenter, personer og kunnskap', 'Search rooms, threads, agents, people and knowledge')}
            placeholder={tr('Søk eller skriv en kommando', 'Search or type a command')}
            value={query()}
            onInput={(event) => setQuery(event.currentTarget.value)}
          />
          <button type="button" class="command-palette__close" onClick={() => props.onClose()} aria-label={tr('Lukk kommandopaletten', 'Close the command palette')}>
            Esc
          </button>
        </div>
        <div class="command-palette__results">
          <ul id={listboxId} role="listbox" aria-label={tr('Resultater', 'Results')} class="command-palette__list">
            <For each={rows()}>
              {(row) => (
                <Show
                  when={row.type === 'item' ? row : undefined}
                  fallback={<li role="presentation" class="command-palette__group">{row.type === 'header' ? row.label : ''}</li>}
                >
                  {(entry) => (
                    <li
                      role="option"
                      id={`${listboxId}-${entry().index}`}
                      aria-selected={entry().index === activeIndex() ? 'true' : 'false'}
                      class="command-palette__option"
                      onMouseMove={() => setActiveIndex(entry().index)}
                      onMouseDown={(event) => event.preventDefault()}
                      onClick={() => pick(entry().item)}
                    >
                      <span class="command-palette__label">{entry().item.label}</span>
                      <Show when={entry().item.hint}>
                        {(hint) => <small class="command-palette__hint">{hint()}</small>}
                      </Show>
                    </li>
                  )}
                </Show>
              )}
            </For>
          </ul>
          <Show when={items().length === 0 && !knowledgeLoading()}>
            <p class="command-palette__empty">
              {knowledgeError() ?? (query().trim() ? tr('Ingen treff.', 'No matches.') : tr('Begynn å skrive for å søke.', 'Start typing to search.'))}
            </p>
          </Show>
          <Show when={knowledgeLoading()}>
            <p class="command-palette__empty">{tr('Søker ...', 'Searching ...')}</p>
          </Show>
          <Show when={knowledgeError() && items().length > 0}>
            <p class="command-palette__empty" role="status">{knowledgeError()}</p>
          </Show>
        </div>
        <p class="command-palette__status" role="status" aria-live="polite">{status() ?? ''}</p>
        <div class="command-palette__footer" aria-hidden="true">
          <span>↑↓ {tr('naviger', 'navigate')}</span>
          <span>↵ {tr('velg', 'select')}</span>
          <span>? {tr('hurtigtaster', 'shortcuts')}</span>
        </div>
      </div>
    </div>
  )
}
