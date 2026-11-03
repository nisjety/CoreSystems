import { ArrowLeft } from '@/shared/icons'
import { For, Show, createSignal } from 'solid-js'
import { useParams } from '@solidjs/router'

import { listModels } from '@/shared/api/chat-client'
import { loadComposerSettingsItems } from '@/shared/api/composer-settings-client'
import { requestJson } from '@/shared/api/http'
import { createResource } from '@/shared/lib/create-resource-compat'
import { useI18n } from '@/shared/i18n'

/**
 * The agent builder (EP-6-S8, behind VITE_AGENT_BUILDER and the BFF's AGENT_DEFINITION_VERSIONS).
 * It edits one draft and publishes it as an immutable numbered version (OD-4 (b)); the history
 * below shows every published version with what changed in its instructions. Who may open it is
 * decided by the BFF (org admin, the creator, or a manager of the room a room-born agent came
 * from); this page only shows the refusal it gets.
 */

export type AgentAutonomyCap = 'read_only' | 'ask_first' | 'auto'

export type AgentDraft = {
  name: string
  system_prompt: string
  model: string
  tool_capability_ids: string[]
  autonomy_cap: AgentAutonomyCap
}

export type AgentDraftState = {
  agent_ref: string
  name: string
  published_version: number | null
  draft: AgentDraft | null
  current: { system_prompt: string | null; model: string | null }
}

export type AgentVersion = AgentDraft & {
  version: number
  digest: string
  published_by: string
  published_at: number
}

type ToolOption = { id: string; name: string }

// The shape capability ids take in the WS05 registry; the BFF refuses anything else.
const CAPABILITY_ID = /^[a-z][a-z0-9_.-]{0,127}$/
const AUTONOMY_CAPS: readonly AgentAutonomyCap[] = ['read_only', 'ask_first', 'auto']

function agentPath(agentRef: string, leaf: 'draft' | 'publish' | 'versions'): string {
  return `/api/v1/agents/${encodeURIComponent(agentRef)}/${leaf}`
}

export function getAgentDraft(agentRef: string): Promise<AgentDraftState> {
  return requestJson<AgentDraftState>(agentPath(agentRef, 'draft'))
}

export async function saveAgentDraft(agentRef: string, draft: AgentDraft): Promise<AgentDraft> {
  const response = await requestJson<{ draft: AgentDraft }>(agentPath(agentRef, 'draft'), {
    method: 'PUT',
    body: JSON.stringify(draft),
  })
  return response.draft
}

export function publishAgent(agentRef: string): Promise<{ version: number; digest: string }> {
  return requestJson(agentPath(agentRef, 'publish'), { method: 'POST' })
}

export async function listAgentVersions(agentRef: string): Promise<AgentVersion[]> {
  const response = await requestJson<{ versions: AgentVersion[] }>(agentPath(agentRef, 'versions'))
  return response.versions
}

export type DiffLine = { kind: 'same' | 'added' | 'removed'; text: string }

/**
 * A line diff of two instruction texts (longest common subsequence). Instructions are capped at
 * 4000 characters, so the quadratic table stays small.
 */
export function diffInstructionLines(before: string, after: string): DiffLine[] {
  const a = before === '' ? [] : before.split('\n')
  const b = after === '' ? [] : after.split('\n')
  const table: number[][] = Array.from({ length: a.length + 1 }, () => new Array<number>(b.length + 1).fill(0))
  for (let i = a.length - 1; i >= 0; i--) {
    for (let j = b.length - 1; j >= 0; j--) {
      table[i]![j] = a[i] === b[j] ? table[i + 1]![j + 1]! + 1 : Math.max(table[i + 1]![j]!, table[i]![j + 1]!)
    }
  }
  const lines: DiffLine[] = []
  let i = 0
  let j = 0
  while (i < a.length && j < b.length) {
    if (a[i] === b[j]) {
      lines.push({ kind: 'same', text: a[i]! })
      i++
      j++
    } else if (table[i + 1]![j]! >= table[i]![j + 1]!) {
      lines.push({ kind: 'removed', text: a[i]! })
      i++
    } else {
      lines.push({ kind: 'added', text: b[j]! })
      j++
    }
  }
  for (; i < a.length; i++) lines.push({ kind: 'removed', text: a[i]! })
  for (; j < b.length; j++) lines.push({ kind: 'added', text: b[j]! })
  return lines
}

