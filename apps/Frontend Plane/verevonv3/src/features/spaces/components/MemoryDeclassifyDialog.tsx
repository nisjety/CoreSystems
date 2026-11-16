import { createSignal, onMount, Show } from 'solid-js'

import { X } from '@/shared/icons'
import { hasErrorCode } from '@/shared/api/api-error'
import {
  declassifySpaceMemory,
  MEMORY_DECLASSIFY_MAX_CHARS,
  retractSpaceMemory,
} from '@/shared/api/memory-client'
import type { DeclassifySpaceMemoryResult } from '@/shared/api/memory-client'
import { translateApiError, useI18n } from '@/shared/i18n'
import { SpaceConfirmButton } from './SpaceConfirmButton'
import './space-private-panel.css'

/**
 * Share one of the person's own memories to a room (WS17-S7), after they have read its exact text.
 *
 * The starting text is the memory as stored. The person reads it, may edit it, and shares it. The copy goes to the room
 * as them, through the declassify route, and the room's agents may then use it. Nothing is sent until they press the
 * button. A retry of the same text reuses its request id, so the room holds the copy once; changed text is a new request.
 * The request carries the memory id, the reviewed text and the retry key, and nothing else: no subject, role or audience.
 *
 * Once the room holds the copy, the dialog stays open on it and offers to take it back from the room (WS17-S7b). That
 * copy's id is kept only for this dialog's life: the room has no list of a person's shares to rebuild it from.
 */

export interface MemoryDeclassifyDialogProps {
  /** The personal memory to share. */
  readonly memoryId: string
  /** The memory's text as stored. Editable; the person reviews it before anything is sent. */
  readonly initialText: string
  /** The room the copy goes to. */
  readonly spaceRef: string
  readonly onClose: () => void
  /** Called once the room holds the copy (stored now, or already stored by an earlier attempt). */
  readonly onShared?: () => void
}

/** A fresh request id for one share: a UUID, as the room post send id is. */
function newRequestId(): string {
  return crypto.randomUUID()
}

/** Characters as the server counts them: code points, not UTF-16 units (the profile editor counts the same way). */
function charCount(text: string): number {
  return [...text].length
}

