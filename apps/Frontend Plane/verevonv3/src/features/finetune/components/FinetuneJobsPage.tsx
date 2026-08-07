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
import { useI18n } from '@/shared/i18n'

function modelOptions(models: ModelInfo[]): Array<{ value: string; label: string }> {
  return models.map((model) => ({ value: model.id, label: model.name || model.id }))
}

const finetuneQueryKeys = {
  jobs: (orgId: string) => ['finetune', 'jobs', orgId] as const,
}

export default function FinetuneJobsPage() {
  const i18n = useI18n()
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
    return [{ value: '', label: i18n.tr('Velg en grunnmodell …', 'Select a base model…') }, ...options]
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

  // Returns the invalidate-and-refetch promise (not void) so a catch-block
  // reconciliation (cancelJob) can await the fresh data landing in `jobs()`
  // before asserting failure.
  const refreshJobs = () => queryClient.invalidateQueries({ queryKey: finetuneQueryKeys.jobs(orgId()) })

  const createJob = async () => {
    const id = orgId()
    if (!id) {
      setError(i18n.tr('Ingen aktiv organisasjon.', 'No active organization.'))
      return
    }
    const selectedFile = file()
    if (!selectedFile) {
      setError(i18n.tr('Velg en JSONL-treningsfil først.', 'Choose a JSONL training file first.'))
      return
    }
    if (!agentId().trim()) {
      setError(i18n.tr('Agent-id er påkrevd.', 'Agent id is required.'))
      return
    }
    if (!baseModel()) {
      setError(i18n.tr('Grunnmodell er påkrevd.', 'Base model is required.'))
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
          ? i18n.tr('Fine-tune-jobb satt i kø for en produksjonsutrulling.', 'Fine-tune job queued for a production deployment.')
          : i18n.tr('Fine-tune-jobb satt i kø som en gratis 24-timers test.', 'Fine-tune job queued as a free 24h test.'),
      )
      setFile(null)
      void refreshJobs()
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : i18n.tr('Kunne ikke opprette fine-tune-jobb.', 'Could not create fine-tune job.'))
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
      setNotice(i18n.tr('Fine-tune-jobb kansellert.', 'Fine-tune job cancelled.'))
      void refreshJobs()
    } catch (reason) {
      // cancelFinetuneJob can fail (e.g. a transient 502) after the
      // cancellation was already durably recorded server-side. Re-fetch and
      // check the job's real status before asserting failure, instead of
      // trusting the network error alone — otherwise a user sees a false
      // "could not cancel" error for a cancellation that already went
      // through.
      let reconciled = true
      try {
        await refreshJobs()
      } catch {
        reconciled = false
      }
      const latest = jobs().find((job) => job.job_id === jobId)
      if (reconciled && latest?.status === 'cancelled') {
        setNotice(i18n.tr('Fine-tune-jobb kansellert.', 'Fine-tune job cancelled.'))
      } else if (reconciled && latest) {
        setError(reason instanceof Error ? reason.message : i18n.tr('Kunne ikke kansellere fine-tune-jobb.', 'Could not cancel fine-tune job.'))
      } else {
        setError(i18n.tr(
          'Vi fikk ikke bekreftet om fine-tune-jobben ble kansellert. Vent litt før du prøver på nytt.',
          "We couldn't confirm whether the fine-tune job was cancelled. Please wait a moment before trying again.",
        ))
      }
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
      setNotice(i18n.tr('Modell forfremmet til en produksjonsutrulling.', 'Model promoted to a production deployment.'))
      void refreshJobs()
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : i18n.tr('Kunne ikke forfremme fine-tune til produksjon.', 'Could not promote fine-tune to production.'))
    } finally {
      setDeployingId(null)
    }
  }

  return (
    <SettingsSurface contentVariant="workspace">
      <SettingsHero
        eyebrow="Admin"
        title={i18n.tr('Fine-tune-jobber', 'Fine-tune jobs')}
        description={i18n.tr('Administrer Azure-modell-finjustering: last opp overvåkede JSONL-eksempler, start jobber og følg status.', 'Manage Azure model fine-tuning: upload supervised JSONL examples, launch jobs, and track status.')}
      />

      <section class="verevon-finetune-pricing" aria-label={i18n.tr('Fine-tune-priser og nivåer', 'Fine-tune pricing and tiers')}>
        <p class="verevon-finetune-pricing__framing">
          {i18n.tr('Ditt ', 'Your ')}<strong>{i18n.tr('RAG-kunnskapssystem er alltid på og standard', 'RAG knowledge system is always on and standard')}</strong>
          {i18n.tr(' — ingen fine-tune-hostingkostnad. ', ' — no fine-tune hosting charge. ')}<strong>{i18n.tr('Fine-tuning er en valgfri forbedring', 'Fine-tuning is an optional enhancement')}</strong>
          {i18n.tr(': test den gratis i ', ': test it free for ')}{developerAutoDelete()}{i18n.tr('t, kjør den ', 'h, run it ')}<strong>{i18n.tr('sammen med', 'alongside')}</strong>
          {i18n.tr(' RAG (en kombinasjon), og behold en produksjonsutrulling bare hvis den vinner for ditt bruksområde.', ' RAG (a combination), and keep a production deployment only if it wins for your use case.')}
        </p>
        <div class="verevon-finetune-pricing__tiers">
          <div
            class="verevon-finetune-pricing__tier verevon-finetune-pricing__tier--developer"
            data-recommended="true"
          >
            <div class="verevon-finetune-pricing__tier-head">
              <span class="verevon-finetune-pricing__tier-name">{i18n.tr('Utvikler (Test)', 'Developer (Test)')}</span>
              <span class="verevon-finetune-pricing__tier-flag">{i18n.tr('Standard · Anbefalt', 'Default · Recommended')}</span>
            </div>
            <p class="verevon-finetune-pricing__tier-price">
              <strong>{i18n.tr('$0/t', '$0/hr')}</strong> {i18n.tr('hosting', 'hosting')}
            </p>
            <p class="verevon-finetune-pricing__tier-note">
              {i18n.tr('Slettes automatisk om ', 'Auto-deletes in ')}<strong>{developerAutoDelete()}{i18n.tr('t', 'h')}</strong>{i18n.tr(' · for evaluering og proof-of-concept.', ' · for evaluation & proof-of-concept.')}
            </p>
          </div>
          <div class="verevon-finetune-pricing__tier verevon-finetune-pricing__tier--production">
            <div class="verevon-finetune-pricing__tier-head">
              <span class="verevon-finetune-pricing__tier-name">{i18n.tr('Produksjon', 'Production')}</span>
              <span class="verevon-finetune-pricing__tier-flag verevon-finetune-pricing__tier-flag--optin">
                {i18n.tr('Eksplisitt tilvalg', 'Explicit opt-in')}
              </span>
            </div>
            <p class="verevon-finetune-pricing__tier-price">
              <strong>{productionRateLabel()}</strong> {i18n.tr('hosting + per-token-inferens', 'hosting + per-token inference')}
            </p>
            <p class="verevon-finetune-pricing__tier-note">
              {i18n.tr('Samme inferensrate som grunnmodellen · faktureres time for time mens utrullet (selv når inaktiv).', 'Same inference rate as the base model · billed hourly while deployed (even when idle).')}
            </p>
          </div>
        </div>
      </section>

      <section class="verevon-finetune">
        <div class="verevon-finetune__create">
          <SectionHeader
            title={i18n.tr('Ny fine-tune-jobb', 'New fine-tune job')}
            description={i18n.tr('Last opp en JSONL-treningsfil og start en jobb mot en grunnmodell.', 'Upload a JSONL training file and launch a job against a base model.')}
          />
          <div class="verevon-settings-field-grid">
            <SettingsField
              id="finetune-agent-id"
              label={i18n.tr('Agent-id', 'Agent id')}
              value={agentId()}
              onInput={(event) => setAgentId(event.currentTarget.value)}
            />
            <SettingsSelect
              id="finetune-base-model"
              label={i18n.tr('Grunnmodell', 'Base model')}
              value={baseModel()}
              options={baseModelOptions()}
              onChange={(event) => setBaseModel(event.currentTarget.value)}
            />
          </div>
          <label for="finetune-file" class="verevon-settings-field">
            <span class="verevon-settings-label">{i18n.tr('Treningsfil (JSONL)', 'Training file (JSONL)')}</span>
            <input
              id="finetune-file"
              type="file"
              accept=".jsonl,application/jsonl,application/json"
              class="verevon-finetune__file"
              onChange={(event) => setFile(event.currentTarget.files?.[0] ?? null)}
            />
            <Show when={file()}>
              {(selected) => <span class="verevon-settings-help">{selected().name}</span>}
            </Show>
          </label>

          <fieldset class="verevon-finetune-tier" aria-describedby="finetune-tier-help">
            <legend class="verevon-settings-label">{i18n.tr('Utrulling', 'Deployment')}</legend>
            <div class="verevon-finetune-tier__options" role="radiogroup" aria-label={i18n.tr('Utrullingsnivå', 'Deployment tier')}>
              <button
                type="button"
                role="radio"
                aria-checked={tier() === 'developer'}
                class="verevon-finetune-tier__option"
                data-active={tier() === 'developer'}
                onClick={() => setTier('developer')}
              >
                <span class="verevon-finetune-tier__option-title">Test</span>
                <span class="verevon-finetune-tier__option-sub">{i18n.tr(`Gratis · slettes automatisk om ${developerAutoDelete()}t`, `Free · auto-deletes in ${developerAutoDelete()}h`)}</span>
              </button>
              <button
                type="button"
                role="radio"
                aria-checked={tier() === 'production'}
                class="verevon-finetune-tier__option"
                data-active={tier() === 'production'}
                onClick={() => setTier('production')}
              >
                <span class="verevon-finetune-tier__option-title">{i18n.tr('Produksjon', 'Production')}</span>
                <span class="verevon-finetune-tier__option-sub">{i18n.tr(`${productionRateLabel()} + inferens`, `${productionRateLabel()} + inference`)}</span>
              </button>
            </div>
            <span id="finetune-tier-help" class="verevon-settings-help">
              <Show
                when={tier() === 'production'}
                fallback={i18n.tr(`Anbefalt for å prøve en modell: gratis, fjernes automatisk etter ${developerAutoDelete()}t.`, `Recommended for trying a model: free, removed automatically after ${developerAutoDelete()}h.`)}
              >
                {i18n.tr(`Faktureres ${productionRateLabel()} mens utrullet (selv når inaktiv), pluss per-token-inferens til grunnmodell-raten.`, `Billed ${productionRateLabel()} while deployed (even when idle), plus per-token inference at the base-model rate.`)}
              </Show>
            </span>
          </fieldset>

          <div class="verevon-settings-actions">
            <Show when={notice()} fallback={<span />}>
              {(message) => (
                <p class="verevon-settings-status-message verevon-settings-status-message--success" role="status">
                  {message()}
                </p>
              )}
            </Show>
            <Show when={error()}>
              {(message) => (
                <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
                  {message()}
                </p>
              )}
            </Show>
            <SettingsButton variant="primary" disabled={busy()} onClick={() => void createJob()}>
              {busy() ? i18n.tr('Starter …', 'Launching…') : i18n.tr('Start fine-tune', 'Launch fine-tune')}
            </SettingsButton>
          </div>
        </div>

        <SectionHeader title={i18n.tr('Jobber', 'Jobs')} description={i18n.tr('Nylige fine-tune-jobber for denne organisasjonen.', 'Recent fine-tune jobs for this organization.')} />
        <Show
          when={!jobsQuery.isLoading}
          fallback={<p class="verevon-finetune__loading">{i18n.tr('Laster jobber …', 'Loading jobs…')}</p>}
        >
          <Show
            when={jobs().length > 0}
            fallback={<p class="verevon-finetune__empty">{i18n.tr('Ingen fine-tune-jobber ennå.', 'No fine-tune jobs yet.')}</p>}
          >
            <div class="verevon-settings-list-card">
              <For each={jobs()}>
                {(job) => (
                  <div class="verevon-finetune__row">
                    <div class="verevon-finetune__row-main">
                      <p>{job.agent_id || job.job_id}</p>
                      <span>{jobDetail(job)}</span>
                      <Show when={job.status === 'failed' && job.error_message}>
                        <span class="verevon-finetune__error">{job.error_message}</span>
                      </Show>
                      <Show when={confirmingDeployId() === job.job_id}>
                        <div class="verevon-finetune__deploy-confirm" role="group" aria-label={i18n.tr('Bekreft produksjonsforfremmelse', 'Confirm production promotion')}>
                          <p class="verevon-finetune__deploy-cost">
                            {i18n.tr('Produksjonshosting: ', 'Production hosting: ')}<strong>{productionRateLabel()}</strong>{i18n.tr(' + per-token-inferens, faktureres mens utrullet (selv når inaktiv).', ' + per-token inference, billed while deployed (even when idle).')}
                          </p>
                          <p class="verevon-finetune__deploy-note">
                            {i18n.tr(`Din nåværende Test-utrulling er gratis og slettes automatisk om ${developerAutoDelete()}t.`, `Your current Test deployment is free and auto-deletes in ${developerAutoDelete()}h.`)}
                          </p>
                          <div class="verevon-finetune__deploy-actions">
                            <SettingsButton
                              settingsSize="sm"
                              variant="primary"
                              disabled={deployingId() === job.job_id}
                              onClick={() => void promoteJob(job.job_id)}
                            >
                              {deployingId() === job.job_id ? i18n.tr('Forfremmer …', 'Promoting…') : i18n.tr('Bekreft produksjon', 'Confirm production')}
                            </SettingsButton>
                            <SettingsButton
                              settingsSize="sm"
                              disabled={deployingId() === job.job_id}
                              onClick={() => setConfirmingDeployId(null)}
                            >
                              {i18n.tr('Avbryt', 'Cancel')}
                            </SettingsButton>
                          </div>
                        </div>
                      </Show>
                    </div>
                    <div class="verevon-finetune__row-actions">
                      <span
                        class="verevon-finetune__tier-badge"
                        data-tier={deploymentTier(job)}
                        title={
                          deploymentTier(job) === 'production'
                            ? i18n.tr(`Produksjonsutrulling · ${formatHourlyRate(productionHourlyRate(job))} hosting`, `Production deployment · ${formatHourlyRate(productionHourlyRate(job))} hosting`)
                            : i18n.tr('Test-utrulling · gratis, slettes automatisk', 'Test deployment · free, auto-deletes')
                        }
                      >
                        {deploymentTierLabel(deploymentTier(job))}
                        <Show when={deploymentTier(job) === 'production'}>
                          {' · '}
                          {formatHourlyRate(productionHourlyRate(job))}
                        </Show>
                      </span>
                      <span
                        class="verevon-finetune__badge"
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
                          {i18n.tr('Forfrem til produksjon', 'Promote to production')}
                        </SettingsButton>
                      </Show>
                      <Show when={isCancellableStatus(job.status)}>
                        <SettingsButton
                          settingsSize="sm"
                          danger
                          disabled={cancellingId() === job.job_id}
                          onClick={() => void cancelJob(job.job_id)}
                        >
                          {cancellingId() === job.job_id ? i18n.tr('Kansellerer …', 'Cancelling…') : i18n.tr('Avbryt', 'Cancel')}
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
