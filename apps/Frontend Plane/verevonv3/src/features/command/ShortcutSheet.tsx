import { createEffect, createMemo, For, onCleanup, Show } from 'solid-js'
import { useI18n } from '@/shared/i18n'
import { allCommands, type CommandGroup } from './command-registry'
import './command.css'

const FOCUSABLE = 'button:not([disabled]), [href], input:not([disabled]), [tabindex]:not([tabindex="-1"])'

function isApplePlatform(): boolean {
  return typeof navigator !== 'undefined' && /Mac|iPhone|iPad/i.test(navigator.platform || navigator.userAgent || '')
}

/** `mod+k` -> `Cmd K` / `Ctrl K`; `alt+3` -> `Alt 3`; `mod+shift+arrowup` -> `Ctrl Shift ↑`. */
export function formatHotkey(spec: string, apple = isApplePlatform()): string {
  return spec
    .split('+')
    .map((part) => {
      if (part === 'mod') return apple ? 'Cmd' : 'Ctrl'
      if (part === 'alt') return apple ? 'Option' : 'Alt'
      if (part === 'shift') return 'Shift'
      if (part === 'arrowup') return '↑'
      if (part === 'arrowdown') return '↓'
      return part.length === 1 ? part.toUpperCase() : part
    })
    .join(' ')
}

/**
 * The shortcut sheet (WS13-S6). Generated from the command registry, so what it
 * lists is exactly what the hotkey layer binds; there is no second table to drift.
 */
export function ShortcutSheet(props: { readonly onClose: () => void }) {
  const i18n = useI18n()
  const tr = i18n.tr
  const opener = typeof document !== 'undefined' ? (document.activeElement as HTMLElement | null) : null
  let panelRef: HTMLDivElement | undefined
  let closeRef: HTMLButtonElement | undefined

  const groupLabel = (group: CommandGroup) => {
    switch (group) {
      case 'navigate': return tr('Navigasjon', 'Navigation')
      case 'room': return tr('Rom', 'Room')
      case 'action': return tr('Handlinger', 'Actions')
      case 'help': return tr('Generelt', 'General')
    }
  }

  const sections = createMemo(() => {
    const bound = allCommands().filter((command) => command.hotkey)
    const order: readonly CommandGroup[] = ['help', 'navigate', 'room', 'action']
    return order
      .map((group) => ({ group, commands: bound.filter((command) => command.group === group) }))
      .filter((section) => section.commands.length > 0)
  })

  createEffect(
    () => undefined,
    () => closeRef?.focus(),
  )
  onCleanup(() => {
    if (opener && opener.isConnected) opener.focus()
  })

  const onKeyDown = (event: KeyboardEvent) => {
    if (event.key === 'Escape') {
      event.preventDefault()
      event.stopPropagation()
      props.onClose()
      return
    }
    if (event.key !== 'Tab') return
    const focusable = panelRef ? Array.from(panelRef.querySelectorAll<HTMLElement>(FOCUSABLE)) : []
    if (focusable.length <= 1) {
      event.preventDefault()
      return
    }
    const first = focusable[0]!
    const last = focusable[focusable.length - 1]!
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault()
      last.focus()
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault()
      first.focus()
    }
  }

  return (
    <div class="command-palette" role="dialog" aria-modal="true" aria-label={tr('Hurtigtaster', 'Keyboard shortcuts')} onKeyDown={onKeyDown}>
      <button type="button" class="command-palette__scrim" tabindex="-1" aria-label={tr('Lukk hurtigtaster', 'Close keyboard shortcuts')} onClick={() => props.onClose()} />
      <div class="command-palette__panel command-sheet" ref={panelRef}>
        <div class="command-palette__header">
          <h2 class="command-sheet__title">{tr('Hurtigtaster', 'Keyboard shortcuts')}</h2>
          <button type="button" ref={closeRef} class="command-palette__close" onClick={() => props.onClose()} aria-label={tr('Lukk hurtigtaster', 'Close keyboard shortcuts')}>
            Esc
          </button>
        </div>
        <div class="command-palette__results">
          <For each={sections()}>
            {(section) => (
              <section class="command-sheet__section">
                <h3 class="command-palette__group">{groupLabel(section.group)}</h3>
                <dl class="command-sheet__list">
                  <For each={section.commands}>
                    {(command) => (
                      <div class="command-sheet__row">
                        <dt>{tr(command.labelNo, command.labelEn)}</dt>
                        <dd>
                          <Show when={command.hotkey}>
                            {(hotkey) => <kbd>{formatHotkey(hotkey())}</kbd>}
                          </Show>
                        </dd>
                      </div>
                    )}
                  </For>
                </dl>
              </section>
            )}
          </For>
        </div>
      </div>
    </div>
  )
}
