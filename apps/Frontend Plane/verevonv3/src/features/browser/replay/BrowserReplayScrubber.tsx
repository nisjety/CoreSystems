import { createEffect, createSignal, onCleanup } from 'solid-js'
import type { JSX } from '@solidjs/web'
import { Pause, Play } from '@/shared/icons'
import { useI18n } from '@/shared/i18n'
import {
  autoPlayStartPosition,
  nextAutoPlayPosition,
  REPLAY_AUTO_PLAY_INTERVAL_MS,
  scrubPositionFor,
  scrubStepAt,
  type ReplayScrubStep,
} from './browser-replay-scrubber'
import './browser-replay-scrubber.css'

/**
 * Slider and auto-play over the recorded steps of the browser timeline (F-t3-050 R1).
 * Moving the slider or playing the steps only chooses which recorded step the chrome
 * shows. Playback ends on Pause, after the last step, when a step or the live page is
 * picked somewhere else, and when the scrubber unmounts.
 */
export function BrowserReplayScrubber(props: {
  steps: readonly ReplayScrubStep[]
  /** The recorded step on screen, or null for the live page. */
  selectedStep: number | null
  onSeek: (step: number) => void
}): JSX.Element {
  const i18n = useI18n()
  const [playing, setPlaying] = createSignal(false)
  let timer: ReturnType<typeof setInterval> | undefined
  // The step this scrubber last asked for. Playback continues from it, and a selection that
  // differs from it was picked somewhere else.
  let lastSought: number | null = null

  const position = () => scrubPositionFor(props.steps, props.selectedStep)
  const statusText = () => {
    const step = props.steps[position()]?.step
    if (step === undefined) return ''
    return `${i18n.tr('Steg', 'Step')} #${step} · ${position() + 1} ${i18n.tr('av', 'of')} ${props.steps.length}`
  }
  const playLabel = () => (playing()
    ? i18n.tr('Stans avspillingen', 'Pause the replay')
    : i18n.tr('Spill av stegene', 'Play the steps'))

  const stopPlayback = () => {
    if (timer !== undefined) clearInterval(timer)
    timer = undefined
    setPlaying(false)
  }
  onCleanup(() => {
    if (timer !== undefined) clearInterval(timer)
    timer = undefined
  })

  const seek = (target: number) => {
    const step = scrubStepAt(props.steps, target)
    if (step === null) return
    lastSought = step
    props.onSeek(step)
  }

  createEffect(
    () => props.selectedStep,
    (selected) => {
      if (timer !== undefined && selected !== lastSought) stopPlayback()
    },
  )

  const togglePlayback = () => {
    if (timer !== undefined) {
      stopPlayback()
      return
    }
    const start = autoPlayStartPosition(props.steps.length, position())
    if (start === null) return
    seek(start)
    timer = setInterval(() => {
      const next = nextAutoPlayPosition(props.steps.length, scrubPositionFor(props.steps, lastSought))
      if (next === null) stopPlayback()
      else seek(next)
    }, REPLAY_AUTO_PLAY_INTERVAL_MS)
    setPlaying(true)
  }

  const pickPosition = (value: string) => {
    stopPlayback()
    seek(Number(value))
  }

  return (
    <div class="browser-replay-scrubber" role="group" aria-label={i18n.tr('Avspilling av steg', 'Step replay')}>
      <button type="button" class="browser-replay-scrubber__play" aria-label={playLabel()} onClick={togglePlayback}>
        {playing() ? <Pause class="size-3.5" aria-hidden="true" /> : <Play class="size-3.5" aria-hidden="true" />}
      </button>
      <input
        type="range"
        class="browser-replay-scrubber__slider"
        min={0}
        max={Math.max(props.steps.length - 1, 0)}
        step={1}
        value={position()}
        aria-label={i18n.tr('Velg steg i avspillingen', 'Choose a step to replay')}
        aria-valuetext={statusText()}
        onInput={(event) => pickPosition(event.currentTarget.value)}
      />
      <span class="browser-replay-scrubber__status">{statusText()}</span>
    </div>
  )
}
