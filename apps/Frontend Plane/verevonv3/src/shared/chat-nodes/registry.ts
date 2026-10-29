/**
 * Keyed renderer registry for [`ConversationNode`]s.
 *
 * The registry lives in `shared` and is POPULATED by the feature layer, because
 * the renderers are feature components. That dependency direction is the point:
 * a new surface (a Space transcript, an export view, the context inspector in
 * plan item 3.5) can register its own renderers against the same node kinds
 * without the derivation or any other surface knowing about it.
 *
 * # Unknown kinds are logged, never dropped
 *
 * The same rule as the SSE client's `onUnknownEvent`. A node with no registered
 * renderer means someone added a node kind and forgot a renderer — and silently
 * rendering nothing makes that indistinguishable from a turn that simply had no
 * such content. `createConversationNodeRegistry` is exhaustive at the type level,
 * so this is a runtime backstop for a partial registry, not the normal path.
 */

import type { JSX } from 'solid-js'
import type {
  ConversationNode,
  ConversationNodeContext,
  ConversationNodeDefinition,
  ConversationNodeKind,
} from './types'

export type ConversationNodeRegistry = {
  /** Render one node, or `null` when its kind has no renderer. */
  render: (node: ConversationNode, ctx: ConversationNodeContext) => JSX.Element | null
  /** Kinds this registry can render — for tests and diagnostics. */
  kinds: () => ConversationNodeKind[]
}

/**
 * Build a registry from an exhaustive map of kind → renderer.
 *
 * Typed as a full record so TypeScript fails the build when a node kind is added
 * without a renderer. That check is the reason to prefer this over a loose array
 * of definitions: the runtime warning below should be unreachable.
 */
export function createConversationNodeRegistry(definitions: {
  [K in ConversationNodeKind]: ConversationNodeDefinition<K>
}): ConversationNodeRegistry {
  const warned = new Set<string>()
  return {
    render: (node, ctx) => {
      const definition = definitions[node.kind]
      if (!definition) {
        // Once per kind: a render loop would otherwise flood the console.
        if (!warned.has(node.kind)) {
          warned.add(node.kind)
          console.warn(
            `[chat-nodes] no renderer registered for node kind "${node.kind}" — ` +
              'the node was derived and then dropped. Add it to the registry.',
          )
        }
        return null
      }
      // The cast is confined to this one line: `definitions` is keyed by the
      // same discriminant the node carries, so the pairing is correct by
      // construction, but TypeScript cannot narrow both sides together.
      return (definition as ConversationNodeDefinition).render(node, ctx)
    },
    kinds: () => Object.keys(definitions) as ConversationNodeKind[],
  }
}
