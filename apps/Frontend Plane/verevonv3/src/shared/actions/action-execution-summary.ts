import type { ActionExecution } from '@/shared/actions/types'

/**
 * Keeps an owner-issued domain operation distinct from a Model run. `runId`
 * remains a compatibility field while legacy dispatchers migrate, but it is
 * never the label source when the owner supplied an operation receipt.
 */
export function actionExecutionSummary(execution: ActionExecution): string {
  if (execution.operationId) {
    return `Operation ${execution.operationId} is ${execution.status}. Audit ${execution.auditEventId ?? execution.auditId} recorded.`
  }
  return `Run ${execution.runId} is ${execution.status}. Audit ${execution.auditId} reserved.`
}
