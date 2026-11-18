import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
import { ROOM_RUN_MAX_STEPS, ROOM_RUN_STEP_CLASSES } from './spaces-client/run-work'

// WS09-S5: the SPA keeps its own copy of the step classes and the step cap (run-work.ts). The golden
// that Model Plane's projector is built against is the authority; this test fails when the two drift.
const golden = JSON.parse(
  readFileSync(join(__dirname, 'contracts', 'space-room-run-frames.v1.json'), 'utf8'),
) as { classes: string[]; limits: { max_steps: number } }

describe('space-room-run-frames golden (WS09-S5)', () => {
  it('ROOM_RUN_STEP_CLASSES equals the golden classes list, in the same order', () => {
    expect([...ROOM_RUN_STEP_CLASSES]).toEqual(golden.classes)
  })

  it('caps a run at max_steps 64, the same as the golden', () => {
    expect(golden.limits.max_steps).toBe(64)
    expect(ROOM_RUN_MAX_STEPS).toBe(golden.limits.max_steps)
  })
})
