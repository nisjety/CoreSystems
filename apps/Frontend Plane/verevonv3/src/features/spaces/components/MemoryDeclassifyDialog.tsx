import { createMemo, createSignal, For, onSettled, Show } from 'solid-js'

import { X } from '@/shared/icons'
import { hasErrorCode } from '@/shared/api/api-error'
import { MEMORY_DECLASSIFY_MAX_CHARS, type DeclassifySpaceMemoryResult } from '@/shared/api/memory-client'
import { getSpaceRoster, listSpaces, type SpaceSummary } from '@/shared/api/spaces-client'
import type { ActionActor } from '@/shared/actions/types'
import { translateApiError, useI18n } from '@/shared/i18n'
import { getSession } from '@/shared/session/session-store'
import { SpaceConfirmButton } from './SpaceConfirmButton'
import {
  memoryRoomErrorMessage,
  shareableRooms,
  shareMemoryToRoom,
  takeMemoryBackFromRoom,
} from '../lib/space-memory-share'
import './space-private-panel.css'

/**
 * Share one of the person's own memories to a room (WS17-S7), after they have read its exact text, and take the copy
 * back from the room (WS17-S7b).
 *
 * The person picks the room from the shared rooms they are in, reads the starting text, may edit it, and shares it. The
 * room's people are counted once a room is picked, so the person sees who will read it. The copy goes to the room as them,
 * and the room's agents may then use it. Nothing is sent until they press the button. A retry of the same text to the same
 * room reuses its request id, so the room holds the copy once; changed text or another room is a new request.
 *
 * Both writes go through the action registry (`memory.declassify_to_space`, `memory.retract_from_space`) and carry the
 * memory id, the room, the text or the copy's id, and a retry key, and nothing else: no subject, role or audience.
 *
 * Once the room holds the copy, the dialog stays open on it and offers to take it back. That copy's id is kept only for
 * this dialog's life: the room has no list of a person's shares to rebuild it from.
 */

export interface MemoryDeclassifyDialogProps {
  /** The personal memory to share. */
  readonly memoryId: string
  /** The memory's text as stored. Editable; the person reviews it before anything is sent. */
  readonly initialText: string
  readonly onClose: () => void
  /** Called once the room holds the copy (stored now, or already stored by an earlier attempt). */
  readonly onShared?: () => void
}

const SHARE_FALLBACK = {
  no: 'Minnet ble ikke delt. Teksten din er beholdt; prøv igjen.',
  en: 'The memory was not shared. Your text is kept; try again.',
}

const TAKE_BACK_FALLBACK = {
  no: 'Minnet ble ikke tatt tilbake. Prøv igjen.',
  en: 'The memory was not taken back. Try again.',
}

/** A fresh request id for one share or take-back: a UUID, as the room post send id is. */
function newRequestId(): string {
  return crypto.randomUUID()
}

/** Characters as the server counts them: code points, not UTF-16 units (the profile editor counts the same way). */
function charCount(text: string): number {
  return [...text].length
}

/** The signed-in actor for an action, or `null` while the session has no user or organisation (nothing is sent then). */
function currentActor(): ActionActor | null {
  const session = getSession()
  const userId = session.user?.id ?? ''
  const orgId = session.activeOrg?.id ?? ''
  return userId && orgId ? { type: 'human', userId, orgId } : null
}

