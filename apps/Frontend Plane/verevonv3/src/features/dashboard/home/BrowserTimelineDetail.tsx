import { For, Show, createSignal } from 'solid-js'
import { AlertCircle, ExternalLink, FileJson, Image as ImageIcon, Loader2, Network, Sparkles, Terminal, X } from '@/shared/icons'
import type { JSX } from '@solidjs/web'
import { createResource } from '@/shared/lib/create-resource-compat'
import {
  artifactsForTimelineEntry,
  browserArtifactUrl,
  describeTimelineDelta,
  type BrowserArtifactDescriptor,
  type BrowserStepRationale,
  type BrowserTimelineDetail,
} from './browser-session'

/** Console entry tone for row styling; shared with the inspector panels. */
export function consoleTone(level: string): 'error' | 'warn' | 'info' {
  const normalized = level.toLowerCase()
  if (normalized.includes('error')) return 'error'
  if (normalized.includes('warn')) return 'warn'
  return 'info'
}

/** Network status tone for row styling; shared with the inspector panels. */
export function networkTone(status: number): 'error' | 'warn' | 'redirect' | 'ok' {
  if (status >= 500) return 'error'
  if (status >= 400) return 'warn'
  if (status >= 300) return 'redirect'
  return 'ok'
}

export function compactBrowserUrl(value: string): string {
  try {
    const url = new URL(value)
    const path = `${url.pathname}${url.search}`.replace(/\/$/, '')
    return `${url.hostname}${path || '/'}`
  } catch {
    return value
  }
}

const NESTED_ARTIFACT_ID_RE = /^art_[A-Za-z0-9_-]{4,}$/
const MAX_JSON_CHILDREN = 50
const MAX_JSON_DEPTH = 6

async function fetchArtifactJson(url: string): Promise<unknown> {
  const response = await fetch(url, { credentials: 'include' })
  if (!response.ok) {
    throw new Error(`Artefakten kunne ikke hentes (${response.status}).`)
  }
  return response.json() as Promise<unknown>
}

