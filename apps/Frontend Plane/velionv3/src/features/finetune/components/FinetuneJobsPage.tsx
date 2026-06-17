import { createQuery, useQueryClient } from '@tanstack/solid-query'
import { createMemo, createSignal, For, Show } from 'solid-js'
import {
  cancelFinetuneJob,
  createFinetuneJob,
  deployFinetuneJob,
  listFinetuneJobs,
  uploadFinetuneTrainingFile,
  type FinetuneDeploymentTier,
  type FinetuneJob,
} from '@/shared/api/finetune-client'
import { listModels, type ModelInfo } from '@/shared/api/chat-client'
import { getSession } from '@/shared/session/session-store'
import {
  SectionHeader,
  SettingsButton,
  SettingsField,
  SettingsHero,
  SettingsSelect,
  SettingsSurface,
} from '@/features/settings/components/settings-ui'
import {
  deploymentTier,
  deploymentTierLabel,
  developerAutoDeleteHours,
  finetuneStatusLabel,
  finetuneStatusTone,
  formatHourlyRate,
  hasActiveJobs,
  isCancellableStatus,
  jobDetail,
  productionHourlyRate,
} from '@/features/finetune/lib/finetune-model'

function modelOptions(models: ModelInfo[]): Array<{ value: string; label: string }> {
  return models.map((model) => ({ value: model.id, label: model.name || model.id }))
}

const finetuneQueryKeys = {
  jobs: (orgId: string) => ['finetune', 'jobs', orgId] as const,
}

