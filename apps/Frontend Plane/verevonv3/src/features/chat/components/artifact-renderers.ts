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
import type { Component } from 'solid-js'
import type { ChatArtifact } from './chat-types'

export type ArtifactRenderer<Id extends string = string> = {
  id: Id
  /** Higher wins. Matchers are expected to be exclusive; priority only settles overlaps. */
  priority: number
  match: (artifact: ChatArtifact) => boolean
  /** Body for an extra viewer. Built-in kinds are drawn by the panel's own switch. */
  component?: Component<{ artifact: ChatArtifact }>
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
