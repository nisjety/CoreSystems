/**
 * The request inspector (EP-14 S7): what one model call of the reader's own run was sent and offered.
 *
 * The server already applied decision D3, so this view never holds platform prompt text to hide: a
 * platform section arrives as its label, version digest and length, and only organisation, user and
 * memory sections carry text. The view shows the routed model and parameters, the prompt sections in
 * order, the tool catalogue and the message manifest (lengths and digests, never message bodies).
 * "Copy request JSON" copies exactly what is shown. When the run dropped snapshots, the view says the
 * record is incomplete instead of presenting what it has as the whole run.
 *
 * Behind `VITE_CHAT_REQUEST_INSPECTOR`; the ledger inspector mounts it per run, and nothing is fetched
 * until the reader asks.
 */

import { createSignal, For, Match, Show, Switch } from 'solid-js'
import { getRunRequestSnapshot, type RunRequestSection, type RunRequestSnapshot } from '@/shared/api/run-console-client'
import { useI18n } from '@/shared/i18n'

/** Only the literal `true` enables the inspector, as for the other chat switches. */
export function isRequestInspectorEnabled(raw: string | undefined): boolean {
  return raw === 'true'
}

/** Fixed per build, like every `VITE_` switch: read once, at load. */
export const requestInspectorEnabled = isRequestInspectorEnabled(import.meta.env.VITE_CHAT_REQUEST_INSPECTOR)

type ToolEntry = { name: string; description: string; parameters: string }

/** The offered tools as the catalogue names them; an entry without a name is skipped, not guessed. */
export function toolEntries(tools: readonly unknown[] | null): ToolEntry[] {
  return (tools ?? []).flatMap((tool) => {
    if (!tool || typeof tool !== 'object') return []
    const item = tool as Record<string, unknown>
    if (typeof item.name !== 'string' || item.name === '') return []
    return [{
      name: item.name,
      description: typeof item.description === 'string' ? item.description : '',
      parameters: typeof item.parameters_json === 'string' ? item.parameters_json : '',
    }]
  })
}

type ManifestEntry = { role: string; sourceTag: string; chars: number; sha256: string }

/** The snapshot's message manifest: role, source tag, length and digest of each message, no bodies. */
export function manifestEntries(snapshot: Record<string, unknown> | null): ManifestEntry[] {
  const messages = snapshot?.messages
  if (!Array.isArray(messages)) return []
  return messages.flatMap((message) => {
    if (!message || typeof message !== 'object') return []
    const item = message as Record<string, unknown>
    return [{
      role: typeof item.role === 'string' ? item.role : '',
      sourceTag: typeof item.source_tag === 'string' ? item.source_tag : '',
      chars: typeof item.chars === 'number' ? item.chars : 0,
      sha256: typeof item.sha256 === 'string' ? item.sha256 : '',
    }]
  })
}

/** What "Copy request JSON" copies: the shown record, which is all the server returned. */
export function requestJson(request: RunRequestSnapshot): string {
  return JSON.stringify({
    run_id: request.runId,
    call_seq: request.callSeq,
    call_purpose: request.callPurpose,
    snapshot: request.snapshot,
    system_sections: request.sections,
    tools: request.tools,
  }, null, 2)
}

const shortDigest = (digest: string) => digest.slice(0, 12)

type PanelState =
  | { kind: 'idle' }
  | { kind: 'loading' }
  | { kind: 'unavailable' }
  | { kind: 'loaded'; request: RunRequestSnapshot }

type Loader = (runId: string, callSeq: number) => Promise<RunRequestSnapshot>

