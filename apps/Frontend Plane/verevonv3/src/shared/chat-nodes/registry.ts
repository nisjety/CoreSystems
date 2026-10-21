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

import type { JSX } from '@solidjs/web'
import { createMemo, getOwner, runWithOwner, untrack } from 'solid-js'
import type {
  ConversationNode,
  ConversationNodeContext,
  ConversationNodeDefinition,
  ConversationNodeEntry,
  ConversationNodeKind,
} from './types'

export type ConversationNodeRegistry = {
  /** Render one node, or `null` when its kind has no renderer. */
  render: (node: ConversationNode, ctx: ConversationNodeContext) => JSX.Element | null
  /**
   * Render one ROW of a keyed node list: the row's entry is read through
   * `entry()`, so the rendered node updates in place when the list re-derives
   * instead of being rebuilt. Renderers are the same functions `render` takes;
   * they receive views whose property reads follow the current entry, so a prop
   * they pass on (`answer={node.answer}`) stays reactive.
   */
  renderLive: (entry: () => ConversationNodeEntry) => JSX.Element | null
  /** Kinds this registry can render — for tests and diagnostics. */
  kinds: () => ConversationNodeKind[]
}

/**
 * A stand-in for `read()` whose every property read goes to the CURRENT value.
 *
 * It exists so the existing renderers, which take a plain node and context, can
 * serve a row that outlives the object it was created from. Each property is its
 * own memo, so a reader of `ctx.citations` is told only when the citations change,
 * not every time the row's entry is replaced, which is every token. Without that a
 * component that reads one stable field would rebuild on each re-derivation, which
 * is the remount `renderLive` is there to end. A read made inside a tracking scope
 * (a JSX prop getter, a memo) subscribes to that property, and one made later, in
 * an event handler, sees the latest value.
 */
function liveView<T extends object>(read: () => T): T {
  const owner = getOwner()
  const fields = new Map<PropertyKey, () => unknown>()
  const field = (key: PropertyKey): (() => unknown) => {
    let accessor = fields.get(key)
    if (!accessor) {
      const compute = () => Reflect.get(read(), key)
      // The memo belongs to the row, so it ends with the row. Outside any owner
      // (a renderer called by hand) there is nothing to end it with: read directly.
      accessor = (owner && runWithOwner(owner, () => createMemo(compute))) || compute
      fields.set(key, accessor)
    }
    return accessor
  }
  return new Proxy({} as T, {
    get: (_target, key) => field(key)(),
    has: (_target, key) => Reflect.has(read(), key),
    ownKeys: () => Reflect.ownKeys(read()),
    // The target is an empty extensible object, so a key it does not own may only
    // be reported as configurable.
    getOwnPropertyDescriptor: (_target, key) => {
      const descriptor = Reflect.getOwnPropertyDescriptor(read(), key)
      return descriptor && { ...descriptor, configurable: true }
    },
  })
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
  // `kind` is passed apart from the node so `renderLive` can read it without
  // subscribing: a row's kind is its key, so it never changes under the row.
  const dispatch = (kind: ConversationNodeKind, node: ConversationNode, ctx: ConversationNodeContext) => {
    const definition = definitions[kind]
    if (!definition) {
      // Once per kind: a render loop would otherwise flood the console.
      if (!warned.has(kind)) {
        warned.add(kind)
        console.warn(
          `[chat-nodes] no renderer registered for node kind "${kind}" — ` +
            'the node was derived and then dropped. Add it to the registry.',
        )
      }
      return null
    }
    // The cast is confined to this one line: `definitions` is keyed by the
    // same discriminant the node carries, so the pairing is correct by
    // construction, but TypeScript cannot narrow both sides together.
    return (definition as ConversationNodeDefinition).render(node, ctx)
  }
  return {
    render: (node, ctx) => dispatch(node.kind, node, ctx),
    renderLive: (entry) =>
      dispatch(
        untrack(() => entry().node.kind),
        liveView(() => entry().node),
        liveView(() => entry().context),
      ),
    kinds: () => Object.keys(definitions) as ConversationNodeKind[],
  }
}
