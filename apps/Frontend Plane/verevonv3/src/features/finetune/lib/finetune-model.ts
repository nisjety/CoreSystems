import type {
  FinetuneDeploymentTier,
  FinetuneJob,
  FinetuneJobStatus,
} from '@/shared/api/finetune-client'

export type FinetuneStatusTone = 'queued' | 'running' | 'ok' | 'error' | 'neutral'

/** Fallback rates when the backend omits the pricing fields. */
export const DEVELOPER_HOSTING_USD_PER_HOUR = 0
export const DEVELOPER_AUTO_DELETE_HOURS = 24
export const PRODUCTION_HOSTING_USD_PER_HOUR = 2.0

/** Read the configured production hourly rate, defaulting to 2.00. */
export function productionHourlyRate(job?: Partial<FinetuneJob>): number {
  const rate = job?.production_hosting_usd_per_hour
  return Number.isFinite(rate) && (rate as number) >= 0
    ? (rate as number)
    : PRODUCTION_HOSTING_USD_PER_HOUR
}

/** Read the configured developer hourly rate, defaulting to 0. */
export function developerHourlyRate(job?: Partial<FinetuneJob>): number {
  const rate = job?.developer_hosting_usd_per_hour
  return Number.isFinite(rate) && (rate as number) >= 0
    ? (rate as number)
    : DEVELOPER_HOSTING_USD_PER_HOUR
}

/** Read the developer auto-delete window in hours, defaulting to 24. */
export function developerAutoDeleteHours(job?: Partial<FinetuneJob>): number {
  const hours = job?.developer_auto_delete_hours
  return Number.isFinite(hours) && (hours as number) > 0
    ? (hours as number)
    : DEVELOPER_AUTO_DELETE_HOURS
}

/** Resolve a job's deployment tier, defaulting to `developer` (test). */
export function deploymentTier(job?: Partial<FinetuneJob>): FinetuneDeploymentTier {
  return job?.deployment_tier === 'production' ? 'production' : 'developer'
}

/** Short badge label for a deployment tier. */
export function deploymentTierLabel(tier: FinetuneDeploymentTier): string {
  return tier === 'production' ? 'Production' : 'Test'
}

/** Format an hourly hosting rate (e.g. `$2.00/hr`, or `Free` at zero). */
export function formatHourlyRate(usdPerHour: number): string {
  if (!Number.isFinite(usdPerHour) || usdPerHour <= 0) return 'Free'
  return `$${usdPerHour.toFixed(2)}/hr`
}

const STATUS_TONES: Record<FinetuneJobStatus, FinetuneStatusTone> = {
  queued: 'queued',
  running: 'running',
  succeeded: 'ok',
  failed: 'error',
  cancelled: 'neutral',
}

const STATUS_LABELS: Record<FinetuneJobStatus, string> = {
  queued: 'Queued',
  running: 'Running',
  succeeded: 'Succeeded',
  failed: 'Failed',
  cancelled: 'Cancelled',
}

export function finetuneStatusTone(status: FinetuneJobStatus): FinetuneStatusTone {
  return STATUS_TONES[status] ?? 'neutral'
}

export function finetuneStatusLabel(status: FinetuneJobStatus): string {
  return STATUS_LABELS[status] ?? status
}

/** Terminal jobs can no longer be cancelled. */
export function isCancellableStatus(status: FinetuneJobStatus): boolean {
  return status === 'queued' || status === 'running'
}

/** Active jobs warrant continued polling. */
export function hasActiveJobs(jobs: readonly FinetuneJob[]): boolean {
  return jobs.some((job) => job.status === 'queued' || job.status === 'running')
}

export function formatCost(usd: number): string {
  if (!Number.isFinite(usd) || usd <= 0) return '—'
  return `$${usd.toFixed(2)}`
}

export function formatTimestamp(value: string | null | undefined): string {
  if (!value) return '—'
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) return value
  return date.toLocaleString()
}

/** Build the secondary detail line for a job row. */
export function jobDetail(job: FinetuneJob): string {
  const parts: string[] = [`Base ${job.base_model || '—'}`]
  if (job.training_example_count > 0) parts.push(`${job.training_example_count} examples`)
  const actual = formatCost(job.actual_cost_usd)
  const estimated = formatCost(job.estimated_cost_usd)
  if (actual !== '—') parts.push(`cost ${actual}`)
  else if (estimated !== '—') parts.push(`est. ${estimated}`)
  if (job.fine_tuned_model) parts.push(job.fine_tuned_model)
  return parts.join(' · ')
}