function isPlainObject(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

/**
 * Collapsible, structured rendering of a JSON artifact. Values are rendered
 * verbatim (bounded per level for very large payloads); string values that are
 * artifact ids are linkified to the gateway artifact route so nested evidence
 * (e.g. annotated screenshots referenced by visual_observation.json) stays one
 * click away. Implemented as a plain recursive render function — the fetched
 * JSON is a static snapshot, so no reactivity is needed per node.
 */
function renderJsonNode(depth: number, label: string, value: unknown, sessionId?: string): JSX.Element {
  if (isPlainObject(value) || Array.isArray(value)) {
    const entries = Array.isArray(value)
      ? value.map((child, index) => [String(index), child] as const)
      : Object.entries(value)
    const totalCount = entries.length
    if (depth >= MAX_JSON_DEPTH) {
      return (
        <p class="knowledge-browser-json__leaf">
          <span>{label}</span>
          <code>{Array.isArray(value) ? `[${totalCount} elementer]` : `{${totalCount} felter}`}</code>
        </p>
      )
    }
    return (
      <details class="knowledge-browser-json__node" open={depth === 0}>
        <summary>
          <span>{label}</span>
          <code>{Array.isArray(value) ? `[${totalCount}]` : `{${totalCount}}`}</code>
        </summary>
        <div class="knowledge-browser-json__children">
          <For each={entries.slice(0, MAX_JSON_CHILDREN)}>
            {([key, child]) => renderJsonNode(depth + 1, key, child, sessionId)}
          </For>
          <Show when={totalCount > MAX_JSON_CHILDREN}>
            <p class="knowledge-browser-json__truncated">+{totalCount - MAX_JSON_CHILDREN} flere felter</p>
          </Show>
        </div>
      </details>
    )
  }

  const artifactLink =
    typeof value === 'string' && NESTED_ARTIFACT_ID_RE.test(value)
      ? browserArtifactUrl(sessionId, value)
      : null

  return (
    <p class="knowledge-browser-json__leaf">
      <span>{label}</span>
      <Show
        when={artifactLink}
        fallback={<code>{value === null ? 'null' : typeof value === 'string' ? value : String(value)}</code>}
      >
        {(url) => (
          <a href={url()} target="_blank" rel="noopener noreferrer">
            {String(value)} <ExternalLink class="size-3" aria-hidden="true" />
          </a>
        )}
      </Show>
    </p>
  )
}

function ArtifactCard(props: { artifact: BrowserArtifactDescriptor; sessionId?: string }) {
  const [expanded, setExpanded] = createSignal(false)
  const [jsonSource] = createResource(
    () => (props.artifact.mediaKind === 'json' && expanded() ? props.artifact.url : null),
    fetchArtifactJson,
  )
  const [broken, setBroken] = createSignal(false)

  return (
    <div class="knowledge-browser-artifact" data-media-kind={props.artifact.mediaKind}>
      <header>
        <span>
          <Show when={props.artifact.mediaKind === 'image'} fallback={<FileJson class="size-3.5" aria-hidden="true" />}>
            <ImageIcon class="size-3.5" aria-hidden="true" />
          </Show>
          {props.artifact.label}
        </span>
        <a
          href={props.artifact.url}
          target="_blank"
          rel="noopener noreferrer"
          aria-label={`Åpne ${props.artifact.label} i ny fane`}
        >
          <ExternalLink class="size-3.5" />
        </a>
      </header>
      <Show when={props.artifact.mediaKind === 'image'}>
        <Show
          when={!broken()}
          fallback={<p class="knowledge-browser-artifact__error"><AlertCircle class="size-3.5" /> Bildet kunne ikke lastes.</p>}
        >
          <img
            src={props.artifact.url}
            alt={props.artifact.label}
            loading="lazy"
            decoding="async"
            onError={() => setBroken(true)}
          />
        </Show>
      </Show>
      <Show when={props.artifact.mediaKind === 'json'}>
        <Show
          when={expanded()}
          fallback={
            <button type="button" class="knowledge-browser-artifact__expand" onClick={() => setExpanded(true)}>
              Vis strukturert innhold
            </button>
          }
        >
          <Show
            when={!jsonSource.loading}
            fallback={<p class="knowledge-browser-artifact__loading"><Loader2 class="size-3.5 dashboard-xsearch-spin" /> Henter artefakt ...</p>}
          >
            <Show
              when={!jsonSource.error}
              fallback={
                <p class="knowledge-browser-artifact__error">
                  <AlertCircle class="size-3.5" /> {jsonSource.error instanceof Error ? jsonSource.error.message : 'Artefakten kunne ikke hentes.'}
                </p>
              }
            >
              <div class="knowledge-browser-json">
                {renderJsonNode(0, props.artifact.label, jsonSource(), props.sessionId)}
              </div>
            </Show>
          </Show>
        </Show>
      </Show>
    </div>
  )
}

/**
 * Deterministic, artifact-backed detail view for one browser timeline step:
 * before/after screenshots, computed observation deltas, console/network/policy
 * evidence, the model rationale for AI-suggested steps, and typed artifact
 * previews. Renders only what the gateway returned — nothing is synthesized.
 */
export function BrowserTimelineDetailPanel(props: {
  detail: BrowserTimelineDetail
  onClose: () => void
  rationale: BrowserStepRationale | null
  sessionId?: string
}) {
  const entry = () => props.detail.entry
  const previous = () => props.detail.previous
  const delta = () => describeTimelineDelta(props.detail)
  const artifacts = () => artifactsForTimelineEntry(entry())
  const consoleEntries = () => entry().consoleSummary ?? []
  const networkEntries = () => entry().networkSummary ?? []
  const policyDenials = () => entry().policyDenials ?? []

  return (
    <section class="knowledge-browser-detail" aria-label={`Detaljer for nettlesersteg ${entry().step}`}>
      <header class="knowledge-browser-detail__head">
        <span class="knowledge-browser-detail__step">#{entry().step}</span>
        <div class="knowledge-browser-detail__title">
          <strong>{entry().title || compactBrowserUrl(entry().url ?? '')}</strong>
          <Show when={entry().url}>
            {(url) => <p title={url()}>{compactBrowserUrl(url())}</p>}
          </Show>
        </div>
        <Show when={entry().observedAt}>
          {(observedAt) => <time datetime={observedAt()}>{observedAt().replace('T', ' ').slice(0, 19)}</time>}
        </Show>
        <button type="button" aria-label="Lukk stegdetaljer" onClick={() => props.onClose()}>
          <X class="size-3.5" />
        </button>
      </header>

      <div class="knowledge-browser-detail__compare">
        <figure class={{ 'knowledge-browser-detail__shot--missing': !previous()?.screenshotUrl }}>
          <figcaption>Før {previous() ? `· #${previous()?.step}` : ''}</figcaption>
          <Show
            when={previous()?.screenshotUrl}
            fallback={<p>{previous() ? 'Ingen skjermbilde-artefakt for forrige steg.' : 'Første steg — ingen tidligere observasjon.'}</p>}
          >
            {(url) => <img src={url()} alt={`Skjermbilde før steg ${entry().step}`} loading="lazy" decoding="async" />}
          </Show>
        </figure>
        <figure class={{ 'knowledge-browser-detail__shot--missing': !entry().screenshotUrl }}>
          <figcaption>Etter · #{entry().step}</figcaption>
          <Show
            when={entry().screenshotUrl}
            fallback={<p>Ingen skjermbilde-artefakt for dette steget.</p>}
          >
            {(url) => <img src={url()} alt={`Skjermbilde etter steg ${entry().step}`} loading="lazy" decoding="async" />}
          </Show>
        </figure>
      </div>

      <div class="knowledge-browser-detail__delta" aria-label="Beregnet endring mellom observasjonene">
        <span class="knowledge-browser-detail__delta-label">Beregnet fra observasjonene:</span>
        <Show when={delta().domNodeDelta !== null} fallback={<span>DOM-delta utilgjengelig</span>}>
          <span>{(delta().domNodeDelta ?? 0) >= 0 ? '+' : ''}{delta().domNodeDelta} DOM-noder</span>
        </Show>
        <Show when={delta().urlChanged}>
          <span>URL endret</span>
        </Show>
        <Show when={delta().titleChanged}>
          <span>Tittel endret</span>
        </Show>
        <Show when={typeof entry().domNodeCount === 'number'}>
          <span>{entry().domNodeCount} noder totalt</span>
        </Show>
      </div>

      <Show when={props.rationale}>
        {(rationale) => (
          <div class="knowledge-browser-detail__rationale">
            <header>
              <Sparkles class="size-3.5" aria-hidden="true" />
              <span>AI-steg · {rationale().actionType ?? 'ingen handling'}</span>
              <Show when={rationale().confidence !== null}>
                <strong>{Math.round((rationale().confidence ?? 0) * 100)} %</strong>
              </Show>
              <Show when={rationale().modelUsed}>
                <code>{rationale().modelUsed}</code>
              </Show>
            </header>
            <Show when={rationale().reason}>
              <p>{rationale().reason}</p>
            </Show>
            <Show when={rationale().goal}>
              <p class="knowledge-browser-detail__goal">Mål: {rationale().goal}</p>
            </Show>
          </div>
        )}
      </Show>

      <Show when={policyDenials().length > 0}>
        <div class="knowledge-browser-detail__section knowledge-browser-detail__section--policy">
          <header>
            <span><AlertCircle class="size-3.5" /> Policy-avslag</span>
            <strong>{policyDenials().length}</strong>
          </header>
          <For each={policyDenials()}>
            {(denial) => <p class="knowledge-browser-policy-denial"><AlertCircle class="size-3.5" /> {denial}</p>}
          </For>
        </div>
      </Show>

      <div class="knowledge-browser-detail__evidence">
        <div class="knowledge-browser-detail__section">
          <header>
            <span><Terminal class="size-3.5" /> Console</span>
            <strong>{consoleEntries().length}</strong>
          </header>
          <For
            each={consoleEntries()}
            fallback={<p class="knowledge-browser-empty">Ingen console-hendelser i dette steget.</p>}
          >
            {(line) => (
              <div class={`knowledge-browser-console-row knowledge-browser-console-row--${consoleTone(line.level)}`}>
                <span>{line.level}</span>
                <p>{line.text}</p>
              </div>
            )}
          </For>
        </div>
        <div class="knowledge-browser-detail__section">
          <header>
            <span><Network class="size-3.5" /> Network</span>
            <strong>{networkEntries().length}</strong>
          </header>
          <For
            each={networkEntries()}
            fallback={<p class="knowledge-browser-empty">Ingen nettverkskall i dette steget.</p>}
          >
            {(call) => (
              <div class={`knowledge-browser-network-row knowledge-browser-network-row--${networkTone(call.status)}`}>
                <span>{call.method}</span>
                <strong>{call.status}</strong>
                <p title={call.url}>{compactBrowserUrl(call.url)}</p>
              </div>
            )}
          </For>
        </div>
      </div>

      <div class="knowledge-browser-detail__artifacts">
        <For
          each={artifacts()}
          fallback={<p class="knowledge-browser-empty">Ingen artefakter returnert for dette steget.</p>}
        >
          {(artifact) => <ArtifactCard artifact={artifact} sessionId={props.sessionId} />}
        </For>
      </div>
    </section>
  )
}
