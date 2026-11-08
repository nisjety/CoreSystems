/**
 * The reader's tools (F-t3-098): the conversation outline and the dyslexia-friendly font. The outline lists the
 * headings of the answers and jumps to the turn that holds one. The font is a reading aid kept on this device.
 *
 * The page mounts it only with `VITE_CHAT_READER_POLISH` on; the component knows nothing of the switch. Escape closes
 * the outline and returns the focus to its button, as the density menu does.
 */

import { createSignal, createUniqueId, For, Show } from 'solid-js'
import { useI18n } from '@/shared/i18n'
import { CHAPTER_TOC_MIN_ENTRIES, type ChapterEntry } from '../lib/chapter-toc'
import './chat-reader.css'

export function ReaderTools(props: {
  chapters: readonly ChapterEntry[]
  dyslexiaFont: boolean
  onDyslexiaFont: (on: boolean) => void
  onJump: (turnId: string) => void
}) {
  const i18n = useI18n()
  const panelId = createUniqueId()
  const [open, setOpen] = createSignal(false)
  let trigger: HTMLButtonElement | undefined

  const close = () => {
    setOpen(false)
    trigger?.focus()
  }
  const jump = (turnId: string) => {
    close()
    props.onJump(turnId)
  }

  return (
    <div
      class="verevon-chat-reader-tools"
      onKeyDown={(event) => {
        if (event.key !== 'Escape' || !open()) return
        event.stopPropagation()
        close()
      }}
    >
      <Show when={props.chapters.length >= CHAPTER_TOC_MIN_ENTRIES}>
        <div class="verevon-chat-reader-tools__outline">
          <button
            ref={(element) => {
              trigger = element
            }}
            type="button"
            class="verevon-chat-reader-tools__button"
            aria-expanded={open() ? 'true' : 'false'}
            aria-controls={open() ? panelId : undefined}
            onClick={() => setOpen((value) => !value)}
          >
            {i18n.tr('Innhold', 'Outline')}
          </button>
          <Show when={open()}>
            <nav
              id={panelId}
              class="verevon-chat-reader-tools__panel"
              aria-label={i18n.tr('Innhold i samtalen', 'Conversation outline')}
            >
              <ol class="verevon-chat-reader-tools__list">
                <For each={props.chapters}>
                  {(entry) => (
                    <li>
                      <button
                        type="button"
                        class="verevon-chat-reader-tools__item"
                        data-level={entry.level}
                        onClick={() => jump(entry.turnId)}
                      >
                        {entry.title}
                      </button>
                    </li>
                  )}
                </For>
              </ol>
            </nav>
          </Show>
        </div>
      </Show>
      <button
        type="button"
        class="verevon-chat-reader-tools__button"
        aria-pressed={props.dyslexiaFont ? 'true' : 'false'}
        onClick={() => props.onDyslexiaFont(!props.dyslexiaFont)}
      >
        {i18n.tr('Dysleksivennlig skrift', 'Dyslexia-friendly font')}
      </button>
    </div>
  )
}