/** The draft a first edit starts from: a saved draft, else the agent's current fields. */
export function seedDraft(state: AgentDraftState): AgentDraft {
  if (state.draft) return state.draft
  return {
    name: state.name,
    system_prompt: state.current.system_prompt ?? '',
    model: state.current.model ?? '',
    tool_capability_ids: [],
    // The middle of the WS25-S6 ladder: the author chooses to loosen or tighten it.
    autonomy_cap: 'ask_first',
  }
}

async function loadToolOptions(): Promise<ToolOption[]> {
  try {
    const items = await loadComposerSettingsItems('/api/v1/capabilities', 'capabilities')
    return items.filter((item) => CAPABILITY_ID.test(item.id)).map((item) => ({ id: item.id, name: item.name }))
  } catch {
    return []
  }
}

async function loadModelOptions(): Promise<string[]> {
  try {
    // The catalogue the gateway serves is already the caller's entitled set (WS10).
    return (await listModels()).filter((model) => model.modality === undefined || model.modality === 'chat').map((model) => model.id)
  } catch {
    return []
  }
}

export default function AgentBuilderPage(props: { readonly agentRef?: string }) {
  const i18n = useI18n()
  const params = useParams<{ agentId: string }>()
  const agentRef = () => props.agentRef ?? params.agentId ?? ''

  const [loaded] = createResource(agentRef, async (ref) => {
    const [state, models, tools] = await Promise.all([getAgentDraft(ref), loadModelOptions(), loadToolOptions()])
    return { state, models, tools }
  })

  return (
    <div class="verevon-agent-builder">
      <header class="verevon-agent-installations__topbar">
        <a href="/agents/installations" link class="verevon-agent-installations__back" aria-label={i18n.tr('Tilbake til agenter', 'Back to agents')}>
          <ArrowLeft size={16} />
        </a>
        <div>
          <p class="verevon-space-eyebrow">{i18n.tr('Agenter', 'Agents')}</p>
          <h1>{i18n.tr('Rediger agent', 'Edit agent')}</h1>
        </div>
      </header>

      <Show when={loaded.loading}>
        <p class="verevon-space-inline-status" role="status">{i18n.tr('Henter agenten …', 'Loading the agent…')}</p>
      </Show>

      <Show when={loaded.error}>
        <p class="verevon-space-projection-error" role="alert">
          {i18n.tr(
            'Agenten kunne ikke åpnes. Bare en administrator, den som opprettet agenten, eller en leder i rommet den ble laget i kan redigere den.',
            'The agent could not be opened. Only an admin, the person who created the agent, or a manager of the room it was made in can edit it.',
          )}
        </p>
      </Show>

      <Show when={loaded.error ? undefined : loaded()}>
        {(data) => <AgentBuilderForm agentRef={agentRef()} state={data().state} models={data().models} tools={data().tools} />}
      </Show>
    </div>
  )
}

