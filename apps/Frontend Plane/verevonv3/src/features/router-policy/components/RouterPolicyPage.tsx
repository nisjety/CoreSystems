import { createEffect, createMemo, createSignal, For, Show } from 'solid-js'
import { createResource } from '@/shared/lib/create-resource-compat'
import { getRouterPolicy, updateRouterPolicy } from '@/shared/api/router-policy-client'
import {
  formatContextWindow,
  formatModelListPrice,
  isChatModel,
  listModelCatalogue,
  type ModelInfo,
} from '@/shared/api/chat-client'
import { ApiError } from '@/shared/api/api-error'
import { getSession } from '@/shared/session/session-store'
import { useI18n } from '@/shared/i18n'
import { translateApiError } from '@/shared/i18n/errors'
import {
  SectionHeader,
  SettingsButton,
  SettingsField,
  SettingsHero,
  SettingsSelect,
  SettingsSurface,
  SettingsTextarea,
  ToggleRow,
} from '@/features/settings/components/settings-ui'
import {
  COMPLEXITY_FIELD_SPECS,
  ROUTER_POLICY_COMPLEXITIES,
  ROUTER_POLICY_COMPLEXITY_LABELS,
  ROUTER_POLICY_ROLE_MAX_CANDIDATES,
  ROUTER_POLICY_ROLE_PURPOSES,
  catalogueOverlayIssues,
  catalogueOverlayIssuesFrom,
  catalogueOverlayOf,
  emptyRoutingPolicy,
  moveCandidate,
  parseKeywords,
  roleEntryOf,
  roleIssuesFrom,
  routerPolicyModeRows,
  withCatalogueOverlay,
  withCheapFallback,
  withComplexityNumber,
  withEnabled,
  withKeywords,
  withoutAliasCopies,
  withRoleCandidates,
  withTableCell,
  withTopLevelNumber,
  type CatalogueOverlayIssue,
  type RouterPolicyCatalogueOverlay,
  type RouterPolicyDocument,
  type RouterPolicyRoleIssue,
  type RouterPolicyRolePurpose,
} from '@/features/router-policy/lib/router-policy-model'
import {
  CatalogueOverlayEditor,
  catalogueOverlayEnabled,
} from '@/features/router-policy/components/CatalogueOverlayEditor'

function modelOptions(models: ModelInfo[]): Array<{ value: string; label: string }> {
  return models.map((model) => ({ value: model.id, label: model.name || model.id }))
}

/**
 * Ensure the currently-selected model id is always present as an option, even if
 * it is not in the live catalog (e.g. a model the backend retired). This keeps the
 * select faithful to the stored policy rather than silently dropping the value.
 */
function optionsWithSelected(
  base: Array<{ value: string; label: string }>,
  selected: string,
): Array<{ value: string; label: string }> {
  if (!selected || base.some((option) => option.value === selected)) return base
  return [{ value: selected, label: `${selected} (unavailable)` }, ...base]
}

function toNumber(value: string): number {
  const parsed = Number(value)
  return Number.isFinite(parsed) ? parsed : 0
}

type Copy = { readonly no: string; readonly en: string }
type Tr = (noText: string, enText: string) => string

/** The label and the one-line hint of each side-task row, in both languages. */
const ROLE_ROW_COPY: Record<RouterPolicyRolePurpose, { label: Copy; hint: Copy }> = {
  title: {
    label: { no: 'Samtaletittel', en: 'Chat title' },
    hint: { no: 'Gir en ny samtale en tittel.', en: 'Gives a new conversation its title.' },
  },
  follow_ups: {
    label: { no: 'Oppfølgingsforslag', en: 'Follow-up suggestions' },
    hint: { no: 'Foreslår neste spørsmål etter et svar.', en: 'Suggests the next question after an answer.' },
  },
  verification_judge: {
    label: { no: 'Verifisering', en: 'Verification' },
    hint: { no: 'Dommeren som vurderer et svar.', en: 'The judge that reviews an answer.' },
  },
  research_plan: {
    label: { no: 'Forskningsplan', en: 'Research plan' },
    hint: { no: 'Planlegger en dyp research-oppgave.', en: 'Plans a deep research task.' },
  },
}

