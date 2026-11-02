import { For, onCleanup, Show } from 'solid-js'
import type { JSX } from '@solidjs/web'

import { MoreHorizontal } from '@/shared/icons'
import './space-sidebar-organizer.css'

export type SpaceMenuItem = {
  readonly id: string
  readonly label: string
  readonly onSelect: () => void
  readonly disabled?: boolean
  readonly danger?: boolean
  /** A two-state entry (Mute, a computed section toggle): drawn as a checkbox item with `aria-checked`. */
  readonly checked?: boolean
}

/**
 * The "more" menu of a room row or a section header. It is a native `<details>` so it opens and
 * closes with Enter/Space, `Escape` closes it and returns focus to the button, and a click outside
 * closes it. Every operation here is also reachable without dragging, which is what makes the
 * organiser usable from the keyboard and by assistive technology.
 */
export function SpaceRoomMenu(props: {
  /** Names the thing the menu is for, e.g. "Launch room"; read out with the button. */
  label: string
  items: readonly SpaceMenuItem[]
  disabled?: boolean
  class?: string
  icon?: JSX.Element
}) {
  let details: HTMLDetailsElement | undefined
  const close = (restoreFocus = false) => {
    if (!details) return
    details.open = false
    if (restoreFocus) details.querySelector('summary')?.focus()
  }
  const onDocumentPointer = (event: PointerEvent) => {
    if (details?.open && event.target instanceof Node && !details.contains(event.target)) close()
  }
  onCleanup(() => document.removeEventListener('pointerdown', onDocumentPointer))
  return (
    <Show when={!props.disabled}>
      <details
        ref={details}
        class={`space-sidebar-menu ${props.class ?? ''}`}
        onToggle={() => {
          if (details?.open) document.addEventListener('pointerdown', onDocumentPointer)
          else document.removeEventListener('pointerdown', onDocumentPointer)
        }}
        onKeyDown={(event) => {
          if (event.key === 'Escape' && details?.open) {
            event.preventDefault()
            event.stopPropagation()
            close(true)
          }
        }}
      >
        <summary aria-label={props.label} title={props.label}>
          {props.icon ?? <MoreHorizontal size={15} aria-hidden="true" />}
        </summary>
        <ul class="space-sidebar-menu__list" role="menu" aria-label={props.label}>
          <For each={props.items}>
            {(item) => (
              <li role="none">
                <button
                  type="button"
                  role={item.checked === undefined ? 'menuitem' : 'menuitemcheckbox'}
                  aria-checked={item.checked === undefined ? undefined : item.checked ? 'true' : 'false'}
                  disabled={item.disabled}
                  data-danger={item.danger ? 'true' : undefined}
                  onClick={() => {
                    close(true)
                    item.onSelect()
                  }}
                >
                  {item.checked ? '✓ ' : ''}{item.label}
                </button>
              </li>
            )}
          </For>
        </ul>
      </details>
    </Show>
  )
}
