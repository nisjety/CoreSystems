import { Search, PanelLeftClose } from 'lucide-solid'
import type { JSX } from 'solid-js'
import { cn } from '@/shared/lib/cn'

export function SidebarPanelTitle(props: {
  children: JSX.Element
  onCollapse: () => void
  spacing?: string
}) {
  return (
    <div class={cn('core-sidebar-panel-title', props.spacing)}>
      <div class="velion-sidebar-title">{props.children}</div>
      <button
        type="button"
        class="velion-sidebar-collapse-button"
        onClick={() => props.onCollapse()}
        aria-label="Collapse sidebar"
        title="Collapse sidebar"
      >
        <PanelLeftClose class="size-[17px]" strokeWidth={1.75} />
      </button>
    </div>
  )
}

export function SidebarSearchField(props: {
  value: string
  onChange: (value: string) => void
  ariaLabel?: string
  class?: string
  placeholder?: string
}) {
  return (
    <label class={cn('core-sidebar-search', props.class)}>
      <span class="sr-only">{props.ariaLabel ?? 'Filter sidebar section'}</span>
      <Search class="velion-sidebar-search-icon" strokeWidth={1.8} />
      <input
        value={props.value}
        onInput={(event) => props.onChange(event.currentTarget.value)}
        class="velion-sidebar-search-input velion-sidebar-input"
        placeholder={props.placeholder ?? 'Filtrer denne seksjonen'}
      />
    </label>
  )
}

export function SidebarEmptyState(props: { label: string }) {
  return <p class="core-sidebar-dedicated-empty velion-sidebar-secondary">{props.label}</p>
}
