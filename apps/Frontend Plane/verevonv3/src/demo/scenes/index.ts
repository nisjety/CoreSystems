import { scene02Plan } from './02-plan'
import { scene03Undersok } from './03-undersok'
import type { SceneDefinition } from '../scene-types'

/** Every scene the `/scene/:id` route can play. */
export const scenes: Record<string, SceneDefinition> = {
  [scene02Plan.id]: scene02Plan,
  [scene03Undersok.id]: scene03Undersok,
}

export function findScene(id: string): SceneDefinition | undefined {
  return scenes[id]
}
