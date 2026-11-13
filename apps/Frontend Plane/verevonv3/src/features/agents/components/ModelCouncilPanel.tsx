/**
 * The model council on the Agents playground (F-t3-092). Two to four models from the catalogue the model picker loads
 * answer one question with no tools, and one synthesis reads where they agree and disagree. The cost is estimated first,
 * and a run starts only on the explicit "Run (≈ $x)" button. A subscription model runs only when the person allows it,
 * since it is billed to their plan. Nothing is stored: the answers live as long as this panel is open. The playground
 * offers the panel only behind `VITE_MODEL_COUNCIL`, read with the strict literal `true`.
 */
import { For, Show, createMemo, createSignal, createUniqueId, onCleanup } from 'solid-js'
import { Portal } from '@solidjs/web'
import { createResource } from '@/shared/lib/create-resource-compat'
import { groupChatModels, listModels, type ModelInfo } from '@/shared/api/chat-client'
import { ApiError } from '@/shared/api/api-error'
import {
  estimateCouncil,
  runCouncil,
  type CouncilEstimate,
  type CouncilEstimateCall,
  type CouncilMember,
  type CouncilRun,
  type CouncilSynthesis,
  type CouncilUsage,
} from '@/shared/api/council-client'
import {
  COUNCIL_MAX_MODELS,
  COUNCIL_QUESTION_MAX_CHARS,
  councilInputAccepted,
  isCouncilSubscriptionProvider,
  type CouncilRunInput,
} from '@/shared/actions/ai-council'
import { translateApiError, useI18n } from '@/shared/i18n'
import { CircleAlert, GitCompare, Loader2, X } from '@/shared/icons'
import './ModelCouncilPanel.css'

/** The translator the panel's helpers read copy through. The copy stays in `i18n.tr(no, en)`, so both locales switch. */
type Translator = { tr: (noText: string, enText: string) => string }

/** `VITE_MODEL_COUNCIL === 'true'` turns the council on. Any other value, or none, keeps it off. */
export function modelCouncilEnabled(): boolean {
  return import.meta.env.VITE_MODEL_COUNCIL === 'true'
}

/**
 * A cost in US dollars, as the panel shows it. A sub-cent cost keeps its digits, so a real cost never reads as $0, and a
 * cost under a millionth reads as below that.
 */
export function councilUsd(value: number): string {
  if (!Number.isFinite(value)) return '—'
  if (value >= 1) return `$${value.toFixed(2)}`
  if (value === 0) return '$0.00'
  if (value < 0.000001) return '< $0.000001'
  const [whole = '0', fraction = ''] = value.toFixed(value < 0.01 ? 6 : 4).split('.')
  return `$${whole}.${fraction.replace(/0+$/, '').padEnd(2, '0')}`
}

function usageLine(usage: CouncilUsage, t: Translator): string {
  const tokens = t.tr(
    `${usage.inputTokens} tokens inn · ${usage.outputTokens} tokens ut`,
    `${usage.inputTokens} tokens in · ${usage.outputTokens} tokens out`,
  )
  const cost = usage.costUsd === null ? t.tr('kostnad ikke vist', 'cost not shown') : `≈ ${councilUsd(usage.costUsd)}`
  return `${tokens} · ${cost}`
}

function estimateCallLine(call: CouncilEstimateCall, label: string, t: Translator): string {
  const cost = call.estimatedCostUsd === null
    ? t.tr('kostnad ikke oppgitt', 'cost not stated')
    : `≈ ${councilUsd(call.estimatedCostUsd)}`
  const tokens = t.tr(
    `ca. ${call.estimatedInputTokens} tokens inn, opptil ${call.maxOutputTokens} ut`,
    `about ${call.estimatedInputTokens} tokens in, up to ${call.maxOutputTokens} out`,
  )
  return `${label}: ${cost} (${tokens})`
}