/** What one refusal of a role table says, on the row it names. */
function roleIssueText(issue: RouterPolicyRoleIssue, tr: Tr): string {
  if (issue.reason === 'candidate_count') {
    return tr('Velg mellom én og fire modeller.', 'Choose between one and four models.')
  }
  if (issue.reason === 'unknown_model') {
    return tr(`«${issue.model ?? ''}» finnes ikke i chat-katalogen.`, `«${issue.model ?? ''}» is not in the chat catalogue.`)
  }
  if (issue.reason === 'reasoning_only') {
    return tr(
      `«${issue.model ?? ''}» kan ikke svare uten resonnering, og denne oppgaven trenger det.`,
      `«${issue.model ?? ''}» cannot answer without reasoning, and this task needs it.`,
    )
  }
  if (issue.reason === 'unknown_effort') {
    return tr(`«${issue.effort ?? ''}» er ikke et gyldig resonneringsnivå.`, `«${issue.effort ?? ''}» is not a valid reasoning level.`)
  }
  return tr('Rollen er ikke gyldig.', 'The role is not valid.')
}

/**
 * One side task's row: its candidates as ordered chips. While role routing is on, the row also
 * carries the controls that change them; while it is off, the row only shows what is stored.
 */
function RoleRow(props: {
  purpose: RouterPolicyRolePurpose
  candidates: readonly string[]
  catalogue: readonly ModelInfo[]
  editable: boolean
  issues: readonly RouterPolicyRoleIssue[]
  onCandidates: (candidates: string[]) => void
}) {
  const i18n = useI18n()
  const copy = () => ROLE_ROW_COPY[props.purpose]
  const modelOf = (id: string): ModelInfo | undefined => props.catalogue.find((model) => model.id === id)
  const addable = () => props.catalogue.filter((model) => isChatModel(model) && !props.candidates.includes(model.id))

  const chipName = (id: string): string => {
    const model = modelOf(id)
    if (!model) return `${id} (${i18n.tr('ikke i katalogen', 'not in the catalogue')})`
    return model.name || model.id
  }

  const chipDetail = (id: string): string => {
    const model = modelOf(id)
    if (!model) return ''
    const parts = [
      formatContextWindow(model.contextWindowTokens),
      model.price ? formatModelListPrice(model.price, i18n.locale()) : undefined,
    ]
    return parts.filter((part): part is string => part !== undefined).join(' · ')
  }

  return (
    <div class="verevon-router-policy__mode">
      <div class="verevon-router-policy__mode-head">
        <p>{i18n.tr(copy().label.no, copy().label.en)}</p>
        <span>{i18n.tr(copy().hint.no, copy().hint.en)}</span>
      </div>

      <Show when={props.candidates.length > 0} fallback={
        <p class="verevon-settings-subnote">
          {i18n.tr('Ingen modell valgt. Turens egen modell brukes.', "No model chosen. The turn's own model is used.")}
        </p>
      }>
        <ol class="verevon-trust-chips" aria-label={i18n.tr(`Modeller for ${copy().label.no}`, `Models for ${copy().label.en}`)}>
          <For each={props.candidates}>
            {(id, index) => (
              <li class="verevon-trust-chip">
                <span>
                  {index() + 1}. {chipName(id)}
                </span>
                <Show when={chipDetail(id)}>
                  {(detail) => <span class="verevon-settings-help">{detail()}</span>}
                </Show>
                <Show when={props.editable}>
                  <SettingsButton
                    type="button"
                    settingsSize="xs"
                    disabled={index() === 0}
                    aria-label={i18n.tr(`Flytt ${chipName(id)} opp`, `Move ${chipName(id)} up`)}
                    onClick={() => props.onCandidates(moveCandidate(props.candidates, index(), -1))}
                  >
                    ↑
                  </SettingsButton>
                  <SettingsButton
                    type="button"
                    settingsSize="xs"
                    disabled={index() === props.candidates.length - 1}
                    aria-label={i18n.tr(`Flytt ${chipName(id)} ned`, `Move ${chipName(id)} down`)}
                    onClick={() => props.onCandidates(moveCandidate(props.candidates, index(), 1))}
                  >
                    ↓
                  </SettingsButton>
                  <SettingsButton
                    type="button"
                    settingsSize="xs"
                    aria-label={i18n.tr(`Fjern ${chipName(id)}`, `Remove ${chipName(id)}`)}
                    onClick={() => props.onCandidates(props.candidates.filter((_, position) => position !== index()))}
                  >
                    ×
                  </SettingsButton>
                </Show>
              </li>
            )}
          </For>
        </ol>
      </Show>

      <Show when={props.editable && props.candidates.length < ROUTER_POLICY_ROLE_MAX_CANDIDATES}>
        <SettingsSelect
          id={`router-role-${props.purpose}-add`}
          label={i18n.tr('Legg til modell', 'Add model')}
          value=""
          options={[
            { value: '', label: i18n.tr('Velg en modell…', 'Choose a model…') },
            ...addable().map((model) => ({ value: model.id, label: model.name || model.id })),
          ]}
          onChange={(event) => {
            const id = event.currentTarget.value
            event.currentTarget.value = ''
            if (id) props.onCandidates([...props.candidates, id])
          }}
        />
      </Show>

      <For each={props.issues}>
        {(issue) => (
          <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
            {roleIssueText(issue, i18n.tr)}
          </p>
        )}
      </For>
    </div>
  )
}

