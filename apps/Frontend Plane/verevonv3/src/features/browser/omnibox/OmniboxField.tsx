import { createEffect, createSignal, createUniqueId, For, onCleanup, Show } from 'solid-js'
import { classifyOmniboxInput } from '@/features/dashboard/home/browser-omnibox'
import { webHistoryHost } from '@/features/dashboard/home/web-history'
import { deleteOmniboxEntry } from './omnibox-history'
import { resolveOmniboxIntent, type OmniboxIntent } from './omnibox-resolver'
import { createOmniboxSuggestions, type OmniboxRow } from './omnibox-suggestions'
import './browser-omnibox.css'

// The browser address bar as a combobox (ARIA 1.2 list autocomplete). The field
// is the primary row; the dropdown holds remembered pages, query suggestions and
// the explicit "Search the web" and "Ask Verevon" rows.
//
// Keys, inside the field only:
//   ArrowDown / ArrowUp  move the highlight (wraps)
//   Enter                run the highlighted row, or the typed text when none is highlighted
//   Escape               close the dropdown; the event stops here, so the global
//                        `dismiss` shortcut of the chat does not also fire
//   Shift+Delete         forget the highlighted remembered page

export type OmniboxFieldProps = {
  value: string
  disabled?: boolean
  /** Accessible name of the field, e.g. "Nettleseradresse". */
  label: string
  placeholder?: string
  /** Whether the omnibox search flag is on for this build. */
  searchEnabled: boolean
  /** Whether a hand-off to chat is wired, which shows the "Ask Verevon" row. */
  askEnabled: boolean
  /** The current page address: an empty field navigates there. */
  fallbackUrl?: string | null
  /** The history key of the signed-in identity, or null until it is known. */
  historyKey: () => string | null
  /** Whether this workspace is zero-data-retention right now. */
  zdr: () => boolean
  onValueChange: (value: string) => void
  onIntent: (intent: OmniboxIntent) => void
  /** Called after a remembered page is forgotten from the dropdown. */
  onHistoryChange?: () => void
  ref?: (element: HTMLInputElement) => void
}

/** The next highlighted index for an arrow key (wraps); -1 when there are no rows. */
export function nextOmniboxIndex(current: number, count: number, key: 'ArrowDown' | 'ArrowUp'): number {
  if (count <= 0) return -1
  if (key === 'ArrowDown') return current < 0 || current >= count - 1 ? 0 : current + 1
  return current <= 0 ? count - 1 : current - 1
}

/** What running a dropdown row does. A suggestion is a search, so it needs the search flag. */
export function intentForRow(row: OmniboxRow, searchEnabled: boolean): OmniboxIntent {
  switch (row.kind) {
    case 'history':
      return { kind: 'navigate', url: row.url }
    case 'suggestion':
      return searchEnabled ? { kind: 'search', query: row.text } : { kind: 'noop', reason: 'search_disabled' }
    case 'search':
      return { kind: 'search', query: row.query }
    case 'ask':
      return { kind: 'ask', text: row.text }
  }
}

function rowText(row: OmniboxRow): string {
  switch (row.kind) {
    case 'history':
      return row.title
    case 'suggestion':
      return row.text
    case 'search':
      return `Søk på nettet etter «${row.query}»`
    case 'ask':
      return `Spør Verevon om «${row.text}»`
  }
}

export function OmniboxField(props: OmniboxFieldProps) {
  const listId = `omnibox-list-${createUniqueId()}`
  const optionId = (index: number) => `${listId}-option-${index}`
  const [open, setOpen] = createSignal(false)
  const [rows, setRows] = createSignal<OmniboxRow[]>([])
  const [active, setActive] = createSignal(-1)

  const suggestions = createOmniboxSuggestions({
    historyKey: () => props.historyKey(),
    searchEnabled: props.searchEnabled,
    askEnabled: props.askEnabled,
    zdr: () => props.zdr(),
    onRows: (next) => {
      setRows(next)
      setActive(-1)
    },
  })
  onCleanup(() => suggestions.dispose())
  createEffect(
    () => props.value,
    (value) => suggestions.update(value),
  )

  // Typed free text while search is off: say so, rather than doing nothing.
  const searchOff = () => !props.searchEnabled && classifyOmniboxInput(props.value).kind === 'query'

  const submit = (intent: OmniboxIntent) => {
    setOpen(false)
    setActive(-1)
    props.onIntent(intent)
  }

  const onKeyDown = (event: KeyboardEvent) => {
    if (props.disabled) return
    switch (event.key) {
      case 'ArrowDown':
      case 'ArrowUp': {
        event.preventDefault()
        setOpen(true)
        setActive(nextOmniboxIndex(active(), rows().length, event.key))
        return
      }
      case 'Enter': {
        event.preventDefault()
        const row = rows()[active()]
        if (row) {
          submit(intentForRow(row, props.searchEnabled))
          return
        }
        submit(resolveOmniboxIntent(props.value, { searchEnabled: props.searchEnabled, fallbackUrl: props.fallbackUrl ?? null }))
        return
      }
      case 'Escape': {
        // Always consumed here: the chat's global dismiss must not also fire.
        event.preventDefault()
        event.stopPropagation()
        setOpen(false)
        setActive(-1)
        return
      }
      case 'Delete': {
        if (!event.shiftKey) return
        const row = rows()[active()]
        if (!row || row.kind !== 'history') return
        event.preventDefault()
        deleteOmniboxEntry(row.url)
        setRows(rows().filter((candidate) => candidate !== row))
        setActive(-1)
        props.onHistoryChange?.()
        return
      }
      default:
        return
    }
  }

  return (
    <div class="browser-omnibox">
      <input
        ref={(element) => props.ref?.(element)}
        type="text"
        role="combobox"
        class="browser-omnibox__input"
        value={props.value}
        disabled={props.disabled}
        placeholder={props.placeholder}
        aria-label={props.label}
        aria-autocomplete="list"
        aria-expanded={open() && rows().length > 0 ? 'true' : 'false'}
        aria-controls={listId}
        aria-activedescendant={active() >= 0 && rows()[active()] ? optionId(active()) : undefined}
        autocomplete="off"
        spellcheck={false}
        onInput={(event) => {
          props.onValueChange(event.currentTarget.value)
          setOpen(true)
        }}
        onFocus={() => setOpen(true)}
        onBlur={() => setOpen(false)}
        onKeyDown={onKeyDown}
      />
      <Show when={open() && (rows().length > 0 || searchOff())}>
        <ul id={listId} class="browser-omnibox__list" role="listbox" aria-label={props.label}>
          <For each={rows()}>
            {(row, index) => (
              <li
                id={optionId(index())}
                role="option"
                aria-selected={active() === index() ? 'true' : 'false'}
                class={['browser-omnibox__row', { 'browser-omnibox__row--active': active() === index() }]}
                onMouseDown={(event) => event.preventDefault()}
                onClick={() => submit(intentForRow(row, props.searchEnabled))}
              >
                <span class="browser-omnibox__row-text">{rowText(row)}</span>
                <Show when={row.kind === 'history'}>
                  <span class="browser-omnibox__row-host">{row.kind === 'history' ? webHistoryHost(row.url) : ''}</span>
                </Show>
              </li>
            )}
          </For>
          <Show when={searchOff()}>
            <li role="presentation" class="browser-omnibox__note">
              Søk er ikke slått på her.
            </li>
          </Show>
        </ul>
      </Show>
    </div>
  )
}