function totalEstimateLine(cost: CouncilEstimate, t: Translator): string {
  const total = cost.totalEstimatedCostUsd
  if (total !== null) return t.tr(`Samlet: ≈ ${councilUsd(total)}`, `Total: ≈ ${councilUsd(total)}`)
  return cost.zdr
    ? t.tr('Kostnaden vises ikke under Zero Data Retention.', 'The cost is not shown under Zero Data Retention.')
    : t.tr('Prislisten oppgir ikke pris for alle modellene.', 'The catalogue does not give a price for every model.')
}

function runButtonLabel(cost: CouncilEstimate, t: Translator): string {
  const total = cost.totalEstimatedCostUsd
  return total === null
    ? t.tr('Kjør', 'Run')
    : t.tr(`Kjør (≈ ${councilUsd(total)})`, `Run (≈ ${councilUsd(total)})`)
}

function totalAnswersLine(run: CouncilRun, t: Translator): string {
  const total = run.totalCostUsd
  return total === null
    ? t.tr('Samlet kostnad vises ikke.', 'The total cost is not shown.')
    : t.tr(`Samlet kostnad: ${councilUsd(total)}`, `Total cost: ${councilUsd(total)}`)
}

/** One member's answer, or why there is none. The answer is plain text, so it is shown as text and never as markup. */
function MemberAnswer(props: { member: CouncilMember; name: string; t: Translator }) {
  const missingText = (): string => {
    if (props.member.status === 'refused') {
      return props.t.tr('Avvist. Gatewayen lot ikke denne modellen svare.', 'Refused. The gateway did not let this model answer.')
    }
    if (props.member.code === 'empty_answer') {
      return props.t.tr('Modellen ga et tomt svar.', 'The model gave an empty answer.')
    }
    return props.t.tr('Svaret feilet. Prøv igjen.', 'The answer failed. Try again.')
  }
  return (
    <article
      class="verevon-model-council__card"
      data-status={props.member.status}
      data-code={props.member.code ?? undefined}
    >
      <h3 class="verevon-model-council__card-name">{props.name}</h3>
      <Show when={props.member.provider}>
        {(provider) => <p class="verevon-model-council__card-provider">{provider()}</p>}
      </Show>
      <Show
        when={props.member.answer}
        fallback={<p class="verevon-model-council__missing">{missingText()}</p>}
      >
        {(answer) => <p class="verevon-model-council__answer">{answer()}</p>}
      </Show>
      <Show when={props.member.usage}>
        {(usage) => <p class="verevon-model-council__usage">{usageLine(usage(), props.t)}</p>}
      </Show>
    </article>
  )
}

