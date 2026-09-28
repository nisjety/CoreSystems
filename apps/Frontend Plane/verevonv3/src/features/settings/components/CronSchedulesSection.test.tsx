// @vitest-environment jsdom

// chat-wave2 "schedule" spec §1/§2.1 — the pre-existing workflow-type defect:
// of the seven workflow types this form used to offer, capability-core's
// `workflow_dispatcher.go::reauthorizeScheduleFire` hard-rejects every scoped
// scheduled fire whose `workflow_type` is not exactly
// `InteractiveRunSupervision`. The fix stops OFFERING the six that can never
// fire. This file pins that fix at the UI layer — it had no coverage at all
// before this review.

import { render, screen, within } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'

vi.mock('@/shared/session/session-store', () => ({
  getSession: () => ({
    status: 'authenticated',
    user: { id: 'user-1', email: 'admin@example.com', name: 'Admin', emailVerified: true, role: 'admin' },
    activeOrg: { id: 'org-1', name: 'Test org', role: 'owner' },
    permissions: [],
    onboardingStatus: 'COMPLETED',
  }),
}))

vi.mock('@/shared/api/cron-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/cron-client')>()
  return {
    ...actual,
    createCronSchedule: vi.fn(),
    deleteCronSchedule: vi.fn(),
    listCronSchedules: vi.fn(),
    updateCronSchedule: vi.fn(),
  }
})

import { listCronSchedules } from '@/shared/api/cron-client'
import { CronSchedulesSection } from './CronSchedulesSection'

const mockedList = vi.mocked(listCronSchedules)

afterEach(() => {
  vi.clearAllMocks()
})

describe('CronSchedulesSection — workflow-type defect fix', () => {
  it('offers only InteractiveRunSupervision in the create form, not the six types that can never fire', async () => {
    mockedList.mockResolvedValue({ schedules: [] })

    const { container } = render(() => <CronSchedulesSection />)

    await screen.findByText('Workflow')
    const select = container.querySelector('#cron-workflow') as HTMLSelectElement
    expect(select).toBeTruthy()
    const options = within(select).getAllByRole('option') as HTMLOptionElement[]

    expect(options).toHaveLength(1)
    expect(options[0]?.value).toBe('InteractiveRunSupervision')

    // The six types `workflow_dispatcher.go` rejects for a scoped scheduled
    // fire must never be selectable again.
    for (const broken of [
      'DeepTaskWorkflow',
      'WideResearchWorkflow',
      'EvaluatorOptimizerWorkflow',
      'MemoryConsolidationWorkflow',
      'SkillPromotionWorkflow',
      'FeedbackPromotionWorkflow',
    ]) {
      expect(screen.queryByText(new RegExp(broken))).toBeNull()
    }
  })

  it('still displays an existing schedule\'s real (broken) workflow type in the list rather than silently relabeling it', async () => {
    mockedList.mockResolvedValue({
      schedules: [
        {
          id: 'cron_1',
          org_id: 'org-1',
          name: 'Old broken schedule',
          description: '',
          schedule_expr: '0 9 * * *',
          timezone: 'UTC',
          task_template: { kind: 'cron', workflow_type: 'DeepTaskWorkflow' },
          enabled: true,
        },
      ],
    })

    render(() => <CronSchedulesSection />)

    expect(await screen.findByText('DeepTaskWorkflow')).toBeTruthy()
  })
})
