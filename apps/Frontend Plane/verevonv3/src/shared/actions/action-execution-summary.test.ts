import { describe, expect, it } from 'vitest'
import { actionExecutionSummary } from './action-execution-summary'

describe('actionExecutionSummary', () => {
  it('renders an owner operation receipt without calling it a Model run', () => {
    expect(actionExecutionSummary({
      actionId: 'tickets.create',
      operationId: 'ticketop_1',
      auditEventId: 'audit_1',
      auditId: 'audit_1',
      eventStream: '',
      runId: 'ticketop_1',
      status: 'completed',
    })).toBe('Operation ticketop_1 is completed. Audit audit_1 recorded.')
  })

  it('retains a true run label for legacy run-based actions', () => {
    expect(actionExecutionSummary({
      actionId: 'agent.run',
      auditId: 'audit_2',
      eventStream: '',
      runId: 'run_2',
      status: 'queued',
    })).toBe('Run run_2 is queued. Audit audit_2 reserved.')
  })
})
