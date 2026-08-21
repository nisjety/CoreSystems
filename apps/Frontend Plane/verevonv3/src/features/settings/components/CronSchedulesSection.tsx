import { CalendarClock, Clock, Loader2, Trash2 } from '@/shared/icons'
import { createMemo, createSignal, For, Show } from 'solid-js'
import { createResource } from '@/shared/lib/create-resource-compat'
import {
  createCronSchedule,
  deleteCronSchedule,
  listCronSchedules,
  updateCronSchedule,
  type CronSchedule,
  type CronTaskTemplate,
} from '@/shared/api/cron-client'
import { translateApiError, useI18n } from '@/shared/i18n'
import { SectionHeader, SettingsButton } from '@/features/settings/components/settings-ui'
import { VerevonInput } from '@/shared/ui/verevon/VerevonInput'
import { getSession } from '@/shared/session/session-store'
import { hasWorkspaceAdminAccess } from '@/shared/session/access'

/**
 * Planlagte kjøringer (cron) — tidsstyrte oppgaver agenten kjører automatisk.
 *
 * Hver plan har et cron-uttrykk («min time dag måned ukedag»). capability-core
 * sin sweeper fyrer forfalte planer: den lager en oppgave fra malen, logger
 * kjøringen og beregner neste kjøring. Bare administratorer kan opprette, endre
 * eller slette planer.
 */
function formatWhen(value?: string | null): string {
  if (!value) return '—'
  try {
    return new Date(value).toLocaleString('nb-NO', { dateStyle: 'short', timeStyle: 'short' })
  } catch {
    return value
  }
}

/**
 * Which workflow a schedule may start, mirroring orchestrator-core's
 * `workflowAllowlist` (internal/orchestration/workflowreg.go). Kept in the same
 * order as that map so the two are easy to diff by eye.
 *
 * `needsInput` is the load-bearing field. When a schedule carries no
 * `workflow_input`, capability-core's dispatcher derives `{goal, policy}` from
 * the task title/description — and only InteractiveRunSupervision can decode
 * that shape. Every other workflow's input decoder uses DisallowUnknownFields
 * and declares no `goal`, so a derived goal is a hard decode error. Offering
 * those types without also collecting their real input would produce schedules
 * that fail on every fire.
 *
 * `deniesZdr` mirrors the allowlist's DeniesZDR flag: orchestrator-core refuses
 * these outright for a Zero-Data-Retention org, so the form says so up front
 * rather than letting the schedule fail at fire time.
 *
 * AutoresearchWorkflow is deliberately absent here because it is absent from
 * the allowlist — its budget guard is driven by a fabricated per-step cost.
 */
interface WorkflowChoice {
  readonly value: string
  readonly label: string
  readonly needsInput: boolean
  readonly deniesZdr: boolean
  /** A minimal input that satisfies this workflow's decoder. */
  readonly inputExample: string
  readonly inputHelp: string
}

/**
 * Named separately from the array so the default and the lookup fallback are
 * provably defined — this project builds with noUncheckedIndexedAccess, so
 * WORKFLOW_CHOICES[0] would otherwise be `WorkflowChoice | undefined`.
 */
const INTERACTIVE_RUN_CHOICE: WorkflowChoice = {
  value: 'InteractiveRunSupervision',
  label: 'InteractiveRunSupervision — vanlig agentkjøring (standard)',
  needsInput: false,
  deniesZdr: false,
  inputExample: '{\n  "goal": "Oppsummer gårsdagens saker"\n}',
  inputHelp:
    'Trenger ingen egen input: målet utledes fra oppgavetittelen og beskrivelsen. Fyll bare ut hvis du vil overstyre.',
}

