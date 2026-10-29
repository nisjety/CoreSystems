import { requestForm, requestJson } from '@/shared/api/http'

// Azure model fine-tuning jobs. The wire shape is snake_case and 1:1 with the
// backend FinetuneJob contract, so we keep snake_case here.

/** Job lifecycle: queued → running → (succeeded|failed|cancelled). */
export type FinetuneJobStatus = 'queued' | 'running' | 'succeeded' | 'failed' | 'cancelled'

/**
 * Deployment intent for a fine-tuned model.
 * - `developer`: free, auto-deletes after a short window — for evaluation.
 * - `production`: explicit opt-in, billed hourly while deployed.
 */
export type FinetuneDeploymentTier = 'developer' | 'production'

export interface FinetuneJob {
  job_id: string
  org_id: string
  agent_id: string
  base_model: string
  azure_file_id: string
  azure_job_id: string
  fine_tuned_model: string
  deployment_name: string
  status: FinetuneJobStatus
  error_message: string
  /** Hyperparameters serialized as a JSON string. */
  hyperparameters: string
  training_example_count: number
  estimated_cost_usd: number
  actual_cost_usd: number
  created_by: string
  created_at: string
  updated_at: string
  completed_at: string | null
  // --- Pricing / tier (all optional; the backend surfaces these per job).
  /** Which tier this deployment uses. Defaults to `developer` when absent. */
  deployment_tier?: FinetuneDeploymentTier
  /** Hosting rate for the developer (test) tier — typically 0. */
  developer_hosting_usd_per_hour?: number
  /** Hours before a developer (test) deployment is auto-deleted — typically 24. */
  developer_auto_delete_hours?: number
  /** Hosting rate for a production deployment — server-configurable, defaults to 2.00. */
  production_hosting_usd_per_hour?: number
}

/** Result of uploading a JSONL training file to Azure. */
export interface FinetuneTrainingFileUpload {
  azure_file_id: string
  training_example_count?: number
}

/** Body for creating a fine-tune job. */
export interface CreateFinetuneJobInput {
  agent_id: string
  base_model: string
  azure_file_id: string
  hyperparameters?: Record<string, unknown>
  /** Deployment intent recorded with the job. Defaults to `developer` (test). */
  deployment_tier?: FinetuneDeploymentTier
}

/** Filters for listing jobs. */
export interface ListFinetuneJobsParams {
  agent_id?: string
  status?: FinetuneJobStatus
  limit?: number
  offset?: number
}

function orgHeaders(orgId: string): HeadersInit {
  return { 'x-verevon-org-id': orgId }
}

/** Normalize the list endpoint, which may return `{ jobs: [...] }` or a bare array. */
function normalizeJobList(raw: unknown): FinetuneJob[] {
  if (Array.isArray(raw)) return raw as FinetuneJob[]
  if (raw && typeof raw === 'object' && Array.isArray((raw as { jobs?: unknown }).jobs)) {
    return (raw as { jobs: FinetuneJob[] }).jobs
  }
  return []
}

/** List fine-tune jobs for the org, optionally filtered. */
export async function listFinetuneJobs(
  orgId: string,
  params: ListFinetuneJobsParams = {},
  signal?: AbortSignal,
): Promise<FinetuneJob[]> {
  const qs = new URLSearchParams()
  if (params.agent_id) qs.set('agent_id', params.agent_id)
  if (params.status) qs.set('status', params.status)
  if (params.limit != null) qs.set('limit', String(params.limit))
  if (params.offset != null) qs.set('offset', String(params.offset))
  const query = qs.toString()
  const raw = await requestJson<unknown>(`/api/v1/finetune/jobs${query ? `?${query}` : ''}`, {
    headers: orgHeaders(orgId),
    signal,
  })
  return normalizeJobList(raw)
}

/** Fetch a single fine-tune job. */
export function getFinetuneJob(
  orgId: string,
  jobId: string,
  signal?: AbortSignal,
): Promise<FinetuneJob> {
  return requestJson<FinetuneJob>(`/api/v1/finetune/jobs/${encodeURIComponent(jobId)}`, {
    headers: orgHeaders(orgId),
    signal,
  })
}

/** Create a fine-tune job from an already-uploaded Azure training file. */
export function createFinetuneJob(
  orgId: string,
  input: CreateFinetuneJobInput,
  signal?: AbortSignal,
): Promise<FinetuneJob> {
  return requestJson<FinetuneJob>('/api/v1/finetune/jobs', {
    method: 'POST',
    body: JSON.stringify(input),
    headers: orgHeaders(orgId),
    signal,
  })
}

/**
 * Upload a JSONL training file (multipart, field name `file`). The browser sets
 * the multipart boundary, so {@link requestForm} omits Content-Type.
 */
export function uploadFinetuneTrainingFile(
  orgId: string,
  file: File,
  signal?: AbortSignal,
): Promise<FinetuneTrainingFileUpload> {
  const form = new FormData()
  form.append('file', file, file.name)
  return requestForm<FinetuneTrainingFileUpload>('/api/v1/finetune/jobs/upload', form, {
    headers: orgHeaders(orgId),
    signal,
  })
}

/** Cancel (DELETE) a fine-tune job. */
export function cancelFinetuneJob(
  orgId: string,
  jobId: string,
  signal?: AbortSignal,
): Promise<FinetuneJob> {
  return requestJson<FinetuneJob>(`/api/v1/finetune/jobs/${encodeURIComponent(jobId)}`, {
    method: 'DELETE',
    headers: orgHeaders(orgId),
    signal,
  })
}

/**
 * Promote a fine-tuned model to a deployment tier (defaults to `production`).
 * The backend returns the updated job with the new `deployment_tier`.
 */
export function deployFinetuneJob(
  orgId: string,
  jobId: string,
  tier: FinetuneDeploymentTier = 'production',
  signal?: AbortSignal,
): Promise<FinetuneJob> {
  return requestJson<FinetuneJob>(`/api/v1/finetune/jobs/${encodeURIComponent(jobId)}/deploy`, {
    method: 'POST',
    body: JSON.stringify({ tier }),
    headers: orgHeaders(orgId),
    signal,
  })
}