function AgentBuilderForm(props: {
  readonly agentRef: string
  readonly state: AgentDraftState
  readonly models: readonly string[]
  readonly tools: readonly ToolOption[]
}) {
  const i18n = useI18n()
  // Seeded once from what the page loaded; later loads do not overwrite what the author is typing.
  const seed = seedDraft(props.state)
  const [name, setName] = createSignal(seed.name)
  const [instructions, setInstructions] = createSignal(seed.system_prompt)
  const [model, setModel] = createSignal(seed.model)
  const [tools, setTools] = createSignal<string[]>([...seed.tool_capability_ids])
  const [cap, setCap] = createSignal<AgentAutonomyCap>(seed.autonomy_cap)
  const [hasDraft, setHasDraft] = createSignal(props.state.draft !== null)
  const [busy, setBusy] = createSignal(false)
  const [notice, setNotice] = createSignal<{ tone: 'status' | 'alert'; text: string } | null>(null)
  const [versions, { refetch: refetchVersions }] = createResource(() => props.agentRef, listAgentVersions)

  // A saved model the catalogue no longer lists is kept selectable, so opening the builder never
  // silently changes it.
  const modelOptions = () => (model() && !props.models.includes(model()) ? [model(), ...props.models] : [...props.models])
  const capLabel = (value: AgentAutonomyCap) =>
    value === 'read_only'
      ? i18n.tr('Bare lese', 'Read only')
      : value === 'ask_first'
        ? i18n.tr('Spør først', 'Ask first')
        : i18n.tr('Automatisk', 'Automatic')

  const toggleTool = (id: string, on: boolean) =>
    setTools((current) => (on ? [...new Set([...current, id])] : current.filter((tool) => tool !== id)))

  const currentDraft = (): AgentDraft => ({
    name: name().trim(),
    system_prompt: instructions(),
    model: model(),
    tool_capability_ids: tools(),
    autonomy_cap: cap(),
  })

  const save = async () => {
    setBusy(true)
    setNotice(null)
    try {
      await saveAgentDraft(props.agentRef, currentDraft())
      setHasDraft(true)
      setNotice({ tone: 'status', text: i18n.tr('Utkastet er lagret. Ingenting endres før du publiserer.', 'Draft saved. Nothing changes until you publish.') })
    } catch {
      setNotice({ tone: 'alert', text: i18n.tr('Utkastet kunne ikke lagres.', 'The draft could not be saved.') })
    } finally {
      setBusy(false)
    }
  }

  const publish = async () => {
    setBusy(true)
    setNotice(null)
    try {
      // Publish what is on screen, not an older saved draft.
      await saveAgentDraft(props.agentRef, currentDraft())
      setHasDraft(true)
      const published = await publishAgent(props.agentRef)
      setNotice({ tone: 'status', text: i18n.tr(`Publisert som versjon ${published.version}.`, `Published as version ${published.version}.`) })
      await refetchVersions()
    } catch {
      setNotice({ tone: 'alert', text: i18n.tr('Agenten kunne ikke publiseres. Gjeldende versjon er uendret.', 'The agent could not be published. The current version is unchanged.') })
    } finally {
      setBusy(false)
    }
  }

  return (
    <div class="verevon-agent-builder__body">
      <section class="verevon-agent-builder__form" aria-label={i18n.tr('Utkast', 'Draft')}>
        <label class="verevon-agent-builder__field">
          <span>{i18n.tr('Navn', 'Name')}</span>
          <input value={name()} maxlength={60} onInput={(event) => setName(event.currentTarget.value)} />
        </label>

        <label class="verevon-agent-builder__field">
          <span>{i18n.tr('Instruksjoner', 'Instructions')}</span>
          <textarea rows={10} maxlength={4000} value={instructions()} onInput={(event) => setInstructions(event.currentTarget.value)} />
        </label>

        <label class="verevon-agent-builder__field">
          <span>{i18n.tr('Modell', 'Model')}</span>
          <select value={model()} onChange={(event) => setModel(event.currentTarget.value)}>
            <For each={modelOptions()}>{(option) => <option value={option}>{option}</option>}</For>
          </select>
        </label>

        <fieldset class="verevon-agent-builder__field">
          <legend>{i18n.tr('Verktøy', 'Tools')}</legend>
          <Show
            when={props.tools.length > 0}
            fallback={<p>{i18n.tr('Ingen verktøy er tilgjengelige i organisasjonen.', 'No tools are available in this organization.')}</p>}
          >
            <For each={props.tools}>
              {(tool) => (
                <label class="verevon-agent-builder__tool">
                  <input
                    type="checkbox"
                    checked={tools().includes(tool.id)}
                    onChange={(event) => toggleTool(tool.id, event.currentTarget.checked)}
                  />
                  {tool.name}
                </label>
              )}
            </For>
          </Show>
        </fieldset>

        <fieldset class="verevon-agent-builder__field">
          <legend>{i18n.tr('Høyeste selvstendighet', 'Autonomy cap')}</legend>
          <For each={AUTONOMY_CAPS}>
            {(value) => (
              <label>
                <input type="radio" name="autonomy-cap" value={value} checked={cap() === value} onChange={() => setCap(value)} />
                {capLabel(value)}
              </label>
            )}
          </For>
        </fieldset>

        <div class="verevon-agent-builder__actions">
          <button type="button" disabled={busy()} onClick={() => void save()}>
            {i18n.tr('Lagre utkast', 'Save draft')}
          </button>
          <button type="button" disabled={busy() || name().trim().length < 2 || !model()} onClick={() => void publish()}>
            {i18n.tr('Publiser', 'Publish')}
          </button>
        </div>
        <Show when={!hasDraft()}>
          <p class="verevon-agent-builder__hint">
            {i18n.tr('Agenten har ikke noe lagret utkast ennå. Feltene viser agenten slik den er nå.', 'The agent has no saved draft yet. The fields show the agent as it is now.')}
          </p>
        </Show>
        <Show when={notice()}>
          {(current) => <p role={current().tone}>{current().text}</p>}
        </Show>
      </section>

      <section class="verevon-agent-builder__history" aria-label={i18n.tr('Publiseringshistorikk', 'Publish history')}>
        <h2>{i18n.tr('Publiseringshistorikk', 'Publish history')}</h2>
        <Show when={versions.error}>
          <p role="alert">{i18n.tr('Historikken kunne ikke hentes.', 'The history could not be loaded.')}</p>
        </Show>
        <Show when={versions.error ? undefined : versions()}>
          {(list) => (
            <Show
              when={list().length > 0}
              fallback={<p>{i18n.tr('Ingen versjon er publisert ennå.', 'No version has been published yet.')}</p>}
            >
              <ol class="verevon-agent-builder__versions">
                <For each={list()}>
                  {(version, index) => <VersionRow version={version} previous={list()[index() + 1]} />}
                </For>
              </ol>
            </Show>
          )}
        </Show>
      </section>
    </div>
  )
}

