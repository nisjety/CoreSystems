import { For } from 'solid-js'
import type { JSX } from '@solidjs/web'
import { useI18n } from '@/shared/i18n'
import type { BrowserDockPosition } from './browser-dock-state'
import './browser-dock.css'

export type BrowserDockControlsProps = {
  /** The layout the dock is in right now. The controls keep no position of their own. */
  position: BrowserDockPosition
  /** Asks for a layout. `hidden` collapses the rail; any other value opens the dock in that layout. */
  onPositionChange: (position: BrowserDockPosition) => void
}

type DockOptionView = { position: BrowserDockPosition; label: string; title: string }

/**
 * The four layouts of the browser dock as one group of toggle buttons (WS9.G). Presentation only:
 * the parent owns the record and decides what a choice writes.
 */
export function BrowserDockControls(props: BrowserDockControlsProps): JSX.Element {
  const i18n = useI18n()
  const tr = i18n.tr
  const options = (): DockOptionView[] => [
    {
      position: 'right',
      label: tr('Høyre', 'Right'),
      title: tr('Vis nettleseren til høyre for samtalen', 'Show the browser to the right of the chat'),
    },
    {
      position: 'bottom',
      label: tr('Nederst', 'Bottom'),
      title: tr('Vis nettleseren under samtalen', 'Show the browser below the chat'),
    },
    {
      position: 'maximised',
      label: tr('Maksimer', 'Maximise'),
      title: tr('Fyll siden med nettleseren', 'Fill the page with the browser'),
    },
    {
      position: 'hidden',
      label: tr('Skjul', 'Hide'),
      title: tr('Skjul nettleseren', 'Hide the browser'),
    },
  ]

  return (
    <div
      class="browser-dock-controls"
      role="group"
      aria-label={tr('Plassering av nettleseren', 'Browser position')}
      data-position={props.position}
    >
      <For each={options()}>
        {(option) => (
          <button
            type="button"
            class="browser-dock-controls__button"
            data-position={option.position}
            aria-pressed={props.position === option.position ? 'true' : 'false'}
            title={option.title}
            onClick={() => props.onPositionChange(option.position)}
          >
            {option.label}
          </button>
        )}
      </For>
    </div>
  )
}
