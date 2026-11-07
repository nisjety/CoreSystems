import { createEffect, createMemo, createSignal, createUniqueId, For, Show, untrack } from 'solid-js'
import { createResource } from '@/shared/lib/create-resource-compat'
import {
  filterModels,
  formatModelListPrice,
  getModelEntitlementMeta,
  groupChatModels,
  isExpensiveModel,
  listModels,
  selectionIsStale,
  verevonModeById,
  VEREVON_MODES,
  type ModelDataUse,
  type ModelInfo,
} from '@/shared/api/chat-client'
import {
  listChatGptSubscriptions,
  OPENAI_CODEX_SUBSCRIPTION_PROVIDER,
} from '@/shared/api/chatgpt-subscription-client'
import { getOrganizationAISettings } from '@/shared/api/organization-client'
import { Check, ChevronDown, Zap } from '@/shared/icons'
import { useI18n } from '@/shared/i18n'
import {
  DEFAULT_AI_MODEL_SELECTION,
  readAiModelSelection,
  rememberAiModelPrivacyTier,
  rememberAiModelSelection,
  type AiModelSelection,
} from '@/shared/ai/model-selection'
import { rememberedPrivacyTier } from '@/features/dashboard/home/submit-privacy-tier'
import { cn } from '@/shared/lib/cn'
import { ModelSearchField } from './ModelSearchField'

/**
 * The data-use statements a catalogue record makes about a model, joined as one short line. A
 * statement the record does not make is left out, so a model with none shows no line.
 */
function modelDataUseLine(
  dataUse: ModelDataUse | undefined,
  tr: (noText: string, enText: string) => string,
): string | undefined {
  if (!dataUse) return undefined
  const statements: string[] = []
  if (dataUse.zdrAttested === true) statements.push(tr('Null datalagring, bekreftet', 'Zero data retention, attested'))
  if (dataUse.trainsOnInputs === true) statements.push(tr('Brukes til trening', 'Used for training'))
  if (dataUse.trainsOnInputs === false) statements.push(tr('Brukes ikke til trening', 'Not used for training'))
  const days = dataUse.retentionDays
  if (days === 0) statements.push(tr('Ingen lagring', 'No retention'))
  if (days !== undefined && days > 0) {
    statements.push(days === 1
      ? tr('Lagres i 1 dag', 'Kept for 1 day')
      : tr(`Lagres i ${days} dager`, `Kept for ${days} days`))
  }
  return statements.length > 0 ? statements.join(' · ') : undefined
}

