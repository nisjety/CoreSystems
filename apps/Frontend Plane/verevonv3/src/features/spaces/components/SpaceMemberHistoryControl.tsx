import { History, Loader2 } from '@/shared/icons'
import { createSignal, onCleanup, Show } from 'solid-js'

import {
  getSpaceMemberHistoryWindow,
  isMemberHistoryMode,
  setSpaceMemberHistoryWindow,
  type MemberHistoryMode,
  type SpaceMemberHistoryWindow,
  type SpaceRosterMember,
} from '@/shared/api/spaces-client'
import { useI18n } from '@/shared/i18n'

import {
  isMemberWindowConflict,
  localDayErrorMessage,
  localDayValue,
  memberHistoryErrorMessage,
  memberWindowConflictMessage,
  memberWindowLabel,
  startOfLocalDay,
  todayDayValue,
} from '../lib/space-history-window'
import './space-history-window.css'

export interface SpaceMemberHistoryControlProps {
  readonly spaceRef: string
  readonly member: SpaceRosterMember
  /** Called with the window Control confirmed. The roster carries no window, so nothing in it needs re-reading. */
  readonly onChanged?: (window: SpaceMemberHistoryWindow) => void
}

// One id per control, so two rows never share a radio group or a label target.
let controlCount = 0

/**
 * D18: the history control of one roster row, for a room owner or manager (the page offers it only then, behind
 * `features.member_history_windows`). It reads the member's window from Control when opened, offers the three
 * choices (full history, from a date, since they joined) and sends the one chosen with the revision it read.
 *
 * A window only HIDES: it narrows what an already-authorised member reads, never widens it, and Control owns
 * the rule, the audit row and the floor every read signs. This sends a mode and an instant and nothing else; it
 * fetches and filters no content. Presentation only: Control re-checks the role on every call, so hiding the
 * control from anyone else is courtesy, not security.
 *
 * A stale revision answers 409 `room_member_changed`: the window is read again and the person is told, so the
 * choice on screen is never one made against a window that has since moved.
 */