export function MemoryDeclassifyDialog(props: MemoryDeclassifyDialogProps) {
  const i18n = useI18n()
  const [text, setText] = createSignal(props.initialText)
  const [sharing, setSharing] = createSignal(false)
  const [failure, setFailure] = createSignal<string | null>(null)
  // Set by a typed 412: the policy is Zero Data Retention, so no room copy can be made from this dialog.
  const [refusedByZdr, setRefusedByZdr] = createSignal(false)
  // The copy the room holds, once this dialog has shared it, and the state of taking it back (WS17-S7b).
  const [shared, setShared] = createSignal<DeclassifySpaceMemoryResult | null>(null)
  const [taking, setTaking] = createSignal(false)
  const [takenBack, setTakenBack] = createSignal(false)
  const [takeBackFailure, setTakeBackFailure] = createSignal<string | null>(null)
  let requestId = newRequestId()
  let attemptedText: string | null = null
  let textarea: HTMLTextAreaElement | undefined

  const trimmed = () => text().trim()
  const charsLeft = () => MEMORY_DECLASSIFY_MAX_CHARS - charCount(text())
  const overLimit = () => charsLeft() < 0
  const canShare = () => trimmed().length > 0 && !overLimit() && !sharing() && !refusedByZdr()

  onMount(() => {
    queueMicrotask(() => textarea?.focus())
  })

  async function share(): Promise<void> {
    if (!canShare()) return
    const content = trimmed()
    if (content !== attemptedText) {
      requestId = newRequestId()
      attemptedText = content
    }
    setSharing(true)
    setFailure(null)
    try {
      const result = await declassifySpaceMemory(props.spaceRef, {
        memoryId: props.memoryId,
        reviewedContent: content,
        clientRequestId: requestId,
      })
      setShared(result)
    } catch (error) {
      if (hasErrorCode(error, 'zdr_durable_mutation_forbidden')) setRefusedByZdr(true)
      setFailure(
        translateApiError(error, i18n.tr, {
          no: 'Minnet ble ikke delt. Teksten din er beholdt; prøv igjen.',
          en: 'The memory was not shared. Your text is kept; try again.',
        }),
      )
      return
    } finally {
      setSharing(false)
    }
    // Reached only when the room holds the copy, so a failing callback is never shown as a failed share. The dialog
    // stays open on the copy, so the person can take it back from the room at once if it was shared by mistake.
    props.onShared?.()
  }

  /** Takes the room copy back (WS17-S7b). A copy already gone from the room is the outcome asked for, not a failure. */
  async function takeBack(): Promise<void> {
    const copy = shared()
    if (!copy || taking() || takenBack()) return
    setTaking(true)
    setTakeBackFailure(null)
    try {
      await retractSpaceMemory(copy.spaceRef, {
        memoryId: copy.roomMemoryId,
        clientRequestId: newRequestId(),
      })
      setTakenBack(true)
    } catch (error) {
      if (hasErrorCode(error, 'memory_not_found')) {
        setTakenBack(true)
      } else {
        setTakeBackFailure(
          translateApiError(error, i18n.tr, {
            no: 'Minnet ble ikke tatt tilbake. Prøv igjen.',
            en: 'The memory was not taken back. Try again.',
          }),
        )
      }
    } finally {
      setTaking(false)
    }
  }

  function onKeyDown(event: KeyboardEvent): void {
    if (event.key === 'Escape') {
      event.preventDefault()
      props.onClose()
    }
  }

  return (
    <div
      class="verevon-space-share"
      role="presentation"
      onClick={(event) => {
        if (event.target === event.currentTarget) props.onClose()
      }}
    >
      <section
        class="verevon-space-share__dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="memory-declassify-title"
        onKeyDown={onKeyDown}
      >
        <header class="verevon-space-share__header">
          <h3 id="memory-declassify-title">
            {shared()
              ? i18n.tr('Delt med rommet', 'Shared to the room')
              : i18n.tr('Del minne i rommet', 'Share memory to room')}
          </h3>
          <button type="button" aria-label={i18n.tr('Lukk', 'Close')} onClick={() => props.onClose()}>
            <X size={16} aria-hidden="true" />
          </button>
        </header>

        <p class="verevon-space-share__notice">
          {shared()
            ? i18n.tr(
                'Alle i dette rommet kan se teksten. Du kan ta den tilbake når som helst. Ditt eget minne endres ikke.',
                'Everyone in this room can see this text. You can take it back at any time. Your own memory does not change.',
              )
            : i18n.tr(
                'Alle i dette rommet kan se teksten, og agentene i rommet kan bruke den i svarene sine. Den deles som deg selv, slik den står her. Ditt eget minne endres ikke.',
                'Everyone in this room can see this text, and the room’s agents can use it in their answers. It is shared as you, exactly as written here. Your own memory does not change.',
              )}
        </p>

        <label class="verevon-space-share__label" for="memory-declassify-text">
          {i18n.tr('Minnet du deler', 'The memory you share')}
        </label>
        <textarea
          id="memory-declassify-text"
          class="verevon-space-share__text"
          rows={6}
          value={text()}
          disabled={sharing() || shared() !== null}
          ref={(element) => {
            textarea = element
          }}
          aria-describedby="memory-declassify-count"
          aria-invalid={overLimit() ? 'true' : undefined}
          onInput={(event) => setText(event.currentTarget.value)}
        />
        <Show when={shared() === null}>
          <p id="memory-declassify-count" class="verevon-space-share__count" data-over={overLimit() ? 'true' : undefined}>
            {overLimit()
              ? i18n.tr('For lang for rommet. Forkort teksten før du deler den.', 'Too long for the room. Shorten the text before sharing it.')
              : i18n.tr(`${charsLeft()} tegn igjen`, `${charsLeft()} characters left`)}
          </p>
        </Show>

        <Show when={failure()}>
          {(message) => (
            <p class="verevon-space-share__error" role="alert">
              {message()}
            </p>
          )}
        </Show>

        <Show when={takeBackFailure()}>
          {(message) => (
            <p class="verevon-space-share__error" role="alert">
              {message()}
            </p>
          )}
        </Show>

        <Show when={takenBack()}>
          <p class="verevon-space-share__notice" role="status">
            {i18n.tr('Minnet er tatt tilbake fra rommet.', 'The memory has been taken back from the room.')}
          </p>
        </Show>

        <footer class="verevon-space-share__footer">
          <Show
            when={shared() !== null}
            fallback={
              <>
                <button type="button" class="verevon-space-share__cancel" onClick={() => props.onClose()}>
                  {i18n.tr('Avbryt', 'Cancel')}
                </button>
                <button
                  type="button"
                  class="verevon-space-share__submit"
                  disabled={!canShare()}
                  onClick={() => void share()}
                >
                  {sharing() ? i18n.tr('Deler …', 'Sharing …') : i18n.tr('Del i rommet', 'Share to room')}
                </button>
              </>
            }
          >
            <Show when={!takenBack()}>
              <SpaceConfirmButton
                label={i18n.tr('Ta tilbake fra rommet', 'Take back from the room')}
                consequence={i18n.tr(
                  'Fjernes fra rommet for alle medlemmer. Ditt eget minne endres ikke.',
                  'Removed from the room for every member. Your own memory does not change.',
                )}
                confirmLabel={i18n.tr('Bekreft at minnet fjernes fra rommet', 'Confirm taking this back from the room')}
                disabled={taking()}
                onConfirm={() => void takeBack()}
              />
            </Show>
            <button type="button" class="verevon-space-share__submit" onClick={() => props.onClose()}>
              {i18n.tr('Ferdig', 'Done')}
            </button>
          </Show>
        </footer>
      </section>
    </div>
  )
}
