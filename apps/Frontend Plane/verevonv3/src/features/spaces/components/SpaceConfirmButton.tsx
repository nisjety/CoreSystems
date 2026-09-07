import { createSignal, onCleanup, Show } from 'solid-js'

import { useI18n } from '@/shared/i18n'

/**
 * A destructive action that asks in place, rather than through `window.confirm`.
 *
 * The room used a native confirm for its three destructive actions. That dialog
 * cannot be styled or laid out with the rest of the page, blocks the whole
 * browser while it is open, and — the reason it matters here — states the
 * consequence in a box that looks nothing like the product, at the moment the
 * reader most needs to trust what they are being told. The settings surface
 * already answers this by switching the button's own label to "Confirm", so
 * this is that pattern made reusable.
 *
 * # It disarms itself
 *
 * An armed button that stays armed is a trap: a reader who clicks once, reads
 * the consequence, decides against it and comes back to the page later finds a
 * button that now deletes on the first click. This reverts after
 * `DISARM_AFTER_MS` and on blur, so the armed state never outlives the moment
 * of attention that created it.
 */
const DISARM_AFTER_MS = 6_000

export interface SpaceConfirmButtonProps {
  /** What the button does, in its resting state — e.g. "Remove from room". */
  readonly label: string
  /** The consequence, shown only once armed. One sentence, plain. */
  readonly consequence: string
  /** Accessible name while armed, so a screen reader hears what is confirmed. */
  readonly confirmLabel: string
  readonly onConfirm: () => void
  readonly disabled?: boolean
  readonly class?: string
}

export function SpaceConfirmButton(props: SpaceConfirmButtonProps) {
  const i18n = useI18n()
  const [armed, setArmed] = createSignal(false)
  let timer: number | undefined

  const disarm = () => {
    if (timer !== undefined) {
      window.clearTimeout(timer)
      timer = undefined
    }
    setArmed(false)
  }
  onCleanup(disarm)

  const click = () => {
    if (props.disabled) return
    if (armed()) {
      disarm()
      props.onConfirm()
      return
    }
    setArmed(true)
    timer = window.setTimeout(disarm, DISARM_AFTER_MS)
  }

  return (
    <span class="verevon-space-confirm">
      <button
        type="button"
        class={props.class}
        disabled={props.disabled}
        aria-label={armed() ? props.confirmLabel : undefined}
        onClick={click}
        onBlur={disarm}
      >
        {armed() ? i18n.tr('Bekreft', 'Confirm') : props.label}
      </button>
      {/* The consequence appears with the armed state and is announced, so the
          answer to "what happens if I press this again" is on screen at the
          moment the question arises. */}
      <Show when={armed()}>
        <span class="verevon-space-confirm__consequence" role="status">
          {props.consequence}
        </span>
      </Show>
    </span>
  )
}
