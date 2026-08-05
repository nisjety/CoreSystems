import { Show } from 'solid-js'
import type { AssistResult } from '@/features/inbox/lib/inbox-ai'
import { useI18n } from '@/shared/i18n'

export type SupportAssistRunMetadata = Pick<AssistResult, 'model' | 'supportAiMode' | 'usage' | 'zdr'>

/**
 * Describes only facts returned for the current Support-assist invocation.
 * Usage values remain absent unless the exact non-streaming Model Gateway
 * response reported them. Confidence is a heuristic answer-quality signal,
 * never a predicted customer or action outcome.
 */
export function SupportAssistDisclosure(props: { metadata: SupportAssistRunMetadata }) {
  const i18n = useI18n()
  const policy = () => props.metadata.supportAiMode === 'review'
    ? i18n.tr('Gjennomgangsmodus', 'Review mode')
    : i18n.tr('Assist-modus', 'Assist mode')
  const policyDetail = () => props.metadata.supportAiMode === 'review'
    ? i18n.tr('Et operatørvalg kan opprette et forslag til gjennomgang. Ingenting utføres automatisk.', 'An operator choice can create a proposal for review. Nothing is executed automatically.')
    : i18n.tr('Svar og utkast er midlertidige. Ingenting lagres som et forslag.', 'Answers and drafts are transient. Nothing is retained as a proposal.')
  const usage = () => props.metadata.usage
  const hasUsage = () => Boolean(usage() && Object.values(usage()!).some((value) => value !== undefined))
  const tokens = () => usage()?.inputTokens !== undefined || usage()?.outputTokens !== undefined
    ? `${usage()?.inputTokens ?? 0} ${i18n.tr('inndata', 'input')} · ${usage()?.outputTokens ?? 0} ${i18n.tr('utdata-tokens', 'output tokens')}`
    : undefined
  const cost = () => usage()?.costUsd
  const latency = () => usage()?.latencyMs
  const confidence = () => usage()?.confidence

  return (
    <section class="verevon-support-assist-disclosure" aria-label={i18n.tr('Kjøringsinformasjon', 'Run information')}>
      <strong>{i18n.tr('Kjøringsinformasjon', 'Run information')}</strong>
      <dl>
        <div>
          <dt>{i18n.tr('Policy', 'Policy')}</dt>
          <dd>
            <b>{policy()}</b>
            <span>{policyDetail()}</span>
          </dd>
        </div>
        <div>
          <dt>{i18n.tr('Lagring', 'Retention')}</dt>
          <dd>
            <b>{props.metadata.zdr ? i18n.tr('Ingen datalagring', 'Zero Data Retention') : i18n.tr('Standard lagring', 'Standard retention')}</b>
            <span>{i18n.tr('Organisasjonens konfigurerte innstilling for denne kjøringen.', 'The organization’s configured setting for this run.')}</span>
          </dd>
        </div>
        <div>
          <dt>{i18n.tr('Modell', 'Model')}</dt>
          <dd>
            <Show
              when={props.metadata.model?.trim()}
              fallback={<b>{i18n.tr('Modellidentitet ble ikke rapportert', 'Model identity was not reported')}</b>}
            >
              {(model) => <b>{model()}</b>}
            </Show>
          </dd>
        </div>
        <Show when={hasUsage()}>
          <div>
            <dt>{i18n.tr('Bruk', 'Usage')}</dt>
            <dd>
              <Show when={tokens()}>{(value) => <span>{value()}</span>}</Show>
              <Show when={latency() !== undefined}><span>{latency()} ms</span></Show>
              <Show when={cost() !== undefined}><span>{cost()!.toFixed(5)} USD</span></Show>
              <Show when={confidence() !== undefined}>
                <span>{Math.round((confidence() ?? 0) * 100)}% {i18n.tr('heuristisk signal for svarkvalitet', 'heuristic answer-quality signal')}</span>
              </Show>
            </dd>
          </div>
        </Show>
      </dl>
      <small>{hasUsage()
        ? i18n.tr('Bruksdataene gjelder bare denne kjøringen. Det heuristiske signalet er ikke en garanti for kundeløsning eller handling.', 'Usage applies only to this run. The heuristic signal is not a guarantee of a customer outcome or action.')
        : i18n.tr('Sikkerhet og kostnad ble ikke rapportert av Support-modellsvar.', 'Confidence and cost were not reported by the Support model response.')}</small>
    </section>
  )
}
