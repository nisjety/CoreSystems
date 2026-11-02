import { createEffect, createSignal, For, Show } from 'solid-js'

import { SIDEBAR_LIMITS } from '@/shared/actions/space-sidebar'
import { useI18n } from '@/shared/i18n'
import './space-sidebar-organizer.css'

export type MoveDialogSection = { readonly id: string; readonly name: string }

/**
 * The accessible "Move to section" dialog: the same `room.move` the drag performs, for people who
 * cannot or do not want to drag. A native modal `<dialog>` (focus trap, `Escape`, inert background
 * come from the platform), like the chat organiser's. The value `''` means "no section": the room
 * goes back to its built-in group.
 */
export function SpaceMoveDialog(props: {
  /** The room being moved, or null when the dialog is closed. */
  roomName: string | null
  sections: readonly MoveDialogSection[]
  /** The room's current section id, `''` when it is in a built-in group. */
  current: string
  error?: string
  onSubmit: (sectionId: string | null) => void
  onClose: () => void
}) {
  const i18n = useI18n()
  let dialog: HTMLDialogElement | undefined
  let select: HTMLSelectElement | undefined
  const [value, setValue] = createSignal('')
  createEffect(
    () => props.roomName,
    (name) => {
      if (name !== null) {
        setValue(props.current)
        queueMicrotask(() => {
          if (dialog && !dialog.open) dialog.showModal()
          select?.focus()
        })
      } else if (dialog?.open) dialog.close()
    },
  )
  return (
    <dialog
      ref={dialog}
      class="space-sidebar-dialog"
      aria-labelledby="space-move-dialog-title"
      onClose={() => props.onClose()}
      onCancel={() => props.onClose()}
    >
      <form
        onSubmit={(event) => {
          event.preventDefault()
          props.onSubmit(value() || null)
        }}
      >
        <h2 id="space-move-dialog-title">{i18n.tr('Flytt til gruppe', 'Move to section')}</h2>
        <p>
          {i18n.tr(
            `Velg hvor «${props.roomName ?? ''}» skal ligge i sidefeltet ditt. Tilgangen til rommet endres ikke.`,
            `Choose where "${props.roomName ?? ''}" sits in your sidebar. Access to the room does not change.`,
          )}
        </p>
        <label>
          {i18n.tr('Gruppe', 'Section')}
          <select ref={select} value={value()} onChange={(event) => setValue(event.currentTarget.value)}>
            <option value="">{i18n.tr('Ingen gruppe (standardplassering)', 'No section (default place)')}</option>
            <For each={props.sections}>{(section) => <option value={section.id}>{section.name}</option>}</For>
          </select>
        </label>
        <Show when={props.error}>
          <p role="alert">{props.error}</p>
        </Show>
        <div class="space-sidebar-dialog__footer">
          <button type="button" onClick={() => props.onClose()}>{i18n.tr('Avbryt', 'Cancel')}</button>
          <button type="submit">{i18n.tr('Flytt', 'Move')}</button>
        </div>
      </form>
    </dialog>
  )
}

/** Creates or renames a section. `mode` is null when closed; `initial` is the current name when renaming. */
export function SpaceSectionNameDialog(props: {
  mode: 'create' | 'rename' | null
  initial: string
  error?: string
  onSubmit: (name: string) => void
  onClose: () => void
}) {
  const i18n = useI18n()
  let dialog: HTMLDialogElement | undefined
  let input: HTMLInputElement | undefined
  const [name, setName] = createSignal('')
  createEffect(
    () => props.mode,
    (mode) => {
      if (mode !== null) {
        setName(props.initial)
        queueMicrotask(() => {
          if (dialog && !dialog.open) dialog.showModal()
          input?.focus()
          input?.select()
        })
      } else if (dialog?.open) dialog.close()
    },
  )
  return (
    <dialog
      ref={dialog}
      class="space-sidebar-dialog"
      aria-labelledby="space-section-dialog-title"
      onClose={() => props.onClose()}
      onCancel={() => props.onClose()}
    >
      <form
        onSubmit={(event) => {
          event.preventDefault()
          props.onSubmit(name())
        }}
      >
        <h2 id="space-section-dialog-title">
          {props.mode === 'rename' ? i18n.tr('Gi gruppen nytt navn', 'Rename section') : i18n.tr('Ny gruppe', 'New section')}
        </h2>
        <p>{i18n.tr('Grupper er bare for deg. De endrer ikke hvem som har tilgang.', 'Sections are only for you. They do not change who has access.')}</p>
        <label>
          {i18n.tr('Gruppenavn', 'Section name')}
          <input
            ref={input}
            required
            maxlength={SIDEBAR_LIMITS.name}
            value={name()}
            onInput={(event) => setName(event.currentTarget.value)}
          />
        </label>
        <Show when={props.error}>
          <p role="alert">{props.error}</p>
        </Show>
        <div class="space-sidebar-dialog__footer">
          <button type="button" onClick={() => props.onClose()}>{i18n.tr('Avbryt', 'Cancel')}</button>
          <button type="submit">{props.mode === 'rename' ? i18n.tr('Lagre', 'Save') : i18n.tr('Opprett', 'Create')}</button>
        </div>
      </form>
    </dialog>
  )
}
