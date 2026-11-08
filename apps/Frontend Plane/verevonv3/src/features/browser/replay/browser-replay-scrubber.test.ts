import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  autoPlayStartPosition,
  nextAutoPlayPosition,
  REPLAY_AUTO_PLAY_INTERVAL_MS,
  replayFallbackFrame,
  replayScrubberAvailable,
  replayScrubberEnabled,
  replayScrubSteps,
  scrubPositionFor,
  scrubStepAt,
} from './browser-replay-scrubber'

afterEach(() => {
  vi.unstubAllEnvs()
})

describe('replayScrubberEnabled', () => {
  it('is off by default and only the literal true turns it on', () => {
    expect(replayScrubberEnabled()).toBe(false)
    vi.stubEnv('VITE_BROWSER_REPLAY_SCRUBBER', '1')
    expect(replayScrubberEnabled()).toBe(false)
    vi.stubEnv('VITE_BROWSER_REPLAY_SCRUBBER', 'false')
    expect(replayScrubberEnabled()).toBe(false)
    vi.stubEnv('VITE_BROWSER_REPLAY_SCRUBBER', 'true')
    expect(replayScrubberEnabled()).toBe(true)
  })
})

describe('replayScrubSteps', () => {
  it('keeps every recorded step, the opening one included, in ascending order', () => {
    const steps = replayScrubSteps([{ step: 2 }, { step: 0 }, { step: 1 }])
    expect(steps.map((entry) => entry.step)).toEqual([0, 1, 2])
  })

  it('keeps the first entry for a repeated step and drops steps that are not valid', () => {
    const entries = [
      { step: 1, title: 'first' },
      { step: 1, title: 'second' },
      { step: -1, title: 'negative' },
      { step: 1.5, title: 'fraction' },
    ]
    expect(replayScrubSteps(entries)).toEqual([{ step: 1, title: 'first' }])
  })

  it('returns an empty list for an empty timeline', () => {
    expect(replayScrubSteps([])).toEqual([])
  })
})

describe('replayScrubberAvailable', () => {
  it('needs two recorded steps to move between', () => {
    expect(replayScrubberAvailable([])).toBe(false)
    expect(replayScrubberAvailable([{ step: 0 }])).toBe(false)
    expect(replayScrubberAvailable([{ step: 0 }, { step: 1 }])).toBe(true)
  })
})

describe('scrubPositionFor', () => {
  const steps = [{ step: 0 }, { step: 3 }, { step: 4 }]

  it('puts the live page and a step that is not recorded on the last position', () => {
    expect(scrubPositionFor(steps, null)).toBe(2)
    expect(scrubPositionFor(steps, 9)).toBe(2)
  })

  it('puts a recorded step on its own position', () => {
    expect(scrubPositionFor(steps, 0)).toBe(0)
    expect(scrubPositionFor(steps, 3)).toBe(1)
  })

  it('is 0 with nothing recorded', () => {
    expect(scrubPositionFor([], null)).toBe(0)
  })
})

describe('scrubStepAt', () => {
  const steps = [{ step: 0 }, { step: 3 }, { step: 4 }]

  it('maps a position to its recorded step', () => {
    expect(scrubStepAt(steps, 1)).toBe(3)
  })

  it('clamps a position that is out of range', () => {
    expect(scrubStepAt(steps, -4)).toBe(0)
    expect(scrubStepAt(steps, 99)).toBe(4)
    expect(scrubStepAt(steps, Number.NaN)).toBe(0)
  })

  it('is null with nothing recorded', () => {
    expect(scrubStepAt([], 0)).toBeNull()
  })
})

describe('auto-play positions', () => {
  it('starts from the current position, or from the first step when the current one is the last', () => {
    expect(autoPlayStartPosition(3, 1)).toBe(1)
    expect(autoPlayStartPosition(3, 2)).toBe(0)
  })

  it('has nothing to play with fewer than two steps', () => {
    expect(autoPlayStartPosition(1, 0)).toBeNull()
    expect(autoPlayStartPosition(0, 0)).toBeNull()
  })

  it('moves one step forward and stops after the last step', () => {
    expect(nextAutoPlayPosition(3, 0)).toBe(1)
    expect(nextAutoPlayPosition(3, 1)).toBe(2)
    expect(nextAutoPlayPosition(3, 2)).toBeNull()
  })

  it('shows each step for the same interval', () => {
    expect(REPLAY_AUTO_PLAY_INTERVAL_MS).toBe(1500)
  })
})

describe('replayFallbackFrame', () => {
  it('falls back to the session frame only when no recorded step is scrubbed', () => {
    expect(replayFallbackFrame(false, '/frame.png')).toBe('/frame.png')
    expect(replayFallbackFrame(true, '/frame.png')).toBeNull()
    expect(replayFallbackFrame(false, null)).toBeNull()
    expect(replayFallbackFrame(false, undefined)).toBeNull()
  })
})