const WORKFLOW_CHOICES: readonly WorkflowChoice[] = [
  INTERACTIVE_RUN_CHOICE,
  {
    value: 'DeepTaskWorkflow',
    label: 'DeepTaskWorkflow — flere steg etter hverandre',
    needsInput: true,
    deniesZdr: false,
    inputExample: '{\n  "steps": [\n    { "goal": "Hent tallene" },\n    { "goal": "Skriv sammendraget" }\n  ]\n}',
    inputHelp: 'Krever «steps» med minst ett steg, og hvert steg må ha en ikke-tom «goal».',
  },
  {
    value: 'WideResearchWorkflow',
    label: 'WideResearchWorkflow — parallelle søk som flettes',
    needsInput: true,
    deniesZdr: false,
    inputExample: '{\n  "queries": ["marked 2026", "konkurrenter"],\n  "merge_strategy": "dedupe"\n}',
    inputHelp: 'Krever «queries» med minst ett ikke-tomt søk. «merge_strategy»: concat, dedupe eller summarize.',
  },
  {
    value: 'EvaluatorOptimizerWorkflow',
    label: 'EvaluatorOptimizerWorkflow — generer, vurder, forbedre',
    needsInput: true,
    deniesZdr: false,
    inputExample: '{\n  "task": "Skriv ukesrapporten",\n  "rubric": "Presis, kortfattet, med kilder",\n  "max_rounds": 3\n}',
    inputHelp: 'Krever «task» (ikke «goal»). «rubric», «max_rounds» og «pass_threshold» er valgfrie.',
  },
  {
    value: 'MemoryConsolidationWorkflow',
    label: 'MemoryConsolidationWorkflow — rydd i minnet',
    needsInput: true,
    deniesZdr: true,
    inputExample: '{\n  "max_items": 200\n}',
    inputHelp: 'Trenger ingen felter — «{}» er nok. «max_items» begrenser hvor mye som konsolideres per kjøring.',
  },
  {
    value: 'SkillPromotionWorkflow',
    label: 'SkillPromotionWorkflow — forfremme en skill',
    needsInput: true,
    deniesZdr: true,
    inputExample: '{\n  "skill_id": "skill_...",\n  "to_scope": "org"\n}',
    inputHelp: 'Krever både «skill_id» og «to_scope».',
  },
  {
    value: 'FeedbackPromotionWorkflow',
    label: 'FeedbackPromotionWorkflow — forfremme lært mønster',
    needsInput: true,
    deniesZdr: true,
    inputExample: '{\n  "min_samples": 20,\n  "promote_threshold": 0.8\n}',
    inputHelp: 'Trenger ingen felter — «{}» er nok. «min_samples» og «promote_threshold» styrer terskelen.',
  },
]

const DEFAULT_WORKFLOW = INTERACTIVE_RUN_CHOICE.value

function workflowChoice(value: string): WorkflowChoice {
  return WORKFLOW_CHOICES.find((choice) => choice.value === value) ?? INTERACTIVE_RUN_CHOICE
}

/** The workflow an existing schedule fires, for display in the list. */
function scheduleWorkflowType(schedule: CronSchedule): string {
  const template = schedule.task_template
  if (template && typeof template === 'object' && !Array.isArray(template)) {
    const value = (template as { workflow_type?: unknown }).workflow_type
    if (typeof value === 'string' && value.trim()) return value.trim()
  }
  return DEFAULT_WORKFLOW
}

