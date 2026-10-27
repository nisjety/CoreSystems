// One import line per Space workstream fragment, then one spread line below.
// Keep both lists sorted. The registry (action-registry.ts) spreads
// `spaceActions` once; a Space slice never edits the registry again.
//
// Ids declared in a fragment are read by the gateway contract test
// (apps/gateway/scripts/action-surface-contract.test.mjs) straight from
// domains/spaces/*.ts, so every fragment id still needs its dispatcher arm in
// handlers.rs (the body of the dispatcher lives in dispatchers_spaces/<ws>.rs).
import { baseSpaceActions } from './base'

export const spaceActions = [
  ...baseSpaceActions,
] as const