export function RequestInspector(props: { runId: string; load?: Loader }) {
  const i18n = useI18n()
  const [callSeq, setCallSeq] = createSignal(1)
  const [state, setState] = createSignal<PanelState>({ kind: 'idle' })
  const [copyStatus, setCopyStatus] = createSignal('')

  const load = async (seq: number) => {
    setCallSeq(seq)
    setCopyStatus('')
    setState({ kind: 'loading' })
    try {
      const request = await (props.load ?? getRunRequestSnapshot)(props.runId, seq)
      setState({ kind: 'loaded', request })
    } catch {
      // Another user's run, a call that was never captured, a zero-retention session and the switch
      // being off all answer the same way, so the view does not tell them apart either.
      setState({ kind: 'unavailable' })
    }
  }

  const loaded = () => {
    const current = state()
    return current.kind === 'loaded' ? current.request : undefined
  }

  const copy = async (request: RunRequestSnapshot) => {
    try {
      await navigator.clipboard.writeText(requestJson(request))
      setCopyStatus(i18n.tr('Forespørselen er kopiert som JSON.', 'Request copied as JSON.'))
    } catch {
      setCopyStatus(i18n.tr('Kunne ikke kopiere forespørselen.', 'Could not copy the request.'))
    }
  }

  const sourceLabel = (section: RunRequestSection) => {
    switch (section.source) {
      case 'org': return i18n.tr('Organisasjon', 'Organisation')
      case 'user': return i18n.tr('Deg', 'You')
      case 'memory': return i18n.tr('Minne', 'Memory')
      default: return i18n.tr('Plattform', 'Platform')
    }
  }

  return (
    <section class="verevon-chat-request" aria-label={i18n.tr('Forespørsel til modellen', 'Model request')}>
      <Switch>
        <Match when={state().kind === 'idle'}>
          <button type="button" class="verevon-chat-ledger__row" onClick={() => void load(1)}>
            {i18n.tr('Vis hva modellen fikk', 'Show what the model was sent')}
          </button>
        </Match>
        <Match when={state().kind === 'loading'}>
          <p class="verevon-chat-ledger__note" role="status">{i18n.tr('Henter forespørselen …', 'Loading the request…')}</p>
        </Match>
        <Match when={state().kind === 'unavailable'}>
          <p class="verevon-chat-ledger__note" role="status">
            {i18n.tr(
              `Kall ${callSeq()} er ikke tilgjengelig for denne kjøringen.`,
              `Call ${callSeq()} is not available for this run.`,
            )}
          </p>
          <Show when={callSeq() > 1}>
            <button type="button" onClick={() => void load(callSeq() - 1)}>{i18n.tr('Forrige kall', 'Previous call')}</button>
          </Show>
        </Match>
        <Match when={loaded()}>
          {(request) => (
            <>
              <header class="verevon-chat-request__header">
                <h4>{i18n.tr(`Kall ${request().callSeq}`, `Call ${request().callSeq}`)} · {request().callPurpose}</h4>
                <button type="button" disabled={request().callSeq <= 1} onClick={() => void load(request().callSeq - 1)}>
                  {i18n.tr('Forrige kall', 'Previous call')}
                </button>
                <button type="button" onClick={() => void load(request().callSeq + 1)}>
                  {i18n.tr('Neste kall', 'Next call')}
                </button>
                <button type="button" onClick={() => void copy(request())}>
                  {i18n.tr('Kopier forespørsel som JSON', 'Copy request JSON')}
                </button>
              </header>
              <Show when={request().dropped > 0}>
                <p class="verevon-chat-request__incomplete" role="note">
                  {i18n.tr(
                    `Denne oversikten er ufullstendig: ${request().dropped} kall ble ikke registrert.`,
                    `This record is incomplete: ${request().dropped} calls were not captured.`,
                  )}
                </p>
              </Show>
              <dl class="verevon-run-proof__receipt">
                <div class="verevon-run-proof__receipt-row">
                  <dt>{i18n.tr('Modell', 'Model')}</dt>
                  <dd><code>{String(request().snapshot?.model ?? '')}</code> <code>{String(request().snapshot?.provider ?? '')}</code></dd>
                </div>
                <div class="verevon-run-proof__receipt-row">
                  <dt>{i18n.tr('Parametere', 'Parameters')}</dt>
                  <dd><code>{JSON.stringify(request().snapshot?.params ?? {})}</code></dd>
                </div>
              </dl>
              <h5>{i18n.tr('Instruksjoner', 'Prompt sections')}</h5>
              <Show when={request().systemTruncated}>
                <p class="verevon-chat-ledger__note">
                  {i18n.tr('Instruksjonene ble kuttet ved lagring; deler etter kuttet vises uten tekst.', 'The prompt was cut when stored; sections past the cut show no text.')}
                </p>
              </Show>
              <ol class="verevon-chat-request__sections">
                <For each={request().sections}>
                  {(section) => (
                    <li data-section-source={section.source}>
                      <strong>{section.label}</strong> · {sourceLabel(section)} · {section.chars} {i18n.tr('tegn', 'chars')} ·{' '}
                      <code title={section.digest}>{shortDigest(section.digest)}</code>
                      <Show
                        when={section.text}
                        fallback={<p class="verevon-chat-ledger__note">{i18n.tr('Teksten vises ikke.', 'Text not shown.')}</p>}
                      >
                        {(text) => <pre class="verevon-chat-request__text">{text()}</pre>}
                      </Show>
                    </li>
                  )}
                </For>
              </ol>
              <h5>{i18n.tr('Verktøy som ble tilbudt', 'Tools offered')}</h5>
              <Show when={request().toolsTruncated}>
                <p class="verevon-chat-ledger__note">
                  {i18n.tr('Verktøylisten ble kuttet ved lagring.', 'The tool catalogue was cut when stored.')}
                </p>
              </Show>
              <Show
                when={toolEntries(request().tools).length > 0}
                fallback={<p class="verevon-chat-ledger__note">{i18n.tr('Ingen verktøy ble tilbudt.', 'No tools were offered.')}</p>}
              >
                <ul class="verevon-chat-request__tools">
                  <For each={toolEntries(request().tools)}>
                    {(tool) => (
                      <li>
                        <details>
                          <summary><code>{tool.name}</code> {tool.description}</summary>
                          <pre>{tool.parameters}</pre>
                        </details>
                      </li>
                    )}
                  </For>
                </ul>
              </Show>
              <h5>{i18n.tr('Meldinger', 'Messages')}</h5>
              <table class="verevon-chat-request__manifest">
                <thead>
                  <tr>
                    <th scope="col">{i18n.tr('Rolle', 'Role')}</th>
                    <th scope="col">{i18n.tr('Kilde', 'Source')}</th>
                    <th scope="col">{i18n.tr('Tegn', 'Chars')}</th>
                    <th scope="col">{i18n.tr('Sjekksum', 'Digest')}</th>
                  </tr>
                </thead>
                <tbody>
                  <For each={manifestEntries(request().snapshot)}>
                    {(entry) => (
                      <tr>
                        <td>{entry.role}</td>
                        <td>{entry.sourceTag}</td>
                        <td>{entry.chars}</td>
                        <td><code title={entry.sha256}>{shortDigest(entry.sha256)}</code></td>
                      </tr>
                    )}
                  </For>
                </tbody>
              </table>
              <p class="verevon-chat-ledger__copy-status" role="status">{copyStatus()}</p>
            </>
          )}
        </Match>
      </Switch>
    </section>
  )
}
