import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  cancelFinetuneJob,
  createFinetuneJob,
  deployFinetuneJob,
  getFinetuneJob,
  listFinetuneJobs,
  uploadFinetuneTrainingFile,
  type CreateFinetuneJobInput,
  type FinetuneJob,
} from './finetune-client'

function jsonResponse(data: unknown): Response {
  return new Response(JSON.stringify({ data }), {
    status: 200,
    headers: { 'Content-Type': 'application/json' },
  })
}

function sampleJob(overrides: Partial<FinetuneJob> = {}): FinetuneJob {
  return {
    job_id: 'job_1',
    org_id: 'org_1',
    agent_id: 'agent_1',
    base_model: 'gpt-4o-mini',
    azure_file_id: 'file_1',
    azure_job_id: 'ftjob_1',
    fine_tuned_model: '',
    deployment_name: '',
    status: 'queued',
    error_message: '',
    hyperparameters: '{"n_epochs":3}',
    training_example_count: 120,
    estimated_cost_usd: 4.2,
    actual_cost_usd: 0,
    created_by: 'user_1',
    created_at: '2026-06-16T10:00:00Z',
    updated_at: '2026-06-16T10:00:00Z',
    completed_at: null,
    ...overrides,
  }
}

describe('finetune API client', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('lists jobs with filters and org context, handling the {jobs} envelope', async () => {
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse({ jobs: [sampleJob()] }))
    vi.stubGlobal('fetch', fetchMock)

    const result = await listFinetuneJobs('org_1', { agent_id: 'agent_1', status: 'running', limit: 25, offset: 0 })

    expect(result).toHaveLength(1)
    expect(result[0]!.job_id).toBe('job_1')
    const [path, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(path).toBe('/api/v1/finetune/jobs?agent_id=agent_1&status=running&limit=25&offset=0')
    expect((init.headers as Headers).get('x-verevon-org-id')).toBe('org_1')
  })

  it('lists jobs when the endpoint returns a bare array', async () => {
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse([sampleJob(), sampleJob({ job_id: 'job_2' })]))
    vi.stubGlobal('fetch', fetchMock)

    const result = await listFinetuneJobs('org_1')

    expect(result.map((job) => job.job_id)).toEqual(['job_1', 'job_2'])
    const [path] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(path).toBe('/api/v1/finetune/jobs')
  })

  it('fetches a single job', async () => {
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse(sampleJob()))
    vi.stubGlobal('fetch', fetchMock)

    const result = await getFinetuneJob('org_1', 'job_1')

    expect(result.job_id).toBe('job_1')
    const [path, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(path).toBe('/api/v1/finetune/jobs/job_1')
    expect((init.headers as Headers).get('x-verevon-org-id')).toBe('org_1')
  })

  it('creates a job without mutating caller input', async () => {
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse(sampleJob()))
    vi.stubGlobal('fetch', fetchMock)
    const input: CreateFinetuneJobInput = {
      agent_id: 'agent_1',
      base_model: 'gpt-4o-mini',
      azure_file_id: 'file_1',
      hyperparameters: { n_epochs: 3 },
    }

    await createFinetuneJob('org_1', input)

    const [path, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(path).toBe('/api/v1/finetune/jobs')
    expect(init.method).toBe('POST')
    expect((init.headers as Headers).get('x-verevon-org-id')).toBe('org_1')
    expect(JSON.parse(init.body as string)).toMatchObject({
      agent_id: 'agent_1',
      base_model: 'gpt-4o-mini',
      azure_file_id: 'file_1',
      hyperparameters: { n_epochs: 3 },
    })
    expect(input.agent_id).toBe('agent_1')
    expect(input.hyperparameters).toEqual({ n_epochs: 3 })
  })

  it('uploads a training file as multipart with field name file', async () => {
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse({ azure_file_id: 'file_99', training_example_count: 42 }))
    vi.stubGlobal('fetch', fetchMock)
    const file = new File(['{"messages":[]}\n'], 'train.jsonl', { type: 'application/jsonl' })

    const result = await uploadFinetuneTrainingFile('org_1', file)

    expect(result.azure_file_id).toBe('file_99')
    const [path, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(path).toBe('/api/v1/finetune/jobs/upload')
    expect(init.method).toBe('POST')
    expect((init.headers as Headers).get('x-verevon-org-id')).toBe('org_1')
    // Browser sets the multipart boundary itself — Content-Type must be absent.
    expect((init.headers as Headers).get('Content-Type')).toBeNull()
    const form = init.body as FormData
    expect(form).toBeInstanceOf(FormData)
    const uploaded = form.get('file') as File
    expect(uploaded.name).toBe('train.jsonl')
  })

  it('cancels a job via DELETE', async () => {
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse(sampleJob({ status: 'cancelled' })))
    vi.stubGlobal('fetch', fetchMock)

    const result = await cancelFinetuneJob('org_1', 'job_1')

    expect(result.status).toBe('cancelled')
    const [path, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(path).toBe('/api/v1/finetune/jobs/job_1')
    expect(init.method).toBe('DELETE')
    expect((init.headers as Headers).get('x-verevon-org-id')).toBe('org_1')
  })

  it('deploys a job to production by default, returning the updated job', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      jsonResponse(sampleJob({ status: 'succeeded', deployment_tier: 'production' })),
    )
    vi.stubGlobal('fetch', fetchMock)

    const result = await deployFinetuneJob('org_1', 'job_1')

    expect(result.deployment_tier).toBe('production')
    const [path, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(path).toBe('/api/v1/finetune/jobs/job_1/deploy')
    expect(init.method).toBe('POST')
    expect((init.headers as Headers).get('x-verevon-org-id')).toBe('org_1')
    expect(JSON.parse(init.body as string)).toEqual({ tier: 'production' })
  })

  it('deploys a job to an explicit tier', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      jsonResponse(sampleJob({ deployment_tier: 'developer' })),
    )
    vi.stubGlobal('fetch', fetchMock)

    await deployFinetuneJob('org_1', 'job_1', 'developer')

    const [, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(JSON.parse(init.body as string)).toEqual({ tier: 'developer' })
  })
})