export function SpaceMemberHistoryControl(props: SpaceMemberHistoryControlProps) {
  const i18n = useI18n()
  const id = `space-member-history-${(controlCount += 1)}`
  const name = () => props.member.display_name?.trim() || props.member.subject_id

  const [open, setOpen] = createSignal(false)
  // The window Control last confirmed, by a read or a change. The draft below starts from it.
  const [current, setCurrent] = createSignal<SpaceMemberHistoryWindow | undefined>(undefined)
  const [loading, setLoading] = createSignal(false)
  const [loadError, setLoadError] = createSignal('')
  const [mode, setMode] = createSignal<MemberHistoryMode | undefined>(undefined)
  const [day, setDay] = createSignal('')
  const [busy, setBusy] = createSignal(false)
  const [error, setError] = createSignal('')
  const [notice, setNotice] = createSignal<'saved' | 'unchanged' | ''>('')

  // A read answered after the panel was closed, reopened or removed is not an answer for the one on screen.
  let readSeq = 0
  onCleanup(() => {
    readSeq += 1
  })

  /** Takes a confirmed window as both the current value and the draft: the radio, and the day of an instant. */
  function adopt(next: SpaceMemberHistoryWindow) {
    setCurrent(next)
    setMode(isMemberHistoryMode(next.mode) ? next.mode : undefined)
    setDay(localDayValue(next.sinceAt))
  }

  async function load(): Promise<void> {
    const seq = (readSeq += 1)
    setLoading(true)
    setLoadError('')
    try {
      const read = await getSpaceMemberHistoryWindow(props.spaceRef, props.member.subject_id)
      if (seq !== readSeq) return
      adopt(read.window)
    } catch (err) {
      if (seq !== readSeq) return
      setLoadError(memberHistoryErrorMessage(err, i18n, 'read'))
    } finally {
      if (seq === readSeq) setLoading(false)
    }
  }

  function toggle() {
    if (open()) {
      setOpen(false)
      return
    }
    setOpen(true)
    setError('')
    setNotice('')
    void load()
  }

  function choose(next: MemberHistoryMode) {
    setMode(next)
    setError('')
    setNotice('')
  }

  // Nothing to save until the draft differs from what Control confirmed. A member who follows the room is shown the
  // room's mode, so the same choice is not a change either: the room already decides that way.
  const dirty = () => {
    const base = current()
    const chosen = mode()
    if (!base || !chosen) return false
    if (base.mode !== chosen) return true
    return chosen === 'since_date' && day() !== localDayValue(base.sinceAt)
  }

  async function save(event: Event) {
    event.preventDefault()
    const base = current()
    const chosen = mode()
    if (busy() || !base || !chosen || !dirty()) return
    let sinceAt: string | undefined
    if (chosen === 'since_date') {
      // The start of the chosen local day, as an instant. A day after today is refused here, before any call.
      const start = startOfLocalDay(day())
      if (!start.ok) {
        setError(localDayErrorMessage(start.reason, i18n))
        setNotice('')
        return
      }
      sinceAt = start.instant
    }
    setBusy(true)
    setError('')
    setNotice('')
    try {
      const result = await setSpaceMemberHistoryWindow(props.spaceRef, props.member.subject_id, {
        mode: chosen,
        sinceAt,
        expectedRevision: base.revision,
      })
      adopt(result.window)
      setNotice(result.changed ? 'saved' : 'unchanged')
      props.onChanged?.(result.window)
    } catch (err) {
      if (isMemberWindowConflict(err)) {
        // The window moved since it was read: read it again, THEN say so, so the sentence outlives the reload.
        await load()
        setError(memberWindowConflictMessage(i18n))
      } else {
        setError(memberHistoryErrorMessage(err, i18n, 'change'))
      }
    } finally {
      setBusy(false)
    }
  }

  const sourceLabel = (held: SpaceMemberHistoryWindow) =>
    held.source === 'member'
      ? i18n.tr('satt for dette medlemmet', 'set for this member')
      : i18n.tr('følger rommets innstilling', 'follows the room setting')

  return (
    <>
      <button
        type="button"
        class="verevon-space-agent__control"
        aria-expanded={open() ? 'true' : 'false'}
        aria-controls={open() ? id : undefined}
        aria-label={i18n.tr(`Historikk for ${name()}`, `History for ${name()}`)}
        onClick={toggle}
      >
        <History size={14} aria-hidden="true" />
        {i18n.tr('Historikk', 'History')}
      </button>

      <Show when={open()}>
        <div id={id} class="verevon-space-member-history__panel">
          <Show when={loading() && !current()}>
            <p class="verevon-space-member-history__note" role="status">
              <Loader2 size={14} aria-hidden="true" />
              {i18n.tr('Henter historikk …', 'Loading history…')}
            </p>
          </Show>

          {/* A window that could not be read is not "full history": nothing is offered until it is read. */}
          <Show when={loadError()}>
            {(message) => (
              <>
                <p class="verevon-space-projection-error" role="alert">{message()}</p>
                <div class="verevon-space-member-history__actions">
                  <button
                    type="button"
                    class="verevon-space-agent__control"
                    disabled={loading()}
                    onClick={() => void load()}
                  >
                    {i18n.tr('Prøv igjen', 'Try again')}
                  </button>
                </div>
              </>
            )}
          </Show>

          <Show when={current()}>
            {(held) => (
              // `max` keeps the picker from offering a future day, but the browser's own validation bubble would speak
              // its language, not the reader's: the form is not validated natively, and a refused day is worded below.
              <form novalidate onSubmit={(event) => void save(event)}>
                <fieldset class="verevon-space-member-history__group" disabled={busy()}>
                  <legend>
                    {i18n.tr(`Hvilke meldinger kan ${name()} lese?`, `Which messages can ${name()} read?`)}
                  </legend>
                  <p class="verevon-space-member-history__note">
                    {i18n.tr(
                      `Nå: ${memberWindowLabel(held(), i18n)} (${sourceLabel(held())})`,
                      `Now: ${memberWindowLabel(held(), i18n)} (${sourceLabel(held())})`,
                    )}
                  </p>

                  <div class="verevon-space-member-history__choice">
                    <input
                      id={`${id}-full`}
                      type="radio"
                      name={`${id}-mode`}
                      aria-describedby={`${id}-full-hint`}
                      checked={mode() === 'full'}
                      onChange={() => choose('full')}
                    />
                    <div>
                      <label for={`${id}-full`}>{i18n.tr('All historikk', 'Full history')}</label>
                      <span id={`${id}-full-hint`} class="verevon-space-member-history__hint">
                        {i18n.tr(
                          'Alt som er sagt i rommet, også før de ble med.',
                          'Everything said in the room, also before they joined.',
                        )}
                      </span>
                    </div>
                  </div>

                  <div class="verevon-space-member-history__choice">
                    <input
                      id={`${id}-date`}
                      type="radio"
                      name={`${id}-mode`}
                      aria-describedby={`${id}-date-hint`}
                      checked={mode() === 'since_date'}
                      onChange={() => choose('since_date')}
                    />
                    <div>
                      <label for={`${id}-date`}>{i18n.tr('Fra en dato', 'From a date')}</label>
                      <span id={`${id}-date-hint`} class="verevon-space-member-history__hint">
                        {i18n.tr(
                          'Meldinger fra starten av den dagen, i din tidssone.',
                          'Messages from the start of that day, in your time zone.',
                        )}
                      </span>
                      <input
                        type="date"
                        class="verevon-space-member-history__date"
                        value={day()}
                        max={todayDayValue()}
                        disabled={mode() !== 'since_date'}
                        aria-label={i18n.tr(`Dato for ${name()}`, `Date for ${name()}`)}
                        onInput={(event) => {
                          setDay(event.currentTarget.value)
                          setError('')
                          setNotice('')
                        }}
                      />
                    </div>
                  </div>

                  <div class="verevon-space-member-history__choice">
                    <input
                      id={`${id}-join`}
                      type="radio"
                      name={`${id}-mode`}
                      aria-describedby={`${id}-join-hint`}
                      checked={mode() === 'since_join'}
                      onChange={() => choose('since_join')}
                    />
                    <div>
                      <label for={`${id}-join`}>{i18n.tr('Siden de ble med', 'Since they joined')}</label>
                      <span id={`${id}-join-hint`} class="verevon-space-member-history__hint">
                        {i18n.tr('Bare det som sies etter at de ble med.', 'Only what is said after they joined.')}
                      </span>
                    </div>
                  </div>
                </fieldset>

                <Show when={error()}>
                  {(message) => (
                    <p class="verevon-space-projection-error" role="alert">{message()}</p>
                  )}
                </Show>
                <Show when={notice()}>
                  {(kind) => (
                    <p class="verevon-space-member-history__note" role="status">
                      {kind() === 'saved'
                        ? i18n.tr('Lagret.', 'Saved.')
                        : i18n.tr('Allerede satt slik. Ingenting er endret.', 'Already set this way. Nothing was changed.')}
                    </p>
                  )}
                </Show>
                <div class="verevon-space-member-history__actions">
                  <button
                    type="submit"
                    class="verevon-space-agent__control"
                    disabled={busy() || !dirty()}
                  >
                    {busy() ? i18n.tr('Lagrer...', 'Saving...') : i18n.tr('Lagre', 'Save')}
                  </button>
                </div>
              </form>
            )}
          </Show>
        </div>
      </Show>
    </>
  )
}