export function AiModelPicker(props: {
  orgId: string
  class?: string
  /** Support transcripts must honor the organization's no-retention posture. */
  respectOrgZdr?: boolean
  /**
   * The catalogue request to render from, when the parent's sends read the
   * same catalogue (`createTurnPrivacyTierResolver`). Defaults to its own
   * `listModels` request.
   */
  catalogue?: () => Promise<ModelInfo[]>
  onChange?: (selection: AiModelSelection) => void
}) {
  const i18n = useI18n()
  const [open, setOpen] = createSignal(false)
  const [query, setQuery] = createSignal('')
  const catalogId = `ai-model-catalog-${createUniqueId()}`
  const [selected, setSelected] = createSignal<AiModelSelection>(DEFAULT_AI_MODEL_SELECTION)
  const [models] = createResource(async () => {
    try {
      return await (untrack(() => props.catalogue) ?? listModels)()
    } catch {
      return [] as ModelInfo[]
    }
  })
  const orgId = () => props.orgId.trim() || false
  const [subscriptions] = createResource(orgId, async (id) => {
    try {
      return await listChatGptSubscriptions(id)
    } catch {
      return []
    }
  })
  const [organizationSettings] = createResource(
    () => props.respectOrgZdr ? orgId() : false,
    async (id) => {
      try {
        return await getOrganizationAISettings(id)
      } catch {
        return undefined
      }
    },
  )
  const activeSubscription = createMemo(() =>
    (subscriptions() ?? []).find((connection) => connection.status.trim().toLocaleLowerCase() === 'active'),
  )
  const subscriptionBlocked = createMemo(() => props.respectOrgZdr && organizationSettings()?.zdr === true)
  const catalogGroups = createMemo(() => groupChatModels(models() ?? []).filter(
    (group) => group.label !== 'Subscription' || Boolean(activeSubscription()),
  ))
  const flatModels = createMemo(() => catalogGroups().flatMap((group) => group.models))
  // The search applies only while its field is shown (more than six chat models). Selection and
  // the policy check keep reading the whole catalogue.
  const activeQuery = () => (flatModels().length > 6 ? query().trim() : '')
  const modelGroups = createMemo(() => activeQuery() === ''
    ? catalogGroups()
    : groupChatModels(filterModels(flatModels(), activeQuery())),
  )

  // A search belongs to one opening of the menu: closing the menu clears it.
  createEffect(
    () => open(),
    (isOpen) => { if (!isOpen) setQuery('') },
  )

  createEffect(
    () => props.orgId,
    (nextOrgId) => { setSelected(readAiModelSelection(nextOrgId)) },
  )

  // Keep the user's explicit selection through an outage or disconnection.
  // Sending through another provider requires a new user choice; the request
  // resolver/backend reports an unavailable subscription instead.

  const selectedCatalogModel = createMemo(() =>
    flatModels().find((model) => model.id === selected().model && model.provider === selected().provider),
  )

  // Keep the stored last-known tier current while the catalogue lists the
  // selection, as the main composer does: a send falls back on it when a later
  // catalogue is slow or down (`submit-privacy-tier.ts`).
  createEffect(
    () => selectedCatalogModel(),
    (listed) => {
      if (!listed) return
      rememberAiModelPrivacyTier(
        props.orgId,
        { model: listed.id, provider: listed.provider },
        rememberedPrivacyTier(listed),
      )
    },
  )

  const selectedLabel = createMemo(() => {
    const current = selected()
    return verevonModeById(current.model)?.label
      ?? selectedCatalogModel()?.name
      ?? current.label
  })

  // A selection the organisation's policy no longer offers would be refused on
  // send, so it is replaced when the catalogue says so, and the person is told.
  // The meta is read after the catalogue answers, which is what sets it.
  const entitlementMeta = createMemo(() => {
    models()
    return getModelEntitlementMeta()
  })
  const [policyResetModel, setPolicyResetModel] = createSignal<string | null>(null)
  createEffect(
    () => ({ catalogue: models() ?? [], meta: entitlementMeta(), current: selected() }),
    (state) => {
      if (!selectionIsStale(state.current, state.catalogue, state.meta)) return
      const label = selectedLabel()
      const preferred = state.meta?.defaultModel
        ? flatModels().find((model) => model.id === state.meta?.defaultModel)
        : undefined
      if (preferred) chooseCatalogModel(preferred)
      else choose(DEFAULT_AI_MODEL_SELECTION)
      setPolicyResetModel(label)
    },
  )

  function choose(selection: AiModelSelection): void {
    setPolicyResetModel(null)
    const next = selection.provider === OPENAI_CODEX_SUBSCRIPTION_PROVIDER
      ? { ...selection, subscriptionConnectionId: untrack(activeSubscription)?.id }
      : selection
    setSelected(next)
    rememberAiModelSelection(untrack(() => props.orgId), next)
    untrack(() => props.onChange)?.(next)
    setOpen(false)
  }

  function chooseCatalogModel(model: ModelInfo): void {
    if (model.provider === OPENAI_CODEX_SUBSCRIPTION_PROVIDER && untrack(subscriptionBlocked)) return
    choose({
      model: model.id,
      label: model.name,
      ...(model.provider ? { provider: model.provider } : {}),
      privacyTier: rememberedPrivacyTier(model),
    })
  }

  return (
    <div class={cn('verevon-ai-model-picker', props.class)}>
      <button
        type="button"
        class="dashboard-composer-model-button verevon-composer-control verevon-ai-model-picker__trigger"
        aria-expanded={open() ? 'true' : 'false'}
        aria-label={i18n.tr('Velg AI-modell', 'Choose AI model')}
        title={i18n.tr('Velg AI-modell', 'Choose AI model')}
        onClick={() => setOpen((current) => !current)}
      >
        <Zap class="size-4 dashboard-composer-model-button__zap" aria-hidden="true" />
        <span>{selectedLabel()}</span>
        <ChevronDown class={cn('dashboard-composer-model-button__chevron', open() && 'dashboard-composer-model-button__chevron--open')} aria-hidden="true" />
      </button>
      <Show when={policyResetModel()}>
        {(modelName) => (
          <p class="verevon-ai-model-picker__notice" role="status">
            {i18n.tr(
              `Organisasjonen din tillater ikke lenger ${modelName()}. Modellen er byttet.`,
              `Your organisation no longer allows ${modelName()}. The model was changed.`,
            )}
          </p>
        )}
      </Show>
      <Show when={open()}>
        <div class="dashboard-composer-model-menu verevon-popover verevon-ai-model-picker__menu">
          <Show when={flatModels().length > 6}>
            <ModelSearchField value={query()} controlsId={catalogId} onChange={setQuery} />
          </Show>
          <div id={catalogId} class="dashboard-composer-model-catalog" role="region" aria-label={i18n.tr('Tilgjengelige AI-modeller', 'Available AI models')} tabindex={0}>
            <div class="dashboard-composer-model-group">
              <p class="dashboard-composer-model-group__label">Verevon</p>
              <For each={VEREVON_MODES}>{(mode) => (
                <button
                  type="button"
                  onClick={() => choose({ model: mode.id, label: mode.label })}
                  class={{ 'dashboard-composer-model-menu__item--active': !selected().provider && selected().model === mode.id }}
                >
                  <span><span><Zap class="size-3" /></span><span>{mode.label}</span></span>
                  <span class="dashboard-composer-model-menu__right">
                    <span class={cn('dashboard-composer-model-badge', mode.badge === 'premium' ? 'dashboard-composer-model-badge--premium' : 'dashboard-composer-model-badge--cheap')}>
                      {mode.badge === 'premium' ? '$$' : i18n.tr('Rimelig', 'Low cost')}
                    </span>
                    <Show when={!selected().provider && selected().model === mode.id}><Check class="size-4" /></Show>
                  </span>
                </button>
              )}</For>
            </div>
            <For each={modelGroups()}>{(group) => (
              <div class="dashboard-composer-model-group">
                <p class="dashboard-composer-model-group__label">{group.label}</p>
                <For each={group.models}>{(model) => {
                  const blocked = () => model.provider === OPENAI_CODEX_SUBSCRIPTION_PROVIDER && subscriptionBlocked()
                  return (
                    <button
                      type="button"
                      disabled={blocked()}
                      title={blocked()
                        ? i18n.tr('Abonnementsmodeller støtter ikke null-lagring for supportdata.', 'Subscription models do not support zero-retention support data.')
                        : i18n.tr(`Bruk ${model.name}`, `Use ${model.name}`)}
                      onClick={() => chooseCatalogModel(model)}
                      class={{ 'dashboard-composer-model-menu__item--active': selected().model === model.id && selected().provider === model.provider }}
                    >
                      <span><span><Zap class="size-3" /></span><span>{model.name}<Show when={model.price}>{(price) => <span class="dashboard-composer-model-price">{formatModelListPrice(price(), i18n.locale())}</span>}</Show><Show when={modelDataUseLine(model.dataUse, i18n.tr)}>{(line) => <span class="dashboard-composer-model-data-use">{line()}</span>}</Show></span></span>
                      <span class="dashboard-composer-model-menu__right">
                        <Show when={model.provider === OPENAI_CODEX_SUBSCRIPTION_PROVIDER}>
                          <span class="dashboard-composer-model-badge dashboard-composer-model-badge--cheap">{i18n.tr('Tilkoblet', 'Connected')}</span>
                        </Show>
                        <Show when={model.provider !== OPENAI_CODEX_SUBSCRIPTION_PROVIDER && isExpensiveModel(model)}>
                          <span class="dashboard-composer-model-badge dashboard-composer-model-badge--premium">$$</span>
                        </Show>
                        <Show when={selected().model === model.id && selected().provider === model.provider}><Check class="size-4" /></Show>
                      </span>
                    </button>
                  )
                }}</For>
              </div>
            )}</For>
            <Show when={activeQuery() !== '' && modelGroups().length === 0}>
              <p class="dashboard-composer-model-empty" role="status">{i18n.tr('Ingen modeller passer', 'No models match')}</p>
            </Show>
          </div>
        </div>
      </Show>
    </div>
  )
}
