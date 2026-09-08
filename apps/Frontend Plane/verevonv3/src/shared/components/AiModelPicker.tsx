import { createEffect, createMemo, createSignal, For, Show } from 'solid-js'
import { createResource } from '@/shared/lib/create-resource-compat'
import {
  groupChatModels,
  isExpensiveModel,
  listModels,
  verevonModeById,
  VEREVON_MODES,
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
  rememberAiModelSelection,
  type AiModelSelection,
} from '@/shared/ai/model-selection'
import { cn } from '@/shared/lib/cn'

export function AiModelPicker(props: {
  orgId: string
  class?: string
  /** Support transcripts must honor the organization's no-retention posture. */
  respectOrgZdr?: boolean
  onChange?: (selection: AiModelSelection) => void
}) {
  const i18n = useI18n()
  const [open, setOpen] = createSignal(false)
  const [selected, setSelected] = createSignal<AiModelSelection>(readAiModelSelection(props.orgId))
  const [models] = createResource(async () => {
    try {
      return await listModels()
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
  const modelGroups = createMemo(() => groupChatModels(models() ?? []).filter(
    (group) => group.label !== 'Subscription' || Boolean(activeSubscription()),
  ))
  const flatModels = createMemo(() => modelGroups().flatMap((group) => group.models))

  createEffect(
    () => props.orgId,
    (nextOrgId) => { setSelected(readAiModelSelection(nextOrgId)) },
  )

  // A removed/disconnected subscription must not leave a picker claiming a
  // route that can no longer run. Reset after both resources have settled.
  createEffect(
    () => ({
      modelsLoading: models.loading,
      subscriptionsLoading: subscriptions.loading,
      connectionId: activeSubscription()?.id,
    }),
    (state) => {
      if (state.modelsLoading || state.subscriptionsLoading) return
      const current = selected()
      if (current.provider !== OPENAI_CODEX_SUBSCRIPTION_PROVIDER || state.connectionId) return
      choose(DEFAULT_AI_MODEL_SELECTION)
    },
  )

  const selectedLabel = createMemo(() => {
    const current = selected()
    return verevonModeById(current.model)?.label
      ?? flatModels().find((model) => model.id === current.model && model.provider === current.provider)?.name
      ?? current.label
  })

  function choose(selection: AiModelSelection): void {
    const next = selection.provider === OPENAI_CODEX_SUBSCRIPTION_PROVIDER
      ? { ...selection, subscriptionConnectionId: activeSubscription()?.id }
      : selection
    setSelected(next)
    rememberAiModelSelection(props.orgId, next)
    props.onChange?.(next)
    setOpen(false)
  }

  function chooseCatalogModel(model: ModelInfo): void {
    if (model.provider === OPENAI_CODEX_SUBSCRIPTION_PROVIDER && subscriptionBlocked()) return
    choose({
      model: model.id,
      label: model.name,
      ...(model.provider ? { provider: model.provider } : {}),
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
      <Show when={open()}>
        <div class="dashboard-composer-model-menu verevon-popover verevon-ai-model-picker__menu">
          <div class="dashboard-composer-model-catalog" role="region" aria-label={i18n.tr('Tilgjengelige AI-modeller', 'Available AI models')} tabindex={0}>
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
                      <span><span><Zap class="size-3" /></span><span>{model.name}</span></span>
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
          </div>
        </div>
      </Show>
    </div>
  )
}
