// WS09-S8: a room manager or owner stops another member's live agent run.
//
//   POST /api/v1/spaces/{space_ref}/runs/{run_id}/stop   {idempotency_key?}
//     -> 200 { data: { run_id, status: 'cancelled' | 'already_terminal', cancelled, receipt_id } }
//
// Errors arrive as an ApiError with the typed envelope: 403 `space_run_control_not_authorized` (the
// caller is below manager, or the organisation has not enabled stopping other members' runs),
// 404 `run_not_found` / `feature_unavailable`, 412 `zdr_durable_mutation_forbidden`, 503
// `run_control_unavailable`. Presentation only: Control decides on every call and Session Core
// verifies the decision again. The same operation is the action `spaces.stop_run`.
import { requestJson } from '../http'

export type SpaceRunStopReceipt = {
  readonly runId: string
  /** `already_terminal` means the run had ended before this stop arrived (nothing was cancelled). */
  readonly status: 'cancelled' | 'already_terminal'
  readonly cancelled: boolean
  readonly receiptId: string
}

type StopWire = {
  run_id?: unknown
  status?: unknown
  cancelled?: unknown
  receipt_id?: unknown
}

/** Stop one live run in a room. One idempotency key names one stop attempt; a retry reuses it. */
export async function stopSpaceRun(
  spaceRef: string,
  runId: string,
  idempotencyKey?: string,
): Promise<SpaceRunStopReceipt> {
  const wire = await requestJson<StopWire>(
    `/api/v1/spaces/${encodeURIComponent(spaceRef)}/runs/${encodeURIComponent(runId)}/stop`,
    {
      method: 'POST',
      body: JSON.stringify(idempotencyKey ? { idempotency_key: idempotencyKey } : {}),
    },
  )
  return {
    runId: typeof wire?.run_id === 'string' && wire.run_id ? wire.run_id : runId,
    status: wire?.status === 'already_terminal' ? 'already_terminal' : 'cancelled',
    cancelled: wire?.cancelled === true,
    receiptId: typeof wire?.receipt_id === 'string' ? wire.receipt_id : '',
  }
}