function VersionRow(props: { readonly version: AgentVersion; readonly previous: AgentVersion | undefined }) {
  const i18n = useI18n()
  const [open, setOpen] = createSignal(false)
  const diff = () => diffInstructionLines(props.previous?.system_prompt ?? '', props.version.system_prompt)

  return (
    <li class="verevon-agent-builder__version">
      <div class="verevon-agent-builder__version-header">
        <strong>{i18n.tr(`Versjon ${props.version.version}`, `Version ${props.version.version}`)}</strong>
        <time datetime={new Date(props.version.published_at).toISOString()}>
          {new Date(props.version.published_at).toLocaleString(i18n.localeCode())}
        </time>
        <button type="button" aria-expanded={open() ? 'true' : 'false'} onClick={() => setOpen(!open())}>
          {open() ? i18n.tr('Skjul endringer', 'Hide changes') : i18n.tr('Vis endringer', 'Show changes')}
        </button>
      </div>
      <Show when={open()}>
        <pre class="verevon-agent-builder__diff" aria-label={i18n.tr('Endringer i instruksjonene', 'Changes to the instructions')}>
          <For each={diff()}>
            {(line) => (
              <span class={`verevon-agent-builder__diff-line verevon-agent-builder__diff-line--${line.kind}`}>
                {line.kind === 'added' ? '+ ' : line.kind === 'removed' ? '- ' : '  '}
                {line.text}
                {'\n'}
              </span>
            )}
          </For>
        </pre>
      </Show>
    </li>
  )
}