export function MemoryDeclassifyDialog(props: MemoryDeclassifyDialogProps) {
  const i18n = useI18n()
  const [text, setText] = createSignal(props.initialText)
  // `null` while the rooms load.
  const [rooms, setRooms] = createSignal<readonly SpaceSummary[] | null>(null)
  const [roomsFailure, setRoomsFailure] = createSignal<string | null>(null)
  const [spaceRef, setSpaceRef] = createSignal('')
  // The picked room's people, once its roster has been read; `null` until then, or when it cannot be read.
  const [peopleCount, setPeopleCount] = createSignal<number | null>(null)
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
  let attempted: { readonly content: string; readonly spaceRef: string } | null = null
  let takeBackId: string | null = null
  let rosterRead = 0
  let textarea: HTMLTextAreaElement | undefined

  const trimmed = () => text().trim()
  // The server trims the text before it checks the length, so the count is the trimmed text's.
  const charsLeft = () => MEMORY_DECLASSIFY_MAX_CHARS - charCount(trimmed())
  const overLimit = () => charsLeft() < 0
  const roomList = createMemo(() => rooms() ?? [])
  const sharedRoomName = () => {
    const copy = shared()
    if (!copy) return ''
    return roomList().find((room) => room.space_ref === copy.spaceRef)?.name ?? ''
  }
  const canShare = () =>
    trimmed().length > 0 &&
    !overLimit() &&
    spaceRef() !== '' &&
    !sharing() &&
    !refusedByZdr() &&
    shared() === null &&
    currentActor() !== null

  onSettled(() => {
    queueMicrotask(() => textarea?.focus())
    void loadRooms()
  })

  async function loadRooms(): Promise<void> {
    try {
      setRooms(shareableRooms(await listSpaces()))
    } catch (error) {
      setRoomsFailure(
        translateApiError(error, i18n.tr, {
          no: 'Rommene dine kunne ikke hentes. Prøv igjen senere.',
          en: 'Your rooms could not be loaded. Try again later.',
        }),
      )
      setRooms([])
    }
  }

  /** Picks a room and reads its people for the count. A stale read never overwrites a newer pick. */
  async function pickRoom(next: string): Promise<void> {
    setSpaceRef(next)
    setPeopleCount(null)
    const read = ++rosterRead
    if (next === '') return
    try {
      const members = await getSpaceRoster(next)
      if (read === rosterRead) setPeopleCount(members.filter((member) => member.subject_type === 'user').length)
    } catch {
      // The count is shown only when it can be read; the share does not wait for it.
    }
  }

  async function share(): Promise<void> {
    const who = currentActor()
    if (!who || !canShare()) return
    const content = trimmed()
    const room = spaceRef()
    if (attempted === null || attempted.content !== content || attempted.spaceRef !== room) {
      requestId = newRequestId()
      attempted = { content, spaceRef: room }
    }
    setSharing(true)
    setFailure(null)
    try {
      setShared(
        await shareMemoryToRoom(who, {
          memoryId: props.memoryId,
          spaceRef: room,
          reviewedContent: content,
          clientRequestId: requestId,
        }),
      )
    } catch (error) {
      if (hasErrorCode(error, 'zdr_durable_mutation_forbidden')) setRefusedByZdr(true)
      setFailure(memoryRoomErrorMessage(error, i18n.tr, 'share', SHARE_FALLBACK))
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
    const who = currentActor()
    if (!copy || !who || taking() || takenBack()) return
    // One id per copy: a retry of the same take-back is the same request, so the room answers it once.
    if (takeBackId === null) takeBackId = newRequestId()
    const clientRequestId = takeBackId
    setTaking(true)
    setTakeBackFailure(null)
    try {
      const result = await takeMemoryBackFromRoom(who, {
        memoryId: copy.roomMemoryId,
        spaceRef: copy.spaceRef,
        clientRequestId,
      })
      if (result.deleted) {
        setTakenBack(true)
      } else {
        setTakeBackFailure(i18n.tr(TAKE_BACK_FALLBACK.no, TAKE_BACK_FALLBACK.en))
      }
    } catch (error) {
      if (hasErrorCode(error, 'memory_not_found')) {
        setTakenBack(true)
      } else {
        setTakeBackFailure(memoryRoomErrorMessage(error, i18n.tr, 'take_back', TAKE_BACK_FALLBACK))
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
                'Alle i rommet du velger kan se teksten, og agentene i rommet kan bruke den i svarene sine. Den deles som deg selv, slik den står her. Ditt eget minne endres ikke.',
                'Everyone in the room you choose can see this text, and the room’s agents can use it in their answers. It is shared as you, exactly as written here. Your own memory does not change.',
              )}
        </p>

        <Show when={shared() === null}>
          <label class="verevon-space-share__label" for="memory-declassify-room">
            {i18n.tr('Rommet', 'Room')}
          </label>
          <select
            id="memory-declassify-room"
            class="verevon-space-share__room"
            disabled={sharing()}
            onChange={(event) => void pickRoom(event.currentTarget.value)}
          >
            <option value="" selected={spaceRef() === ''}>
              {rooms() === null
                ? i18n.tr('Laster rommene dine …', 'Loading your rooms …')
                : i18n.tr('Velg et rom …', 'Choose a room …')}
            </option>
            <For each={roomList()}>
              {(room) => (
                <option value={room.space_ref} selected={room.space_ref === spaceRef()}>
                  {room.name || room.space_ref}
                </option>
              )}
            </For>
          </select>
          <Show when={roomsFailure()}>
            {(message) => (
              <p class="verevon-space-share__error" role="alert">
                {message()}
              </p>
            )}
          </Show>
          <Show when={rooms() !== null && roomList().length === 0 && roomsFailure() === null}>
            <p class="verevon-space-share__count">
              {i18n.tr('Du er ikke med i noen delte rom.', 'You are not in any shared rooms.')}
            </p>
          </Show>
          <Show when={peopleCount() !== null && spaceRef() !== ''}>
            <p class="verevon-space-share__count" role="status">
              {peopleCount() === 1
                ? i18n.tr('1 person i rommet kan se teksten.', '1 person in the room can see this text.')
                : i18n.tr(
                    `${peopleCount()} personer i rommet kan se teksten.`,
                    `${peopleCount()} people in the room can see this text.`,
                  )}
            </p>
          </Show>
        </Show>

        <Show when={shared() !== null && sharedRoomName() !== ''}>
          <p class="verevon-space-share__count">
            {i18n.tr('Rom', 'Room')}: {sharedRoomName()}
          </p>
        </Show>

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
