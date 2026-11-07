// @vitest-environment jsdom
import { fireEvent, render } from '@solidjs/testing-library'
import { createSignal, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { BrowserReplayScrubber } from './BrowserReplayScrubber'
import { REPLAY_AUTO_PLAY_INTERVAL_MS, type ReplayScrubStep } from './browser-replay-scrubber'

const steps: ReplayScrubStep[] = [{ step: 0 }, { step: 1 }, { step: 2 }]

const slider = (container: HTMLElement) =>
  container.querySelector<HTMLInputElement>('.browser-replay-scrubber__slider') as HTMLInputElement
const playButton = (container: HTMLElement) =>
  container.querySelector<HTMLButtonElement>('.browser-replay-scrubber__play') as HTMLButtonElement
const statusText = (container: HTMLElement) =>
  container.querySelector<HTMLElement>('.browser-replay-scrubber__status')?.textContent ?? ''
const seekedSteps = (onSeek: ReturnType<typeof vi.fn>) => onSeek.mock.calls.map(([step]) => step)

/** A parent that keeps the selected step, the way the browser chrome does. `pick` changes it from outside. */
function renderWithSelection(initial: number | null, onSeek: (step: number) => void) {
  let setSelected: (step: number | null) => void = () => undefined
  const view = render(() => {
    const [selected, set] = createSignal<number | null>(initial)
    setSelected = set
    return (
      <BrowserReplayScrubber
        steps={steps}
        selectedStep={selected()}
        onSeek={(step) => {
          onSeek(step)
          set(step)
        }}
      />
    )
  })
  return {
    ...view,
    pick: (step: number | null) => {
      setSelected(step)
      flush()
    },
  }
}

describe('BrowserReplayScrubber', () => {
  beforeEach(() => {
    vi.useFakeTimers()
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('sits on the last step while the live page is on screen', () => {
    const view = renderWithSelection(null, vi.fn())
    expect(slider(view.container).max).toBe('2')
    expect(slider(view.container).value).toBe('2')
    expect(statusText(view.container)).toContain('#2')
    expect(statusText(view.container)).toContain('3 av 3')
  })

  it('shows the position of a recorded step on the slider', () => {
    const view = renderWithSelection(1, vi.fn())
    expect(slider(view.container).value).toBe('1')
    expect(statusText(view.container)).toContain('#1')
  })

  it('a slider pick seeks to that recorded step', () => {
    const onSeek = vi.fn()
    const view = renderWithSelection(null, onSeek)
    fireEvent.input(slider(view.container), { target: { value: '0' } })
    flush()
    expect(onSeek).toHaveBeenLastCalledWith(0)
    expect(statusText(view.container)).toContain('#0')
  })

  it('plays the recorded steps in order on a fixed interval and stops after the last one', () => {
    const onSeek = vi.fn()
    const view = renderWithSelection(null, onSeek)
    const playLabel = playButton(view.container).getAttribute('aria-label')

    // The live page sits on the last step, so Play starts again from the first recorded step.
    fireEvent.click(playButton(view.container))
    flush()
    expect(seekedSteps(onSeek)).toEqual([0])
    expect(playButton(view.container).getAttribute('aria-label')).not.toBe(playLabel)

    vi.advanceTimersByTime(REPLAY_AUTO_PLAY_INTERVAL_MS)
    flush()
    expect(seekedSteps(onSeek)).toEqual([0, 1])
    vi.advanceTimersByTime(REPLAY_AUTO_PLAY_INTERVAL_MS)
    flush()
    expect(seekedSteps(onSeek)).toEqual([0, 1, 2])

    vi.advanceTimersByTime(REPLAY_AUTO_PLAY_INTERVAL_MS)
    flush()
    expect(seekedSteps(onSeek)).toEqual([0, 1, 2])
    expect(playButton(view.container).getAttribute('aria-label')).toBe(playLabel)
  })

  it('Play on a middle step continues forward from that step', () => {
    const onSeek = vi.fn()
    const view = renderWithSelection(1, onSeek)
    fireEvent.click(playButton(view.container))
    vi.advanceTimersByTime(REPLAY_AUTO_PLAY_INTERVAL_MS)
    flush()
    expect(seekedSteps(onSeek)).toEqual([1, 2])
  })

  it('a slider pick while playing stops the playback', () => {
    const onSeek = vi.fn()
    const view = renderWithSelection(null, onSeek)
    fireEvent.click(playButton(view.container))
    vi.advanceTimersByTime(REPLAY_AUTO_PLAY_INTERVAL_MS)
    fireEvent.input(slider(view.container), { target: { value: '2' } })
    flush()
    const callsAfterPick = onSeek.mock.calls.length

    vi.advanceTimersByTime(REPLAY_AUTO_PLAY_INTERVAL_MS * 3)
    flush()
    expect(onSeek).toHaveBeenCalledTimes(callsAfterPick)
    expect(seekedSteps(onSeek).at(-1)).toBe(2)
  })

  it('a step picked somewhere else in the chrome ends the playback', () => {
    const onSeek = vi.fn()
    const view = renderWithSelection(null, onSeek)
    const playLabel = playButton(view.container).getAttribute('aria-label')
    fireEvent.click(playButton(view.container))
    flush()
    const callsBeforePick = onSeek.mock.calls.length

    view.pick(2)
    vi.advanceTimersByTime(REPLAY_AUTO_PLAY_INTERVAL_MS * 3)
    flush()
    expect(onSeek).toHaveBeenCalledTimes(callsBeforePick)
    expect(playButton(view.container).getAttribute('aria-label')).toBe(playLabel)
  })

  it('returning to the live page ends the playback', () => {
    const onSeek = vi.fn()
    const view = renderWithSelection(0, onSeek)
    fireEvent.click(playButton(view.container))
    flush()
    const callsBeforeLive = onSeek.mock.calls.length

    view.pick(null)
    vi.advanceTimersByTime(REPLAY_AUTO_PLAY_INTERVAL_MS * 2)
    flush()
    expect(onSeek).toHaveBeenCalledTimes(callsBeforeLive)
  })

  it('Pause stops the playback', () => {
    const onSeek = vi.fn()
    const view = renderWithSelection(0, onSeek)
    fireEvent.click(playButton(view.container))
    fireEvent.click(playButton(view.container))
    const calls = onSeek.mock.calls.length

    vi.advanceTimersByTime(REPLAY_AUTO_PLAY_INTERVAL_MS * 3)
    expect(onSeek).toHaveBeenCalledTimes(calls)
  })

  it('stops its timer when it unmounts', () => {
    const onSeek = vi.fn()
    const view = renderWithSelection(null, onSeek)
    fireEvent.click(playButton(view.container))
    const calls = onSeek.mock.calls.length

    view.unmount()
    vi.advanceTimersByTime(REPLAY_AUTO_PLAY_INTERVAL_MS * 3)
    expect(onSeek).toHaveBeenCalledTimes(calls)
  })

  it('has nothing to play with a single recorded step', () => {
    const onSeek = vi.fn()
    const view = render(() => <BrowserReplayScrubber steps={[{ step: 0 }]} selectedStep={null} onSeek={onSeek} />)
    fireEvent.click(playButton(view.container))
    vi.advanceTimersByTime(REPLAY_AUTO_PLAY_INTERVAL_MS * 2)
    expect(onSeek).not.toHaveBeenCalled()
  })
})