export default function RouterPolicyPage() {
  const i18n = useI18n()
  const session = getSession()
  const orgId = createMemo(() => session.activeOrg?.id ?? '')

  // The document is kept with the organisation it was read for: a reload that fails leaves the last value
  // in place, and that value must never be saved into another organisation.
  const [policyResource, { refetch: refetchPolicy }] = createResource(
    () => orgId() || undefined,
    (id) => getRouterPolicy(id).then((document) => ({ orgId: id, document })),
  )
  const loaded = createMemo(() => {
    const current = policyResource()
    return current && current.orgId === orgId() ? current.document : undefined
  })
  const [catalogue, { refetch: refetchCatalogue }] = createResource(() => listModelCatalogue())
  const catalogueModels = createMemo(() => catalogue()?.models ?? [])
  const roleRouting = createMemo(() => catalogue()?.roleRouting === true)

  const [draft, setDraft] = createSignal<RouterPolicyDocument>(emptyRoutingPolicy())
  const [keywordsText, setKeywordsText] = createSignal('')
  const [busy, setBusy] = createSignal(false)
  const [error, setError] = createSignal<string | null>(null)
  const [notice, setNotice] = createSignal<string | null>(null)
  const [roleIssues, setRoleIssues] = createSignal<RouterPolicyRoleIssue[]>([])

  // The overlay the draft holds. The listing carries a copy of each alias the server applied, so the copies are taken out
  // of the models an alias may name. Both the loaded overlay and the draft are read for that, since the listing was read
  // with the loaded one.
  const overlay = createMemo(() => catalogueOverlayOf(draft()))
  const overlayModels = createMemo(() => {
    const aliases = [...Object.entries(catalogueOverlayOf(loaded() ?? {}).aliases), ...Object.entries(overlay().aliases)]
    return withoutAliasCopies(catalogueModels(), aliases).filter(isChatModel)
  })
  // With no chat listing (it failed to load), the checks that need one are skipped rather than refusing every alias.
  const overlayIssues = createMemo(() => {
    const models = overlayModels()
    return catalogueOverlayIssues(overlay(), models.length > 0 ? models.map((model) => model.id) : undefined)
  })
  const [overlayRefusals, setOverlayRefusals] = createSignal<CatalogueOverlayIssue[]>([])

  // Seed the draft from the loaded document. Nothing is saved before it arrives: the empty draft
  // above would overwrite the stored policy with zeros.
  createEffect(
    () => loaded(),
    (document) => {
      if (document) {
        setDraft(document)
        setKeywordsText(document.complexity.keywords.join(', '))
      }
    },
  )

  const modeRows = routerPolicyModeRows()
  const baseOptions = createMemo(() => modelOptions(catalogueModels()))

  const save = async () => {
    const id = orgId()
    if (!id) {
      setError('No active organization.')
      return
    }
    if (!loaded()) return
    // With the overlay editor shown, its own refusals stop the save here, so the person sees them on their rows first.
    if (catalogueOverlayEnabled() && overlayIssues().length > 0) {
      setError(i18n.tr('Rett feilene i katalogoversikten før du lagrer.', 'Fix the catalogue overlay errors before you save.'))
      return
    }
    setBusy(true)
    setError(null)
    setNotice(null)
    setRoleIssues([])
    setOverlayRefusals([])
    try {
      const next = withKeywords(draft(), parseKeywords(keywordsText()))
      const stored = await updateRouterPolicy(id, next)
      setDraft(stored)
      setKeywordsText(stored.complexity.keywords.join(', '))
      setNotice('Router policy saved.')
    } catch (reason) {
      if (reason instanceof ApiError && reason.status === 409) {
        // The policy moved on after it was read. Reload it, so the person makes the change again on the current copy.
        setError(
          i18n.tr(
            'Ruterpolicyen er endret siden den ble lastet. Den er lastet på nytt; gjør endringene på nytt.',
            'The router policy changed after it was loaded. It has been reloaded; make your changes again.',
          ),
        )
        void refetchPolicy()
        void refetchCatalogue()
      } else {
        setError(translateApiError(reason, i18n.tr, { no: 'Kunne ikke lagre ruterpolicyen.', en: 'Could not save router policy.' }))
      }
      // Only the role table refusal names rows: its issues sit under the envelope's details (or are the details).
      setRoleIssues(
        reason instanceof ApiError && reason.code === 'invalid_role_table' ? roleIssuesFrom(reason.details) : [],
      )
      // The overlay refusal likewise names the aliases and models it refuses, and sits under the envelope's details.
      setOverlayRefusals(
        reason instanceof ApiError && reason.code === 'invalid_catalogue_overlay'
          ? catalogueOverlayIssuesFrom(reason.details)
          : [],
      )
    } finally {
      setBusy(false)
    }
  }

  const setRoleCandidates = (purpose: RouterPolicyRolePurpose, candidates: readonly string[]) => {
    setRoleIssues([])
    setDraft((prev) => withRoleCandidates(prev, purpose, candidates))
  }

  const setOverlay = (next: RouterPolicyCatalogueOverlay) => {
    setOverlayRefusals([])
    setDraft((prev) => withCatalogueOverlay(prev, next))
  }

  return (
    <SettingsSurface contentVariant="workspace">
      <SettingsHero
        eyebrow="Admin"
        title="Router policy"
        description="Tune the Verevon intent layer at runtime: complexity scoring, budgets, and the model routing table."
      />

      <Show
        when={!policyResource.loading}
        fallback={<p class="verevon-router-policy__loading">Loading router policy…</p>}
      >
        <Show when={policyResource.error}>
          <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
            Could not load the router policy from the gateway.
          </p>
        </Show>

        <Show when={loaded()}>
          <section class="verevon-router-policy">
            <div class="verevon-router-policy__block">
              <ToggleRow
                title="Routing enabled"
                description="When off, requests bypass the intent layer and use the cheap fallback."
                enabled={draft().enabled}
                onChange={(checked) => setDraft((prev) => withEnabled(prev, checked))}
              />
            </div>

            <div class="verevon-settings-field-grid">
              <SettingsField
                id="router-budget-cap"
                label="Budget cap (USD)"
                type="number"
                value={String(draft().budget_cap_usd)}
                onInput={(event) =>
                  setDraft((prev) => withTopLevelNumber(prev, 'budget_cap_usd', toNumber(event.currentTarget.value)))
                }
              />
              <SettingsField
                id="router-constrained-fraction"
                label="Constrained fraction"
                type="number"
                helpText="Share of spend routed under budget constraints (0–1)."
                value={String(draft().constrained_fraction)}
                onInput={(event) =>
                  setDraft((prev) =>
                    withTopLevelNumber(prev, 'constrained_fraction', toNumber(event.currentTarget.value)),
                  )
                }
              />
              <SettingsSelect
                id="router-cheap-fallback"
                label="Cheap fallback model"
                value={draft().cheap_fallback}
                options={optionsWithSelected(baseOptions(), draft().cheap_fallback)}
                onChange={(event) => setDraft((prev) => withCheapFallback(prev, event.currentTarget.value))}
              />
            </div>

            <SectionHeader
              title="Routing table"
              description="Each cell is the model used for a given mode and complexity bucket."
            />
            <div class="verevon-router-policy__table" role="group" aria-label="Routing table">
              <For each={modeRows}>
                {(row) => (
                  <div class="verevon-router-policy__mode">
                    <div class="verevon-router-policy__mode-head">
                      <p>{row.label}</p>
                      <span>{row.description}</span>
                    </div>
                    <div class="verevon-router-policy__cells">
                      <For each={ROUTER_POLICY_COMPLEXITIES}>
                        {(complexity) => (
                          <SettingsSelect
                            id={`router-cell-${row.mode}-${complexity}`}
                            label={ROUTER_POLICY_COMPLEXITY_LABELS[complexity]}
                            value={draft().table[row.mode][complexity]}
                            options={optionsWithSelected(baseOptions(), draft().table[row.mode][complexity])}
                            onChange={(event) =>
                              setDraft((prev) => withTableCell(prev, row.mode, complexity, event.currentTarget.value))
                            }
                          />
                        )}
                      </For>
                    </div>
                  </div>
                )}
              </For>
            </div>

            <SectionHeader
              title={i18n.tr('Modeller for hjelpeoppgaver', 'Models for side tasks')}
              description={i18n.tr(
                'Modellen hver hjelpeoppgave bruker. Den første modellen i listen som kan brukes, velges. Finnes ingen, brukes turens egen modell.',
                "The model each side task uses. The first model in the list that can be used is chosen; when none can, the turn's own model is used.",
              )}
            />
            <Show when={catalogue() !== undefined && !roleRouting()}>
              <p class="verevon-settings-subnote" role="status">
                {i18n.tr(
                  'Rollerutingen er ikke aktivert i denne installasjonen. Tabellen vises bare, og brukes ikke før den er slått på.',
                  'Role routing is not turned on in this installation. The table is shown only, and is not used until it is switched on.',
                )}
              </p>
            </Show>
            <div
              class="verevon-router-policy__table"
              role="group"
              aria-label={i18n.tr('Modeller for hjelpeoppgaver', 'Models for side tasks')}
            >
              <For each={ROUTER_POLICY_ROLE_PURPOSES}>
                {(purpose) => (
                  <RoleRow
                    purpose={purpose}
                    candidates={roleEntryOf(draft(), purpose)?.candidates ?? []}
                    catalogue={catalogueModels()}
                    editable={roleRouting()}
                    issues={roleIssues().filter((issue) => issue.purpose === purpose)}
                    onCandidates={(candidates) => setRoleCandidates(purpose, candidates)}
                  />
                )}
              </For>
            </div>

            <Show when={catalogueOverlayEnabled()}>
              <CatalogueOverlayEditor
                overlay={overlay()}
                models={overlayModels()}
                issues={[...overlayIssues(), ...overlayRefusals()]}
                onOverlay={setOverlay}
              />
            </Show>

            <SectionHeader
              title="Complexity scoring"
              description="Thresholds and weights that decide which complexity bucket a request lands in."
            />
            <div class="verevon-settings-field-grid">
              <For each={COMPLEXITY_FIELD_SPECS}>
                {(spec) => (
                  <SettingsField
                    id={`router-complexity-${spec.key}`}
                    label={spec.label}
                    type="number"
                    helpText={spec.helpText}
                    value={String(draft().complexity[spec.key])}
                    onInput={(event) =>
                      setDraft((prev) => withComplexityNumber(prev, spec.key, toNumber(event.currentTarget.value)))
                    }
                  />
                )}
              </For>
            </div>

            <SettingsTextarea
              id="router-keywords"
              label="Complexity keywords (comma or newline separated)"
              value={keywordsText()}
              onInput={(event) => setKeywordsText(event.currentTarget.value)}
            />

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
              <SettingsButton variant="primary" disabled={busy()} onClick={() => void save()}>
                {busy() ? 'Saving…' : 'Save router policy'}
              </SettingsButton>
            </div>
          </section>
        </Show>
      </Show>
    </SettingsSurface>
  )
}
