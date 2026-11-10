/**
 * Artifact renderer registry.
 *
 * A renderer claims an artifact through `match`; when several claim it, the
 * highest `priority` wins and a tie goes to the one registered first. Pure and
 * JSX-free: nothing here touches the DOM or the i18n context, so the rules stay
 * unit-testable without a component root.
 *
 * Two uses share this one mechanism:
 *   - the built-in kinds, whose table lives beside their matchers in
 *     `chat-artifacts.ts` (the matchers need that module's kind sets, and this
 *     file must stay a leaf so `chat-artifacts.ts` can import it);
 *   - extra viewers registered at runtime, drawn by the panel's Dynamic slot.
 */
import type { JSX } from '@solidjs/web'
import { createContext, type Component } from 'solid-js'
import type { DeckTemplate } from '@/shared/api/chat-client'
import type { ChatArtifact, GeneratedFile } from './chat-types'

/**
 * What an extra viewer is given. `file` is the generated file the artifact came
 * from, when there is one. `fallback` is the panel's own body for the artifact,
 * for a viewer with nothing better to draw.
 */
export type ArtifactRendererProps = {
  artifact: ChatArtifact
  file?: GeneratedFile
  fallback?: () => JSX.Element
}

export type ArtifactRenderer<Id extends string = string> = {
  id: Id
  /** Higher wins. Matchers are expected to be exclusive; priority only settles overlaps. */
  priority: number
  match: (artifact: ChatArtifact) => boolean
  /** Body for an extra viewer. Built-in kinds are drawn by the panel's own switch. */
  component?: Component<ArtifactRendererProps>
}

export type ArtifactRendererRegistry<Id extends string = string> = {
  /** Adds a renderer; the returned function removes it again. */
  register: (renderer: ArtifactRenderer<Id>) => () => void
  resolve: (artifact: ChatArtifact) => ArtifactRenderer<Id> | undefined
}

/**
 * The renderer that claims `artifact`, or `undefined` when none does. Ties on
 * priority keep the earlier entry, so the list order is the tie-breaker.
 */
export function pickArtifactRenderer<Id extends string>(
  renderers: readonly ArtifactRenderer<Id>[],
  artifact: ChatArtifact,
): ArtifactRenderer<Id> | undefined {
  let picked: ArtifactRenderer<Id> | undefined
  for (const renderer of renderers) {
    if (!renderer.match(artifact)) continue
    if (!picked || renderer.priority > picked.priority) picked = renderer
  }
  return picked
}

export function createArtifactRendererRegistry<Id extends string = string>(): ArtifactRendererRegistry<Id> {
  const renderers: ArtifactRenderer<Id>[] = []
  return {
    register(renderer) {
      renderers.push(renderer)
      return () => {
        const index = renderers.indexOf(renderer)
        if (index >= 0) renderers.splice(index, 1)
      }
    },
    resolve: (artifact) => pickArtifactRenderer(renderers, artifact),
  }
}

/** Extra viewers for artifact kinds the built-ins do not draw. Empty unless something registers. */
export const extraArtifactRenderers = createArtifactRendererRegistry()

/**
 * What a deck viewer may do with the brand deck templates (EP-5 S7). The chat page provides it around its artifacts panel,
 * so the viewer gets it without widening `ArtifactRendererProps`. Outside that provider the viewer offers no template control.
 */
export type DeckTemplateActions = {
  /** The templates to offer, or `null` while they are off or the thread is temporary (the gateway refuses a template there). */
  list: () => Promise<DeckTemplate[] | null>
  /** Sends a pick as a turn of its own. `null` is Standard, the neutral theme, and carries no template. */
  apply: (template: DeckTemplate | null) => void
  /** True while a run streams. A pick waits for the run to end, so a template is never queued into a run. */
  busy: () => boolean
}

export const DeckTemplateContext = createContext<DeckTemplateActions>()