export default function FinetuneJobsPage() {
  const session = getSession()
  const queryClient = useQueryClient()
  const orgId = createMemo(() => session.activeOrg?.id ?? '')

  // Fetch the model catalog once for the base-model select (same source as the
  // chat picker). Failure degrades gracefully to an empty list.
  const [modelList, setModelList] = createSignal<ModelInfo[]>([])
  void listModels().then(setModelList).catch(() => setModelList([]))

  const jobsQuery = createQuery(() => {
    const id = orgId()
    return {
      enabled: Boolean(id),
      queryKey: finetuneQueryKeys.jobs(id),
      queryFn: () => listFinetuneJobs(id),
      // Poll while there are active jobs so status transitions surface promptly.
      refetchInterval: (query: { state: { data?: FinetuneJob[] } }) =>
        hasActiveJobs(query.state.data ?? []) ? 4000 : false,
    }
  })

  const jobs = createMemo<FinetuneJob[]>(() => jobsQuery.data ?? [])

  const [agentId, setAgentId] = createSignal('')
  const [baseModel, setBaseModel] = createSignal('')
  const [file, setFile] = createSignal<File | null>(null)
  const [tier, setTier] = createSignal<FinetuneDeploymentTier>('developer')
  const [busy, setBusy] = createSignal(false)
  const [error, setError] = createSignal<string | null>(null)
  const [notice, setNotice] = createSignal<string | null>(null)
  const [cancellingId, setCancellingId] = createSignal<string | null>(null)
  // Inline confirm + busy state for "Promote to production" (per-job).
  const [confirmingDeployId, setConfirmingDeployId] = createSignal<string | null>(null)
  const [deployingId, setDeployingId] = createSignal<string | null>(null)

  const baseModelOptions = createMemo(() => {
    const options = modelOptions(modelList())
    return [{ value: '', label: 'Select a base model…' }, ...options]
  })

  // Production hourly rate is server-configurable: read it from any job envelope
  // that carries it, else fall back to the 2.00 default.
  const productionRate = createMemo(() => {
    const withRate = jobs().find((job) => job.production_hosting_usd_per_hour != null)
    return productionHourlyRate(withRate)
  })
  // Developer auto-delete window, read defensively from job envelopes (else 24h).
  const developerAutoDelete = createMemo(() => {
    const withHours = jobs().find((job) => job.developer_auto_delete_hours != null)
    return developerAutoDeleteHours(withHours)
  })
  const productionRateLabel = createMemo(() => formatHourlyRate(productionRate()))

  const refreshJobs = () => {
    void queryClient.invalidateQueries({ queryKey: finetuneQueryKeys.jobs(orgId()) })
  }

  const createJob = async () => {
    const id = orgId()
    if (!id) {
      setError('No active organization.')
      return
    }
    const selectedFile = file()
    if (!selectedFile) {
      setError('Choose a JSONL training file first.')
      return
    }
    if (!agentId().trim()) {
      setError('Agent id is required.')
      return
    }
    if (!baseModel()) {
      setError('Base model is required.')
      return
    }
    setBusy(true)
    setError(null)
    setNotice(null)
    try {
      const upload = await uploadFinetuneTrainingFile(id, selectedFile)
      await createFinetuneJob(id, {
        agent_id: agentId().trim(),
        base_model: baseModel(),
        azure_file_id: upload.azure_file_id,
        deployment_tier: tier(),
      })
      setNotice(
        tier() === 'production'
          ? 'Fine-tune job queued for a production deployment.'
          : 'Fine-tune job queued as a free 24h test.',
      )
      setFile(null)
      refreshJobs()
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : 'Could not create fine-tune job.')
    } finally {
      setBusy(false)
    }
  }

  const cancelJob = async (jobId: string) => {
    const id = orgId()
    if (!id) return
    setCancellingId(jobId)
    setError(null)
    setNotice(null)
    try {
      await cancelFinetuneJob(id, jobId)
      setNotice('Fine-tune job cancelled.')
      refreshJobs()
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : 'Could not cancel fine-tune job.')
    } finally {
      setCancellingId(null)
    }
  }

  const promoteJob = async (jobId: string) => {
    const id = orgId()
    if (!id) return
    setDeployingId(jobId)
    setError(null)
    setNotice(null)
    try {
      await deployFinetuneJob(id, jobId, 'production')
      setConfirmingDeployId(null)
      setNotice('Model promoted to a production deployment.')
      refreshJobs()
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : 'Could not promote fine-tune to production.')
    } finally {
      setDeployingId(null)
    }
  }

  return (
    <SettingsSurface contentVariant="workspace">
      <SettingsHero
        eyebrow="Admin"
        title="Fine-tune jobs"
        description="Manage Azure model fine-tuning: upload supervised JSONL examples, launch jobs, and track status."
      />

      <section class="velion-finetune-pricing" aria-label="Fine-tune pricing and tiers">
        <p class="velion-finetune-pricing__framing">
          Your <strong>RAG knowledge system is always on and standard</strong> — no fine-tune
          hosting charge. <strong>Fine-tuning is an optional enhancement</strong>: test it free for
          {' '}{developerAutoDelete()}h, run it <strong>alongside</strong> RAG (a combination), and
          keep a production deployment only if it wins for your use case.
        </p>
        <div class="velion-finetune-pricing__tiers">
          <div
            class="velion-finetune-pricing__tier velion-finetune-pricing__tier--developer"
            data-recommended="true"
          >
            <div class="velion-finetune-pricing__tier-head">
              <span class="velion-finetune-pricing__tier-name">Developer (Test)</span>
              <span class="velion-finetune-pricing__tier-flag">Default · Recommended</span>
            </div>
            <p class="velion-finetune-pricing__tier-price">
              <strong>$0/hr</strong> hosting
            </p>
            <p class="velion-finetune-pricing__tier-note">
              Auto-deletes in <strong>{developerAutoDelete()}h</strong> · for evaluation &amp;
              proof-of-concept.
            </p>
          </div>
          <div class="velion-finetune-pricing__tier velion-finetune-pricing__tier--production">
            <div class="velion-finetune-pricing__tier-head">
              <span class="velion-finetune-pricing__tier-name">Production</span>
              <span class="velion-finetune-pricing__tier-flag velion-finetune-pricing__tier-flag--optin">
                Explicit opt-in
              </span>
            </div>
            <p class="velion-finetune-pricing__tier-price">
              <strong>{productionRateLabel()}</strong> hosting + per-token inference
            </p>
            <p class="velion-finetune-pricing__tier-note">
              Same inference rate as the base model · billed hourly while deployed (even when idle).
            </p>
          </div>
        </div>
      </section>

      <section class="velion-finetune">
        <div class="velion-finetune__create">
          <SectionHeader
            title="New fine-tune job"
            description="Upload a JSONL training file and launch a job against a base model."
          />
          <div class="velion-settings-field-grid">
            <SettingsField
              id="finetune-agent-id"
              label="Agent id"
              value={agentId()}
              onInput={(event) => setAgentId(event.currentTarget.value)}
            />
            <SettingsSelect
              id="finetune-base-model"
              label="Base model"
              value={baseModel()}
              options={baseModelOptions()}
              onChange={(event) => setBaseModel(event.currentTarget.value)}
            />
          </div>
          <label for="finetune-file" class="velion-settings-field">
            <span class="velion-settings-label">Training file (JSONL)</span>
            <input
              id="finetune-file"
              type="file"
              accept=".jsonl,application/jsonl,application/json"
              class="velion-finetune__file"
              onChange={(event) => setFile(event.currentTarget.files?.[0] ?? null)}
            />
            <Show when={file()}>
              {(selected) => <span class="velion-settings-help">{selected().name}</span>}
            </Show>
          </label>

          <fieldset class="velion-finetune-tier" aria-describedby="finetune-tier-help">
            <legend class="velion-settings-label">Deployment</legend>
            <div class="velion-finetune-tier__options" role="radiogroup" aria-label="Deployment tier">
              <button
                type="button"
                role="radio"
                aria-checked={tier() === 'developer'}
                class="velion-finetune-tier__option"
                data-active={tier() === 'developer'}
                onClick={() => setTier('developer')}
              >
                <span class="velion-finetune-tier__option-title">Test</span>
                <span class="velion-finetune-tier__option-sub">Free · auto-deletes in {developerAutoDelete()}h</span>
              </button>
              <button
                type="button"
                role="radio"
                aria-checked={tier() === 'production'}
                class="velion-finetune-tier__option"
                data-active={tier() === 'production'}
                onClick={() => setTier('production')}
              >
                <span class="velion-finetune-tier__option-title">Production</span>
                <span class="velion-finetune-tier__option-sub">{productionRateLabel()} + inference</span>
              </button>
            </div>
            <span id="finetune-tier-help" class="velion-settings-help">
              <Show
                when={tier() === 'production'}
                fallback={`Recommended for trying a model: free, removed automatically after ${developerAutoDelete()}h.`}
              >
                {`Billed ${productionRateLabel()} while deployed (even when idle), plus per-token inference at the base-model rate.`}
              </Show>
            </span>
          </fieldset>

          <div class="velion-settings-actions">
            <Show when={notice()} fallback={<span />}>
              {(message) => (
                <p class="velion-settings-status-message velion-settings-status-message--success" role="status">
                  {message()}
                </p>
              )}
            </Show>
            <Show when={error()}>
              {(message) => (
                <p class="velion-settings-status-message velion-settings-status-message--error" role="alert">
                  {message()}
                </p>
              )}
            </Show>
            <SettingsButton variant="primary" disabled={busy()} onClick={() => void createJob()}>
              {busy() ? 'Launching…' : 'Launch fine-tune'}
            </SettingsButton>
          </div>
        </div>

        <SectionHeader title="Jobs" description="Recent fine-tune jobs for this organization." />
        <Show
          when={!jobsQuery.isLoading}
          fallback={<p class="velion-finetune__loading">Loading jobs…</p>}
        >
          <Show
            when={jobs().length > 0}
            fallback={<p class="velion-finetune__empty">No fine-tune jobs yet.</p>}
          >
            <div class="velion-settings-list-card">
              <For each={jobs()}>
                {(job) => (
                  <div class="velion-finetune__row">
                    <div class="velion-finetune__row-main">
                      <p>{job.agent_id || job.job_id}</p>
                      <span>{jobDetail(job)}</span>
                      <Show when={job.status === 'failed' && job.error_message}>
                        <span class="velion-finetune__error">{job.error_message}</span>
                      </Show>
                      <Show when={confirmingDeployId() === job.job_id}>
                        <div class="velion-finetune__deploy-confirm" role="group" aria-label="Confirm production promotion">
                          <p class="velion-finetune__deploy-cost">
                            Production hosting: <strong>{productionRateLabel()}</strong> + per-token
                            inference, billed while deployed (even when idle).
                          </p>
                          <p class="velion-finetune__deploy-note">
                            Your current Test deployment is free and auto-deletes in{' '}
                            {developerAutoDelete()}h.
                          </p>
                          <div class="velion-finetune__deploy-actions">
                            <SettingsButton
                              settingsSize="sm"
                              variant="primary"
                              disabled={deployingId() === job.job_id}
                              onClick={() => void promoteJob(job.job_id)}
                            >
                              {deployingId() === job.job_id ? 'Promoting…' : 'Confirm production'}
                            </SettingsButton>
                            <SettingsButton
                              settingsSize="sm"
                              disabled={deployingId() === job.job_id}
                              onClick={() => setConfirmingDeployId(null)}
                            >
                              Cancel
                            </SettingsButton>
                          </div>
                        </div>
                      </Show>
                    </div>
                    <div class="velion-finetune__row-actions">
                      <span
                        class="velion-finetune__tier-badge"
                        data-tier={deploymentTier(job)}
                        title={
                          deploymentTier(job) === 'production'
                            ? `Production deployment · ${formatHourlyRate(productionHourlyRate(job))} hosting`
                            : 'Test deployment · free, auto-deletes'
                        }
                      >
                        {deploymentTierLabel(deploymentTier(job))}
                        <Show when={deploymentTier(job) === 'production'}>
                          {' · '}
                          {formatHourlyRate(productionHourlyRate(job))}
                        </Show>
                      </span>
                      <span
                        class="velion-finetune__badge"
                        data-tone={finetuneStatusTone(job.status)}
                      >
                        {finetuneStatusLabel(job.status)}
                      </span>
                      <Show
                        when={
                          job.status === 'succeeded' && deploymentTier(job) !== 'production'
                        }
                      >
                        <SettingsButton
                          settingsSize="sm"
                          variant="primary"
                          disabled={
                            confirmingDeployId() === job.job_id || deployingId() === job.job_id
                          }
                          onClick={() => setConfirmingDeployId(job.job_id)}
                        >
                          Promote to production
                        </SettingsButton>
                      </Show>
                      <Show when={isCancellableStatus(job.status)}>
                        <SettingsButton
                          settingsSize="sm"
                          danger
                          disabled={cancellingId() === job.job_id}
                          onClick={() => void cancelJob(job.job_id)}
                        >
                          {cancellingId() === job.job_id ? 'Cancelling…' : 'Cancel'}
                        </SettingsButton>
                      </Show>
                    </div>
                  </div>
                )}
              </For>
            </div>
          </Show>
        </Show>
      </section>
    </SettingsSurface>
  )
}
