import { createMemo, For, Show } from 'solid-js'
import { parseResearchProvenance } from '@/shared/api/chat-client'
import { useI18n } from '@/shared/i18n'
import { formatMoment } from '@/shared/i18n/format'
import { isSafeLinkHref } from '../lib/safe-href'

/**
 * The provenance stamp under a research report's artifact card (EP-2-S4): what the report was built from, the
 * coverage it reached and the model that wrote it. Takes the stamp as it arrived, live or from a stored turn, and
 * renders nothing unless it is complete, so a report without one (the switch off, an older turn, a ZDR or waived
 * turn reloaded) looks exactly as before.
 */
export function ArtifactProvenance(props: { provenance: unknown }) {
  const i18n = useI18n()
  const stamp = createMemo(() => parseResearchProvenance(props.provenance))
  const when = (iso: string) => formatMoment(iso, i18n.locale())
  return (
    <Show when={stamp()}>
      {(value) => (
        <footer class="verevon-chat-artifact-provenance" aria-label={i18n.tr('Hvordan rapporten ble laget', 'How this report was made')}>
          <p class="verevon-chat-artifact-provenance__summary">
            {i18n.tr(
              `Bygget fra ${value().coverage.read} av ${value().coverage.found} kilder · ${value().model} · ${when(value().generated_at)}`,
              `Built from ${value().coverage.read} of ${value().coverage.found} sources · ${value().model} · ${when(value().generated_at)}`,
            )}
          </p>
          <Show when={value().coverage.deadline_hit}>
            <p class="verevon-chat-artifact-provenance__warning" role="note">
              {i18n.tr('Tiden gikk ut før alle kildene var lest.', 'Time ran out before every source was read.')}
            </p>
          </Show>
          <details class="verevon-chat-artifact-provenance__details">
            <summary>{i18n.tr('Spørsmål, delspørsmål og kilder', 'Question, sub-queries and sources')}</summary>
            <p>{value().question}</p>
            <Show when={value().sub_queries.length > 0}>
              <ul class="verevon-chat-artifact-provenance__queries">
                <For each={value().sub_queries}>{(query) => <li>{query}</li>}</For>
              </ul>
            </Show>
            <ol class="verevon-chat-artifact-provenance__sources">
              <For each={value().sources}>
                {(source) => (
                  <li value={source.number}>
                    {/* A search-supplied address: an unsafe one keeps its title but is not a link. */}
                    <Show when={isSafeLinkHref(source.url)} fallback={<span>{source.title || source.url}</span>}>
                      <a href={source.url} target="_blank" rel="noopener noreferrer">{source.title || source.url}</a>
                    </Show>
                  </li>
                )}
              </For>
            </ol>
          </details>
        </footer>
      )}
    </Show>
  )
}
