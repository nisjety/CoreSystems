/**
 * The transcript density menu (EP-14 S2). A button names the level in effect and opens the four levels. Each level
 * is a plain button with `aria-pressed`, so the keyboard reaches them in order and a screen reader hears which one is
 * in effect. Choosing a level, or pressing Escape, closes the list and puts the focus back on its button.
 *
 * The page mounts it only with `VITE_CHAT_TRANSCRIPT_DENSITY` on; the menu itself knows nothing of the switch.
 */

import { createSignal, createUniqueId, For, Show } from 'solid-js'
import { TRANSCRIPT_DENSITY_LEVELS, type TranscriptDensity } from '@/shared/api/navbar-client'
import { ChevronDown } from '@/shared/icons'
import { useI18n } from '@/shared/i18n'
import type { Translator } from '../lib/stream-activity'
import './chat-density.css'

function levelCopy(level: TranscriptDensity, i18n: Translator): { label: string; hint: string } {
  switch (level) {
    case 'detailed':
      return { label: i18n.tr('Detaljert', 'Detailed'), hint: i18n.tr('Hvert steg vises', 'Every step is shown') }
    case 'standard':
      return {
        label: i18n.tr('Standard', 'Standard'),
        hint: i18n.tr('Svar, beslutninger og arbeid i korte linjer', 'Answers, decisions and work in short lines'),
      }
    case 'compact':
      return {
        label: i18n.tr('Kompakt', 'Compact'),
        hint: i18n.tr('Ferdige svar samles til én linje', 'Finished turns fold to one line'),
      }
    case 'minimal':
      return {
        label: i18n.tr('Minimal', 'Minimal'),
        hint: i18n.tr('Bare svar, beslutninger og filer', 'Answers, decisions and files only'),
      }
  }
}

export function DensityMenu(props: { density: TranscriptDensity; onChoose: (density: TranscriptDensity) => void }) {
  const i18n = useI18n()
  const panelId = createUniqueId()
  const [open, setOpen] = createSignal(false)
  let trigger: HTMLButtonElement | undefined

  const close = () => {
    setOpen(false)
    trigger?.focus()
  }
  const choose = (level: TranscriptDensity) => {
    props.onChoose(level)
    close()
  }

  return (
    <div
      class="verevon-chat-density"
      onKeyDown={(event) => {
        if (event.key !== 'Escape' || !open()) return
        event.stopPropagation()
        close()
      }}
    >
      <button
        ref={(element) => {
          trigger = element
        }}
        type="button"
        class="verevon-chat-density__trigger"
        aria-expanded={open() ? 'true' : 'false'}
        aria-controls={open() ? panelId : undefined}
        onClick={() => setOpen((value) => !value)}
      >
        <span>
          {i18n.tr('Visning', 'View')}: {levelCopy(props.density, i18n).label}
        </span>
        <ChevronDown size={14} aria-hidden="true" />
      </button>
      <Show when={open()}>
        <div
          id={panelId}
          class="verevon-chat-density__panel"
          role="group"
          aria-label={i18n.tr('Hvor mye arbeid samtalen viser', 'How much work the conversation shows')}
        >
          <For each={TRANSCRIPT_DENSITY_LEVELS}>
            {(level) => {
              const copy = levelCopy(level, i18n)
              return (
                <button
                  type="button"
                  class="verevon-chat-density__option"
                  aria-pressed={props.density === level ? 'true' : 'false'}
                  onClick={() => choose(level)}
                >
                  <strong>{copy.label}</strong>
                  <span>{copy.hint}</span>
                </button>
              )
            }}
          </For>
        </div>
      </Show>
    </div>
  )
}
