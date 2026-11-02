import { Show } from 'solid-js'
import type { JSX } from '@solidjs/web'

import { ChevronDown, ChevronRight } from '@/shared/icons'
import { useI18n } from '@/shared/i18n'
import './space-sidebar-organizer.css'

/**
 * One collapsible group of rooms in the Spaces sidebar (Starred, a custom section, a built-in group
 * or Hidden). It owns only the header and the drop handlers; what the rows are and what a drop does
 * belong to the panel. The header toggle is a real button with `aria-expanded`, so the section is
 * reachable and operable by keyboard; the optional `menu` sits beside it.
 */
export function SpaceSidebarSection(props: {
  id: string
  title: string
  count: number
  collapsed: boolean
  onToggle?: () => void
  icon?: JSX.Element
  menu?: JSX.Element
  /** Shown under the header while the section is open and has no rows. */
  emptyHint?: string
  /** A room is being dragged and this section would accept it. */
  dropReady?: boolean
  /** The dragged room is over this section. */
  dropActive?: boolean
  onDragOver?: (event: DragEvent) => void
  onDragLeave?: (event: DragEvent) => void
  onDrop?: (event: DragEvent) => void
  children: JSX.Element
}) {
  const i18n = useI18n()
  const bodyId = () => `space-sidebar-section-${props.id}`
  return (
    <section
      class="core-sidebar-group space-sidebar-section"
      data-section-id={props.id}
      data-drop-ready={props.dropReady ? 'true' : undefined}
      data-drop-active={props.dropActive ? 'true' : undefined}
      onDragEnter={(event) => props.onDragOver?.(event)}
      onDragOver={(event) => props.onDragOver?.(event)}
      onDragLeave={(event) => props.onDragLeave?.(event)}
      onDrop={(event) => props.onDrop?.(event)}
    >
      <div class="core-sidebar-group__header space-sidebar-section__header">
        <button
          type="button"
          class="space-sidebar-section__toggle"
          aria-expanded={props.collapsed ? 'false' : 'true'}
          aria-controls={bodyId()}
          disabled={!props.onToggle}
          onClick={() => props.onToggle?.()}
          title={props.collapsed ? i18n.tr('Utvid', 'Expand') : i18n.tr('Skjul', 'Collapse')}
        >
          {props.collapsed
            ? <ChevronRight class="space-sidebar-section__chevron" size={13} strokeWidth={2} aria-hidden="true" />
            : <ChevronDown class="space-sidebar-section__chevron" size={13} strokeWidth={2} aria-hidden="true" />}
          {props.icon}
          <span class="verevon-sidebar-group-title">{props.title}</span>
          <small class="space-sidebar-section__count">{props.count}</small>
        </button>
        {props.menu}
      </div>
      <div id={bodyId()} class="core-sidebar-group__items" hidden={props.collapsed}>
        {props.children}
        <Show when={props.emptyHint && props.count === 0}>
          <p class="space-sidebar-section__hint">{props.emptyHint}</p>
        </Show>
      </div>
    </section>
  )
}
