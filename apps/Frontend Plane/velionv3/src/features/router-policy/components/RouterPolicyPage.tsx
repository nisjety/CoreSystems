import { createEffect, createMemo, createResource, createSignal, For, Show } from 'solid-js'
import { getRouterPolicy, updateRouterPolicy } from '@/shared/api/router-policy-client'
import { listModels, type ModelInfo } from '@/shared/api/chat-client'
import { getSession } from '@/shared/session/session-store'
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
  emptyRoutingPolicy,
  parseKeywords,
  routerPolicyModeRows,
  withCheapFallback,
  withComplexityNumber,
  withEnabled,
  withKeywords,
  withTableCell,
  withTopLevelNumber,
  type RoutingPolicy,
} from '@/features/router-policy/lib/router-policy-model'

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

export default function RouterPolicyPage() {
  const session = getSession()
  const orgId = createMemo(() => session.activeOrg?.id ?? '')

  const [policyResource] = createResource(
    () => orgId() || undefined,
    (id) => getRouterPolicy(id),
  )
  const [models] = createResource(() => listModels())

  const [draft, setDraft] = createSignal<RoutingPolicy>(emptyRoutingPolicy())
  const [keywordsText, setKeywordsText] = createSignal('')
  const [busy, setBusy] = createSignal(false)
  const [error, setError] = createSignal<string | null>(null)
  const [notice, setNotice] = createSignal<string | null>(null)

  // Seed the immutable draft from the loaded policy.
  createEffect(() => {
    const loaded = policyResource()
    if (loaded) {
      setDraft(loaded)
      setKeywordsText(loaded.complexity.keywords.join(', '))
    }
  })

  const modeRows = routerPolicyModeRows()
  const baseOptions = createMemo(() => modelOptions(models() ?? []))

  const save = async () => {
    const id = orgId()
    if (!id) {
      setError('No active organization.')
      return
    }
    setBusy(true)
    setError(null)
    setNotice(null)
    try {
      const next = withKeywords(draft(), parseKeywords(keywordsText()))
      const stored = await updateRouterPolicy(id, next)
      setDraft(stored)
      setKeywordsText(stored.complexity.keywords.join(', '))
      setNotice('Router policy saved.')
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : 'Could not save router policy.')
    } finally {
      setBusy(false)
    }
  }

  return (
    <SettingsSurface contentVariant="workspace">
      <SettingsHero
        eyebrow="Admin"
        title="Router policy"
        description="Tune the Velion intent layer at runtime: complexity scoring, budgets, and the model routing table."
      />

      <Show
        when={!policyResource.loading}
        fallback={<p class="velion-router-policy__loading">Loading router policy…</p>}
      >
        <Show when={policyResource.error}>
          <p class="velion-settings-status-message velion-settings-status-message--error" role="alert">
            Could not load the router policy from the gateway.
          </p>
        </Show>

        <section class="velion-router-policy">
          <div class="velion-router-policy__block">
            <ToggleRow
              title="Routing enabled"
              description="When off, requests bypass the intent layer and use the cheap fallback."
              enabled={draft().enabled}
              onChange={(checked) => setDraft((prev) => withEnabled(prev, checked))}
            />
          </div>

          <div class="velion-settings-field-grid">
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
          <div class="velion-router-policy__table" role="group" aria-label="Routing table">
            <For each={modeRows}>
              {(row) => (
                <div class="velion-router-policy__mode">
                  <div class="velion-router-policy__mode-head">
                    <p>{row.label}</p>
                    <span>{row.description}</span>
                  </div>
                  <div class="velion-router-policy__cells">
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
            title="Complexity scoring"
            description="Thresholds and weights that decide which complexity bucket a request lands in."
          />
          <div class="velion-settings-field-grid">
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
            <SettingsButton variant="primary" disabled={busy()} onClick={() => void save()}>
              {busy() ? 'Saving…' : 'Save router policy'}
            </SettingsButton>
          </div>
        </section>
      </Show>
    </SettingsSurface>
  )
}
