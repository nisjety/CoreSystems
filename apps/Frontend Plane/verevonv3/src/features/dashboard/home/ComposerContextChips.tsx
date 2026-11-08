import { createEffect, createSignal, For, Show } from 'solid-js'
import { BookOpen, CirclePlus, X } from '@/shared/icons'
import { useI18n } from '@/shared/i18n'
import {
  addContextRef,
  CONTEXT_REFS_LIMIT,
  removeContextRef,
  type ChatContextRef,
} from '@/shared/actions/chat-context-refs'
import { ContextRefPicker, type ContextRefPickerSpace } from '@/features/chat/components/ContextRefPicker'
import type { SlashCommandRegistry } from './composer-slash-registry'

/**
 * The composer's context chips (EP-3 S5): a "+ Kontekst" button, the `/kontekst` command and the chips
 * the person attached. Mounted by the composer in one line; it owns its picker and its command.
 *
 * OD-4: references are added from the picker only. No character opens it (`@` names Space agents,
 * DEC-26, and `#` is a Markdown heading), and a chip is atomic: Backspace or Delete on a focused chip
 * removes the whole chip, never part of its label. The row exists only where the catalogue says the
 * gateway resolves references and the turn may carry them.
 */
export function ComposerContextChips(props: {
  /** The catalogue advertises `context_refs` and this turn may carry references. */
  available: boolean
  refs: readonly ChatContextRef[]
  onChange: (refs: ChatContextRef[]) => void
  registry: SlashCommandRegistry
  orgId?: string
  space?: ContextRefPickerSpace
  disabled?: boolean
}) {
  const i18n = useI18n()
  const [pickerOpen, setPickerOpen] = createSignal(false)

  // `/kontekst` exists only while the row does, so the menu never offers a command that does nothing.
  // The cleanup is returned, not `onCleanup`: inside a two-argument effect this fork drops onCleanup.
  createEffect(
    () => props.available,
    (available) => {
      if (!available) {
        setPickerOpen(false)
        return undefined
      }
      return props.registry.register({
        id: 'builtin:context_refs',
        name: 'kontekst',
        aliases: ['context', 'kilde', 'referanse'],
        label: 'Kontekst',
        description: {
          no: 'Legg ved et dokument, en ferdighet eller rommet som kontekst for meldingen.',
          en: 'Attach a document, a skill or the Space as context for the message.',
        },
        icon: BookOpen,
        run: () => setPickerOpen(true),
      })
    },
  )

  const remove = (ref: ChatContextRef) => props.onChange(removeContextRef(props.refs, ref))
  const full = () => props.refs.length >= CONTEXT_REFS_LIMIT

  return (
    <Show when={props.available}>
      <div
        class="dashboard-composer-context-chips"
        role="list"
        aria-label={i18n.tr('Kontekst for meldingen', 'Context for the message')}
        style={{ display: 'flex', 'flex-wrap': 'wrap', gap: '4px', padding: '2px 12px' }}
      >
        <For each={props.refs}>
          {(ref) => (
            <span
              role="listitem"
              tabindex="0"
              class="dashboard-composer-context-chip"
              data-kind={ref.kind}
              aria-label={ref.label || ref.id}
              onKeyDown={(event) => {
                if (event.key === 'Backspace' || event.key === 'Delete') {
                  event.preventDefault()
                  remove(ref)
                }
              }}
              style={{
                display: 'inline-flex',
                'align-items': 'center',
                gap: '4px',
                'max-width': '240px',
                padding: '1px 8px',
                'border-radius': '999px',
                'font-size': '12px',
                border: '1px solid currentColor',
              }}
            >
              <span style={{ overflow: 'hidden', 'text-overflow': 'ellipsis', 'white-space': 'nowrap' }}>
                {ref.label || ref.id}
              </span>
              <button
                type="button"
                aria-label={i18n.tr(`Fjern ${ref.label || ref.id}`, `Remove ${ref.label || ref.id}`)}
                disabled={props.disabled}
                onClick={() => remove(ref)}
              >
                <X size={11} strokeWidth={2} />
              </button>
            </span>
          )}
        </For>
        <button
          type="button"
          class="dashboard-composer-context-add"
          aria-haspopup="dialog"
          aria-expanded={pickerOpen() ? 'true' : 'false'}
          disabled={props.disabled || full()}
          onClick={() => setPickerOpen((open) => !open)}
        >
          <CirclePlus size={12} strokeWidth={2} />
          {i18n.tr('Kontekst', 'Context')}
        </button>
      </div>
      <Show when={pickerOpen()}>
        <ContextRefPicker
          orgId={props.orgId}
          space={props.space}
          selected={props.refs}
          onPick={(ref) => {
            props.onChange(addContextRef(props.refs, ref))
            setPickerOpen(false)
          }}
          onClose={() => setPickerOpen(false)}
        />
      </Show>
    </Show>
  )
}