/** The synthesis: where the answers agree, where they disagree with each model's stance, and what only one model said. */
function SynthesisView(props: { synthesis: CouncilSynthesis; nameOf: (id: string) => string; t: Translator }) {
  const emptyText = (text: string) => <p class="verevon-model-council__column-empty">{text}</p>
  return (
    <section
      class="verevon-model-council__synthesis"
      data-status={props.synthesis.status}
      data-code={props.synthesis.code ?? undefined}
    >
      <h3 class="verevon-model-council__synthesis-title">
        {props.t.tr('Syntese', 'Synthesis')}
        <Show when={props.synthesis.model}>
          {(model) => <span class="verevon-model-council__synthesis-model"> · {props.nameOf(model())}</span>}
        </Show>
      </h3>
      <Show when={props.synthesis.status === 'structured'}>
        <div class="verevon-model-council__columns">
          <div>
            <h4 class="verevon-model-council__column-title">{props.t.tr('Der de er enige', 'Where they agree')}</h4>
            <Show
              when={props.synthesis.agreement.length > 0}
              fallback={emptyText(props.t.tr('Ingen felles punkter ble funnet.', 'No shared points were found.'))}
            >
              <ul class="verevon-model-council__column-list">
                <For each={props.synthesis.agreement}>{(claim) => <li>{claim}</li>}</For>
              </ul>
            </Show>
          </div>
          <div>
            <h4 class="verevon-model-council__column-title">{props.t.tr('Der de er uenige', 'Where they disagree')}</h4>
            <Show
              when={props.synthesis.disagreement.length > 0}
              fallback={emptyText(props.t.tr('Ingen uenighet ble funnet.', 'No disagreement was found.'))}
            >
              <ul class="verevon-model-council__column-list">
                <For each={props.synthesis.disagreement}>
                  {(item) => (
                    <li>
                      <p class="verevon-model-council__claim">{item.claim}</p>
                      <ul class="verevon-model-council__positions">
                        <For each={item.positions}>
                          {(position) => <li>{props.nameOf(position.model)}: {position.stance}</li>}
                        </For>
                      </ul>
                    </li>
                  )}
                </For>
              </ul>
            </Show>
          </div>
          <div>
            <h4 class="verevon-model-council__column-title">{props.t.tr('Bare én modell nevnte', 'Only one model said')}</h4>
            <Show
              when={props.synthesis.unique.length > 0}
              fallback={emptyText(props.t.tr('Ingen.', 'None.'))}
            >
              <ul class="verevon-model-council__column-list">
                <For each={props.synthesis.unique}>
                  {(item) => <li>{props.nameOf(item.model)}: {item.claim}</li>}
                </For>
              </ul>
            </Show>
          </div>
        </div>
      </Show>
      <Show when={props.synthesis.status === 'unstructured'}>
        <p class="verevon-model-council__status">
          {props.t.tr(
            'Svaret kunne ikke leses som strukturert, og vises derfor som tekst.',
            'The reply could not be read as a structure, so it is shown as text.',
          )}
        </p>
        <p class="verevon-model-council__text">{props.synthesis.text ?? ''}</p>
      </Show>
      <Show when={props.synthesis.status === 'failed'}>
        <p class="verevon-model-council__status" data-tone="alert">
          {props.t.tr('Syntesen kunne ikke lages.', 'The synthesis could not be made.')}
        </p>
      </Show>
      <Show when={props.synthesis.status === 'not_run'}>
        <p class="verevon-model-council__status">
          {props.t.tr(
            'Syntesen ble ikke laget, fordi færre enn to modeller svarte.',
            'No synthesis was made, because fewer than two models answered.',
          )}
        </p>
      </Show>
      <Show when={props.synthesis.usage}>
        {(usage) => <p class="verevon-model-council__usage">{usageLine(usage(), props.t)}</p>}
      </Show>
    </section>
  )
}

