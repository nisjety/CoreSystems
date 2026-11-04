import { describe, expect, it } from 'vitest'
import { actionRegistry, getActionDescriptor } from '@/shared/actions/action-registry'
import { isModelExecutableAction } from '@/shared/actions/model-eligibility'
import { MODEL_ACCESS } from '@/shared/actions/model-access'

// WS15.L5: the contract comes first. These pin the descriptors the gateway dispatchers
// (W2-FE-L5b) and the SPA build against.

function descriptor(id: string) {
  const found = getActionDescriptor(id as never)
  if (!found) throw new Error(`${id} is not registered`)
  return found
}

function access(id: string) {
  return MODEL_ACCESS[id as keyof typeof MODEL_ACCESS]
}

describe('ingestions run control actions (WS15.L5)', () => {
  it('registers status, pause, resume and cancel with the planned risk and approval', () => {
    expect(descriptor('ingestions.get_run_status')).toMatchObject({ risk: 'low', requiresApproval: false, ownerPlane: 'ingestion' })
    expect(descriptor('ingestions.pause_run')).toMatchObject({ risk: 'medium', requiresApproval: false, reversible: true })
    expect(descriptor('ingestions.resume_run')).toMatchObject({ risk: 'medium', requiresApproval: false, reversible: true })
    expect(descriptor('ingestions.cancel_run')).toMatchObject({ risk: 'medium', requiresApproval: true, reversible: false })
  })

  it('takes a run id and an optional crawl kind, and refuses anything else', () => {
    for (const id of ['get_run_status', 'pause_run', 'resume_run', 'cancel_run']) {
      const schema = descriptor(`ingestions.${id}`).inputSchema
      expect(schema.safeParse({ runId: 'job_1' }).success, id).toBe(true)
      expect(schema.safeParse({ runId: 'job_1', kind: 'crawl' }).success, id).toBe(true)
      expect(schema.safeParse({ runId: '  ' }).success, id).toBe(false)
      expect(schema.safeParse({}).success, id).toBe(false)
      expect(schema.safeParse({ runId: 'job_1', kind: 'scrape' }).success, id).toBe(false)
    }
  })

  it('keeps every new action closed to the model, and the status read a read', () => {
    for (const id of ['get_run_status', 'pause_run', 'resume_run', 'cancel_run', 'pause_schedule', 'change_schedule_state']) {
      expect(isModelExecutableAction(`ingestions.${id}`), id).toBe(false)
      expect(access(`ingestions.${id}`).access, id).toBe('none')
    }
    expect(access('ingestions.get_run_status').sideEffectClass).toBe('read')
  })
})

describe('ingestions schedule state actions (WS15.L5)', () => {
  it('splits the old run_schedule_action into pause_schedule and change_schedule_state', () => {
    const ids = actionRegistry.map((action) => action.id as string)
    expect(ids).not.toContain('ingestions.run_schedule_action')
    expect(ids).toContain('ingestions.pause_schedule')
    expect(ids).toContain('ingestions.change_schedule_state')
  })

  it('lets a pause through without approval and gates every other state change', () => {
    expect(descriptor('ingestions.pause_schedule')).toMatchObject({ requiresApproval: false, reversible: true })
    expect(descriptor('ingestions.change_schedule_state')).toMatchObject({ requiresApproval: true, reversible: false })
    expect(descriptor('ingestions.pause_schedule').inputSchema.safeParse({ scheduleId: 'sched_1' }).success).toBe(true)
    expect(descriptor('ingestions.pause_schedule').inputSchema.safeParse({ scheduleId: '' }).success).toBe(false)
  })

  it('accepts unpause, trigger, backfill and delete, and a backfill only with its window', () => {
    const schema = descriptor('ingestions.change_schedule_state').inputSchema
    for (const action of ['unpause_schedule', 'trigger_schedule', 'delete_schedule']) {
      expect(schema.safeParse({ action, scheduleId: 'sched_1' }).success, action).toBe(true)
    }
    expect(schema.safeParse({ action: 'pause_schedule', scheduleId: 'sched_1' }).success).toBe(false)
    expect(schema.safeParse({ action: 'backfill_schedule', scheduleId: 'sched_1' }).success).toBe(false)
    expect(
      schema.safeParse({
        action: 'backfill_schedule',
        scheduleId: 'sched_1',
        startAt: '2026-10-01T00:00:00Z',
        endAt: '2026-10-02T00:00:00Z',
      }).success,
    ).toBe(true)
  })
})

describe('ingestions.create_schedule input (WS15.L5)', () => {
  const schema = () => descriptor('ingestions.create_schedule').inputSchema

  it('still takes a plain crawl schedule and a one-time scheduleAt', () => {
    expect(schema().safeParse({ name: 'Docs', kind: 'crawl', targetUrl: 'https://example.com', cron: '0 7 * * *' }).success).toBe(true)
    expect(
      schema().safeParse({ name: 'Once', kind: 'scrape', targetUrl: 'https://example.com', scheduleAt: '2026-12-01T07:00:00Z' }).success,
    ).toBe(true)
    expect(schema().safeParse({ name: 'No time', kind: 'crawl', targetUrl: 'https://example.com' }).success).toBe(false)
  })

  it('carries maxPages and an overlap policy from the owner vocabulary', () => {
    const base = { name: 'Docs', kind: 'crawl', targetUrl: 'https://example.com', cron: '0 7 * * *' }
    expect(schema().safeParse({ ...base, maxPages: 40, overlapPolicy: 'cancel' }).success).toBe(true)
    expect(schema().safeParse({ ...base, overlapPolicy: 'buffer_one' }).success).toBe(false)
    expect(schema().safeParse({ ...base, maxPages: 0 }).success).toBe(false)
    expect(schema().safeParse({ ...base, maxPages: 1.5 }).success).toBe(false)
  })

  it('needs urls for a batch schedule and a targetUrl for every other kind', () => {
    const when = { name: 'Batch', cron: '0 7 * * *' }
    expect(schema().safeParse({ ...when, kind: 'batch', urls: ['https://a.example/x', 'https://b.example/y'] }).success).toBe(true)
    expect(schema().safeParse({ ...when, kind: 'batch', targetUrl: 'https://a.example/x' }).success).toBe(false)
    expect(schema().safeParse({ ...when, kind: 'batch', urls: [] }).success).toBe(false)
    expect(schema().safeParse({ ...when, kind: 'crawl', urls: ['https://a.example/x'] }).success).toBe(false)
  })

  it('never takes an activate flag: activation is a separate step', () => {
    const result = schema().safeParse({
      name: 'Docs',
      kind: 'crawl',
      targetUrl: 'https://example.com',
      cron: '0 7 * * *',
      activate: true,
    })
    expect(result.success).toBe(false)
  })
})