export function CronSchedulesSection() {
  const i18n = useI18n()
  const session = getSession()
  const orgId = createMemo(() => session.activeOrg?.id ?? '')
  const isAdmin = createMemo(() => hasWorkspaceAdminAccess(session))

  const [schedules, { refetch }] = createResource(
    () => orgId() || undefined,
    (id) => listCronSchedules(id).then((result) => result.schedules),
  )

  const [name, setName] = createSignal('')
  const [expr, setExpr] = createSignal('')
  const [timezone, setTimezone] = createSignal('UTC')
  const [title, setTitle] = createSignal('')
  const [workflowType, setWorkflowType] = createSignal(DEFAULT_WORKFLOW)
  const [workflowInput, setWorkflowInput] = createSignal('')

  const selectedWorkflow = createMemo(() => workflowChoice(workflowType()))

  const [submitting, setSubmitting] = createSignal(false)
  const [busyId, setBusyId] = createSignal<string | null>(null)
  const [formError, setFormError] = createSignal<string | null>(null)
  const [actionError, setActionError] = createSignal<string | null>(null)

  const list = createMemo(() => schedules() ?? [])

  const resetForm = () => {
    setName('')
    setExpr('')
    setTimezone('UTC')
    setTitle('')
    setWorkflowType(DEFAULT_WORKFLOW)
    setWorkflowInput('')
  }

  const handleCreate = async (event: Event) => {
    event.preventDefault()
    const id = orgId()
    if (!id || submitting()) return

    const trimmedName = name().trim()
    const trimmedExpr = expr().trim()
    if (!trimmedName || !trimmedExpr) {
      setFormError('Navn og cron-uttrykk er påkrevd.')
      return
    }

    // Validate the workflow input here rather than letting it fail at fire
    // time: a schedule with an undecodable input is accepted by the API and
    // then fails on every single fire, which is only visible in the task row.
    const choice = selectedWorkflow()
    const rawInput = workflowInput().trim()
    let parsedInput: Record<string, unknown> | undefined
    if (rawInput) {
      let candidate: unknown
      try {
        candidate = JSON.parse(rawInput)
      } catch {
        setFormError('Workflow-input må være gyldig JSON.')
        return
      }
      if (candidate === null || typeof candidate !== 'object' || Array.isArray(candidate)) {
        setFormError('Workflow-input må være et JSON-objekt, ikke en liste eller en enkel verdi.')
        return
      }
      parsedInput = candidate as Record<string, unknown>
    } else if (choice.needsInput) {
      setFormError(
        `${choice.value} kan ikke utlede målet fra tittelen — den krever en egen workflow-input. ${choice.inputHelp}`,
      )
      return
    }

    const template: CronTaskTemplate = {
      kind: 'cron',
      title: title().trim() || trimmedName,
      workflow_type: choice.value,
      ...(parsedInput ? { workflow_input: parsedInput } : {}),
    }

    setSubmitting(true)
    setFormError(null)
    try {
      await createCronSchedule(id, {
        name: trimmedName,
        schedule_expr: trimmedExpr,
        timezone: timezone().trim() || 'UTC',
        task_template: template,
        enabled: true,
      })
      resetForm()
      await refetch()
    } catch (err) {
      setFormError(
        translateApiError(err, i18n.tr, { no: 'Kunne ikke opprette planen.', en: 'Could not create the schedule.' }),
      )
    } finally {
      setSubmitting(false)
    }
  }

  const handleToggle = async (schedule: CronSchedule) => {
    const id = orgId()
    if (!id || busyId()) return
    setBusyId(schedule.id)
    setActionError(null)
    try {
      await updateCronSchedule(id, schedule.id, { enabled: !schedule.enabled })
      await refetch()
    } catch (err) {
      setActionError(
        translateApiError(err, i18n.tr, { no: 'Kunne ikke oppdatere planen.', en: 'Could not update the schedule.' }),
      )
    } finally {
      setBusyId(null)
    }
  }

  const handleDelete = async (schedule: CronSchedule) => {
    const id = orgId()
    if (!id || busyId()) return
    if (
      typeof window !== 'undefined' &&
      !window.confirm(`Slette planen «${schedule.name}»?`)
    ) {
      return
    }
    setBusyId(schedule.id)
    setActionError(null)
    try {
      await deleteCronSchedule(id, schedule.id)
      await refetch()
    } catch (err) {
      setActionError(
        translateApiError(err, i18n.tr, { no: 'Kunne ikke slette planen.', en: 'Could not delete the schedule.' }),
      )
    } finally {
      setBusyId(null)
    }
  }

  return (
    <>
      <SectionHeader
        title="Planlagte kjøringer"
        description="Tidsstyrte oppgaver agenten kjører automatisk. Hver plan har et cron-uttrykk; sweeperen lager en oppgave, logger kjøringen og beregner neste kjøring."
      />

      <Show when={actionError()}>
        {(message) => (
          <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
            {message()}
          </p>
        )}
      </Show>

      <Show when={schedules.error}>
        <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
          Kunne ikke laste planlagte kjøringer.
        </p>
      </Show>

      <Show
        when={!schedules.loading}
        fallback={
          <p class="verevon-settings-subnote" role="status" aria-busy="true">
            Laster planer…
          </p>
        }
      >
        <Show
          when={list().length > 0}
          fallback={
            <div class="verevon-settings-list-card">
              <p class="verevon-settings-empty-row">
                Ingen planlagte kjøringer ennå. Legg til en nedenfor.
              </p>
            </div>
          }
        >
          <div class="verevon-settings-list-card">
            <For each={list()}>
              {(schedule) => (
                <div class="verevon-settings-integration-row">
                  <div>
                    <p>
                      {schedule.name}{' '}
                      <span class="verevon-trust-chip" aria-label={`Status: ${schedule.enabled ? 'aktiv' : 'pauset'}`}>
                        {schedule.enabled ? 'aktiv' : 'pauset'}
                      </span>
                    </p>
                    <span>
                      <code>{schedule.schedule_expr}</code> · {schedule.timezone}
                      {' '}· <code>{scheduleWorkflowType(schedule)}</code>
                      {' '}· <Clock size={12} aria-hidden="true" /> neste: {formatWhen(schedule.next_fire_at)}
                      <Show when={schedule.last_fire_at}> · sist: {formatWhen(schedule.last_fire_at)}</Show>
                    </span>
                  </div>
                  <Show when={isAdmin()}>
                    <div>
                      <SettingsButton
                        settingsSize="sm"
                        disabled={busyId() === schedule.id}
                        onClick={() => void handleToggle(schedule)}
                        aria-label={`${schedule.enabled ? 'Pause' : 'Aktiver'} ${schedule.name}`}
                      >
                        <Show
                          when={busyId() === schedule.id}
                          fallback={<>{schedule.enabled ? 'Pause' : 'Aktiver'}</>}
                        >
                          <Loader2 size={14} aria-hidden="true" /> Lagrer…
                        </Show>
                      </SettingsButton>{' '}
                      <SettingsButton
                        settingsSize="sm"
                        danger
                        disabled={busyId() === schedule.id}
                        onClick={() => void handleDelete(schedule)}
                        aria-label={`Slett ${schedule.name}`}
                      >
                        <Trash2 size={14} aria-hidden="true" /> Slett
                      </SettingsButton>
                    </div>
                  </Show>
                </div>
              )}
            </For>
          </div>
        </Show>
      </Show>

      <Show
        when={isAdmin()}
        fallback={
          <p class="verevon-settings-subnote">
            <CalendarClock size={14} aria-hidden="true" /> Bare administratorer kan
            opprette eller endre planlagte kjøringer.
          </p>
        }
      >
        <form class="verevon-settings-field-grid verevon-settings-field-grid--spaced" onSubmit={handleCreate}>
          <label for="cron-name" class="verevon-settings-field">
            <span class="verevon-settings-label">Navn</span>
            <span class="verevon-settings-input-wrap">
              <VerevonInput
                id="cron-name"
                value={name()}
                required
                placeholder="f.eks. Daglig rapport"
                onInput={(event) => setName(event.currentTarget.value)}
                class="verevon-settings-input"
              />
            </span>
          </label>

          <label for="cron-expr" class="verevon-settings-field">
            <span class="verevon-settings-label">Cron-uttrykk</span>
            <span class="verevon-settings-input-wrap">
              <VerevonInput
                id="cron-expr"
                value={expr()}
                required
                placeholder="min time dag måned ukedag — f.eks. 0 9 * * 1"
                onInput={(event) => setExpr(event.currentTarget.value)}
                class="verevon-settings-input"
              />
            </span>
            <span class="verevon-settings-help">
              5 felt. Eksempel: «0 9 * * 1» = hver mandag kl. 09:00.
            </span>
          </label>

          <label for="cron-tz" class="verevon-settings-field">
            <span class="verevon-settings-label">Tidssone</span>
            <span class="verevon-settings-input-wrap">
              <VerevonInput
                id="cron-tz"
                value={timezone()}
                placeholder="UTC"
                onInput={(event) => setTimezone(event.currentTarget.value)}
                class="verevon-settings-input"
              />
            </span>
          </label>

          <label for="cron-title" class="verevon-settings-field">
            <span class="verevon-settings-label">Oppgavetittel (valgfritt)</span>
            <span class="verevon-settings-input-wrap">
              <VerevonInput
                id="cron-title"
                value={title()}
                placeholder="Tittel på oppgaven som opprettes ved hver kjøring"
                onInput={(event) => setTitle(event.currentTarget.value)}
                class="verevon-settings-input"
              />
            </span>
          </label>

          <label for="cron-workflow" class="verevon-settings-field">
            <span class="verevon-settings-label">Workflow</span>
            <span class="verevon-settings-input-wrap">
              <select
                id="cron-workflow"
                value={workflowType()}
                onChange={(event) => setWorkflowType(event.currentTarget.value)}
                class="verevon-settings-input verevon-settings-select"
              >
                <For each={WORKFLOW_CHOICES}>
                  {(choice) => <option value={choice.value}>{choice.label}</option>}
                </For>
              </select>
            </span>
            <span class="verevon-settings-help">
              Hva kjøringen faktisk starter. Standard er en vanlig agentkjøring.
            </span>
          </label>

          <label for="cron-workflow-input" class="verevon-settings-field">
            <span class="verevon-settings-label">
              Workflow-input {selectedWorkflow().needsInput ? '(påkrevd)' : '(valgfritt)'}
            </span>
            <span class="verevon-settings-input-wrap">
              <textarea
                id="cron-workflow-input"
                value={workflowInput()}
                rows={5}
                spellcheck={false}
                placeholder={selectedWorkflow().inputExample}
                onInput={(event) => setWorkflowInput(event.currentTarget.value)}
                class="verevon-settings-input"
              />
            </span>
            <span class="verevon-settings-help">{selectedWorkflow().inputHelp}</span>
            <Show when={selectedWorkflow().deniesZdr}>
              <span class="verevon-settings-help">
                Denne workflowen avvises for organisasjoner med Zero Data Retention slått
                på — kjøringen vil da feile.
              </span>
            </Show>
          </label>

          <Show when={formError()}>
            {(message) => (
              <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
                {message()}
              </p>
            )}
          </Show>

          <div>
            <SettingsButton type="submit" variant="primary" settingsSize="sm" disabled={submitting()}>
              <Show
                when={submitting()}
                fallback={<><CalendarClock size={14} aria-hidden="true" /> Opprett plan</>}
              >
                <Loader2 size={14} aria-hidden="true" /> Oppretter…
              </Show>
            </SettingsButton>
          </div>
        </form>
      </Show>

      <p class="verevon-settings-subnote">
        <CalendarClock size={14} aria-hidden="true" /> Org-tilhørighet utledes fra den
        verifiserte økten. Planer gjelder hele organisasjonen.
      </p>
    </>
  )
}