export function ModelCouncilPanel(props: { onClose: () => void }) {
  const i18n = useI18n()
  const titleId = createUniqueId()
  const questionId = createUniqueId()
  // The catalogue the model picker loads. A failed load reads as no models, which the panel says plainly.
  const [models] = createResource(async () => {
    try {
      return await listModels()
    } catch {
      return [] as ModelInfo[]
    }
  })
  const [selected, setSelected] = createSignal<string[]>([])
  const [allowSubscription, setAllowSubscription] = createSignal(false)
  const [question, setQuestion] = createSignal('')
  const [busy, setBusy] = createSignal<'estimate' | 'run' | null>(null)
  const [failure, setFailure] = createSignal<{ error: unknown } | null>(null)
  const [estimate, setEstimate] = createSignal<{ key: string; value: CouncilEstimate } | null>(null)
  const [answers, setAnswers] = createSignal<{ key: string; value: CouncilRun } | null>(null)

  const catalogue = createMemo(() => models() ?? [])
  const groups = createMemo(() => groupChatModels(catalogue()))
  const nameOf = (id: string): string => catalogue().find((model) => model.id === id)?.name ?? id
  const isSubscriptionModel = (id: string): boolean =>
    isCouncilSubscriptionProvider(catalogue().find((model) => model.id === id)?.provider)
  const questionLength = createMemo(() => Array.from(question().trim()).length)
  const currentInput = createMemo((): CouncilRunInput => ({
    question: question(),
    models: selected(),
    allowSubscription: allowSubscription(),
  }))
  // An estimate and its answers belong to one question and one set of models. Changing either takes them out of view.
  const inputKey = createMemo(() => JSON.stringify([question().trim(), selected(), allowSubscription()]))
  const ready = createMemo(() => councilInputAccepted(currentInput()))
  const shownEstimate = createMemo(() => {
    const current = estimate()
    return current && current.key === inputKey() ? current.value : null
  })
  const shownAnswers = createMemo(() => {
    const current = answers()
    return current && current.key === inputKey() ? current.value : null
  })

  // Escape closes the panel. It is mounted only while it is open, so its listener lives and ends with it.
  const onKey = (event: KeyboardEvent) => {
    if (event.key === 'Escape') props.onClose()
  }
  document.addEventListener('keydown', onKey)
  onCleanup(() => document.removeEventListener('keydown', onKey))

  // A subscription model needs the allowance, and the cap of four holds for every other model.
  function optionDisabled(model: ModelInfo): boolean {
    if (busy() !== null) return true
    if (isCouncilSubscriptionProvider(model.provider) && !allowSubscription()) return true
    return !selected().includes(model.id) && selected().length >= COUNCIL_MAX_MODELS
  }

  function toggleModel(id: string): void {
    setSelected((current) => {
      if (current.includes(id)) return current.filter((entry) => entry !== id)
      return current.length < COUNCIL_MAX_MODELS ? [...current, id] : current
    })
  }

  function setSubscriptionAllowed(allowed: boolean): void {
    setAllowSubscription(allowed)
    // The allowance taken back takes the subscription models already picked out with it.
    if (!allowed) setSelected((current) => current.filter((id) => !isSubscriptionModel(id)))
  }

  function failureText(error: unknown): string {
    if (error instanceof ApiError && error.code === 'model_council_disabled') {
      return i18n.tr(
        'Modellrådet er ikke slått på for denne installasjonen.',
        'The model council is not turned on for this installation.',
      )
    }
    if (error instanceof ApiError && error.code === 'delegated_auth_unavailable') {
      return i18n.tr(
        'Modelltilgangen er midlertidig utilgjengelig. Prøv igjen om litt.',
        'Model access is temporarily unavailable. Try again shortly.',
      )
    }
    const fallback = i18n.tr(
      'Modellrådet kunne ikke fullføre spørsmålet. Prøv igjen.',
      'The model council could not finish the question. Try again.',
    )
    return translateApiError(error, i18n.tr, { no: fallback, en: fallback })
  }

  // The estimate makes no model call, so it costs nothing and stores nothing.
  async function estimateCost(): Promise<void> {
    if (busy() !== null || !ready()) return
    const key = inputKey()
    const input = currentInput()
    setBusy('estimate')
    setFailure(null)
    try {
      const value = await estimateCouncil(input)
      setEstimate({ key, value })
    } catch (error) {
      setEstimate(null)
      setFailure({ error })
    } finally {
      setBusy(null)
    }
  }

  // Only the explicit Run button calls this, and only for the estimate the person is looking at.
  async function runAnswers(): Promise<void> {
    if (busy() !== null || shownEstimate() === null) return
    const key = inputKey()
    const input = currentInput()
    setBusy('run')
    setFailure(null)
    try {
      const value = await runCouncil(input)
      setAnswers({ key, value })
    } catch (error) {
      setFailure({ error })
    } finally {
      setBusy(null)
    }
  }

  return (
    <Portal>
      <div class="verevon-model-council-backdrop">
        <section role="dialog" aria-modal="true" aria-labelledby={titleId} class="verevon-model-council">
          <header class="verevon-model-council__header">
            <div>
              <h2 id={titleId} class="verevon-model-council__title">
                <GitCompare class="size-4 shrink-0" aria-hidden="true" />
                {i18n.tr('Sammenlign AI-modeller', 'Compare AI models')}
              </h2>
              <p class="verevon-model-council__lede">
                {i18n.tr(
                  'Samme spørsmål til to til fire modeller, uten verktøy. Du ser kostnaden før noe kjøres. Svarene lagres ikke.',
                  'The same question to two to four models, with no tools. You see the cost before anything runs. The answers are not stored.',
                )}
              </p>
            </div>
            <button
              type="button"
              class="verevon-model-council__close"
              aria-label={i18n.tr('Lukk', 'Close')}
              onClick={() => props.onClose()}
            >
              <X class="size-4" aria-hidden="true" />
            </button>
          </header>

          <div class="verevon-model-council__body">
            <fieldset class="verevon-model-council__models">
              <legend class="verevon-model-council__legend">
                <span>{i18n.tr('Modeller, to til fire', 'Models, two to four')}</span>
                <span class="verevon-model-council__count">
                  {i18n.tr(`${selected().length} valgt`, `${selected().length} selected`)}
                </span>
              </legend>
              <Show
                when={models() !== undefined}
                fallback={<p class="verevon-model-council__hint">{i18n.tr('Henter modeller …', 'Loading models …')}</p>}
              >
                <Show
                  when={catalogue().length > 0}
                  fallback={(
                    <p class="verevon-model-council__hint">
                      {i18n.tr('Ingen modeller er tilgjengelige akkurat nå.', 'No models are available right now.')}
                    </p>
                  )}
                >
                  <div class="verevon-model-council__groups">
                    <For each={groups()}>
                      {(group) => (
                        <div>
                          <p class="verevon-model-council__group-label">{group.label}</p>
                          <div class="verevon-model-council__options">
                            <For each={group.models}>
                              {(model) => (
                                <label
                                  class="verevon-model-council__option"
                                  data-selected={selected().includes(model.id) ? 'true' : 'false'}
                                  data-disabled={optionDisabled(model) ? 'true' : 'false'}
                                >
                                  <input
                                    type="checkbox"
                                    checked={selected().includes(model.id)}
                                    disabled={optionDisabled(model)}
                                    onChange={() => toggleModel(model.id)}
                                  />
                                  <span class="verevon-model-council__option-text">
                                    <span class="verevon-model-council__option-name">{model.name}</span>
                                    <span class="verevon-model-council__option-id">{model.id}</span>
                                  </span>
                                  <Show when={isCouncilSubscriptionProvider(model.provider)}>
                                    <span class="verevon-model-council__badge">{i18n.tr('Abonnement', 'Subscription')}</span>
                                  </Show>
                                </label>
                              )}
                            </For>
                          </div>
                        </div>
                      )}
                    </For>
                  </div>
                </Show>
              </Show>
            </fieldset>

            <label class="verevon-model-council__allow">
              <input
                type="checkbox"
                checked={allowSubscription()}
                disabled={busy() !== null}
                onChange={(event) => setSubscriptionAllowed(event.currentTarget.checked)}
              />
              <span>
                {i18n.tr(
                  'Tillat abonnementsmodeller i denne sammenligningen. De faktureres til abonnementet ditt, ikke per token.',
                  'Allow subscription models in this comparison. They are billed to your subscription, not per token.',
                )}
              </span>
            </label>

            <div class="verevon-model-council__field">
              <label for={questionId} class="verevon-model-council__label">
                {i18n.tr('Spørsmål', 'Question')}
              </label>
              <textarea
                id={questionId}
                class="verevon-model-council__textarea"
                rows={3}
                value={question()}
                disabled={busy() !== null}
                placeholder={i18n.tr(
                  'Skriv spørsmålet alle modellene skal svare på …',
                  'Write the question every model should answer …',
                )}
                onInput={(event) => setQuestion(event.currentTarget.value)}
              />
              <p
                class="verevon-model-council__counter"
                data-over={questionLength() > COUNCIL_QUESTION_MAX_CHARS ? 'true' : 'false'}
              >
                {`${questionLength()} / ${COUNCIL_QUESTION_MAX_CHARS}`}
              </p>
            </div>

            <div class="verevon-model-council__actions">
              <button
                type="button"
                class="verevon-model-council__button"
                disabled={!ready() || busy() !== null}
                onClick={() => void estimateCost()}
              >
                <Show when={busy() === 'estimate'}>
                  <Loader2 class="verevon-model-council__spinner size-4" aria-hidden="true" />
                </Show>
                {busy() === 'estimate'
                  ? i18n.tr('Vurderer kostnaden …', 'Estimating the cost …')
                  : i18n.tr('Vurder kostnaden', 'Estimate the cost')}
              </button>
              <Show when={!ready()}>
                <p class="verevon-model-council__hint">
                  {i18n.tr(
                    `Velg to til fire modeller, og skriv et spørsmål på høyst ${COUNCIL_QUESTION_MAX_CHARS} tegn.`,
                    `Pick two to four models, and write a question of at most ${COUNCIL_QUESTION_MAX_CHARS} characters.`,
                  )}
                </p>
              </Show>
            </div>

            <Show when={shownEstimate()}>
              {(cost) => (
                <div class="verevon-model-council__estimate" role="status" aria-live="polite">
                  <p class="verevon-model-council__estimate-title">
                    {i18n.tr('Kostnadsestimat, før kjøring', 'Cost estimate, before the run')}
                  </p>
                  <ul class="verevon-model-council__list">
                    <For each={cost().members}>
                      {(call) => <li>{estimateCallLine(call, nameOf(call.model), i18n)}</li>}
                    </For>
                    <li>
                      {estimateCallLine(
                        cost().synthesis,
                        `${i18n.tr('Syntese', 'Synthesis')} (${nameOf(cost().synthesis.model)})`,
                        i18n,
                      )}
                    </li>
                  </ul>
                  <Show when={cost().dropped.length > 0}>
                    <p class="verevon-model-council__note">
                      {i18n.tr('Utelatt, fordi abonnement ikke er tillatt: ', 'Left out, because subscriptions are not allowed: ')}
                      {cost().dropped.map((item) => nameOf(item.model)).join(', ')}
                    </p>
                  </Show>
                  <p class="verevon-model-council__total">{totalEstimateLine(cost(), i18n)}</p>
                  <button
                    type="button"
                    class="verevon-model-council__button verevon-model-council__button--primary"
                    disabled={busy() !== null}
                    onClick={() => void runAnswers()}
                  >
                    <Show when={busy() === 'run'}>
                      <Loader2 class="verevon-model-council__spinner size-4" aria-hidden="true" />
                    </Show>
                    {busy() === 'run' ? i18n.tr('Kjører …', 'Running …') : runButtonLabel(cost(), i18n)}
                  </button>
                  <Show when={busy() === 'run'}>
                    <p class="verevon-model-council__busy-note">
                      {i18n.tr(
                        'Modellene svarer samtidig, og syntesen kommer til slutt. Dette kan ta en stund.',
                        'The models answer at the same time, and the synthesis comes last. This can take a while.',
                      )}
                    </p>
                  </Show>
                </div>
              )}
            </Show>

            <Show when={failure()}>
              {(failed) => (
                <p role="alert" class="verevon-model-council__alert">
                  <CircleAlert class="size-4 shrink-0" aria-hidden="true" />
                  <span>{failureText(failed().error)}</span>
                </p>
              )}
            </Show>

            <Show when={shownAnswers()}>
              {(run) => (
                <section class="verevon-model-council__answers" aria-label={i18n.tr('Svar', 'Answers')} aria-live="polite">
                  <div class="verevon-model-council__cards">
                    <For each={run().members}>
                      {(member) => <MemberAnswer member={member} name={nameOf(member.model)} t={i18n} />}
                    </For>
                  </div>
                  <SynthesisView synthesis={run().synthesis} nameOf={nameOf} t={i18n} />
                  <p class="verevon-model-council__answers-total">{totalAnswersLine(run(), i18n)}</p>
                  <Show when={run().dropped.length > 0}>
                    <p class="verevon-model-council__dropped">
                      {i18n.tr('Utelatt: ', 'Left out: ')}
                      {run().dropped.map((item) => nameOf(item.model)).join(', ')}
                    </p>
                  </Show>
                </section>
              )}
            </Show>
          </div>
        </section>
      </div>
    </Portal>
  )
}
