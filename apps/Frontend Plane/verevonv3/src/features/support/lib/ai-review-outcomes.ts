type ReviewTimingAction = {
  status: string
  created_at: string
  updated_at: string
}

export type ReviewDecisionTiming = {
  measuredActions: number
  medianMilliseconds: number | null
}

const terminalReviewStatuses = new Set(['approved', 'executed', 'rejected', 'failed'])

/**
 * Measures time from proposal creation to the ledger's terminal recorded
 * review decision/outcome. It does not measure customer delivery, resolution,
 * agent quality, or any model property.
 */
export function deriveReviewDecisionTiming(actions: readonly ReviewTimingAction[]): ReviewDecisionTiming {
  const durations = actions.flatMap((action) => {
    if (!terminalReviewStatuses.has(action.status)) return []
    const created = Date.parse(action.created_at)
    const updated = Date.parse(action.updated_at)
    const duration = updated - created
    return Number.isFinite(created) && Number.isFinite(updated) && duration >= 0 ? [duration] : []
  }).sort((left, right) => left - right)

  if (durations.length === 0) return { measuredActions: 0, medianMilliseconds: null }
  const middle = Math.floor(durations.length / 2)
  const medianMilliseconds = durations.length % 2 === 1
    ? durations[middle]!
    : (durations[middle - 1]! + durations[middle]!) / 2
  return { measuredActions: durations.length, medianMilliseconds }
}

export function formatReviewTiming(milliseconds: number | null): string {
  if (milliseconds === null) return '—'
  const totalMinutes = Math.max(0, Math.round(milliseconds / 60_000))
  if (totalMinutes < 60) return `${totalMinutes} min`
  const hours = Math.floor(totalMinutes / 60)
  const minutes = totalMinutes % 60
  return minutes ? `${hours} h ${minutes} min` : `${hours} h`
}
