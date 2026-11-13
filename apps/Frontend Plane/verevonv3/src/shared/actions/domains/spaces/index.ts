// One import line per Space workstream fragment, then one spread line below.
// Keep both lists sorted. The registry (action-registry.ts) spreads
// `spaceActions` once; a Space slice never edits the registry again.
//
// Ids declared in a fragment are read by the gateway contract test
// (apps/gateway/scripts/action-surface-contract.test.mjs) straight from
// domains/spaces/*.ts, so every fragment id still needs its dispatcher arm in
// handlers.rs (the body of the dispatcher lives in dispatchers_spaces/<ws>.rs).
import { baseSpaceActions } from './base'
import { ws01SpaceActions } from './ws01'
import { ws03SpaceActions } from './ws03'
import { ws04SpaceActions } from './ws04'
import { ws08SpaceActions } from './ws08'
import { ws09SpaceActions } from './ws09'
import { ws10SpaceActions } from './ws10'
import { ws10bSpaceActions } from './ws10b'
import { ws11SpaceActions } from './ws11'
import { ws12SpaceActions } from './ws12'
import { ws14SpaceActions } from './ws14'
import { ws16SpaceActions } from './ws16'
import { ep08SpaceActions } from './ep08'

export const spaceActions = [
  ...baseSpaceActions,
  ...ws01SpaceActions,
  ...ws03SpaceActions,
  ...ws04SpaceActions,
  ...ws08SpaceActions,
  ...ws09SpaceActions,
  ...ws10SpaceActions,
  ...ws10bSpaceActions,
  ...ws11SpaceActions,
  ...ws12SpaceActions,
  ...ws14SpaceActions,
  ...ws16SpaceActions,
  // EP-8 S7 is appended last, not sorted in: action-registry.test.ts pins the order of the first
  // Space entries (base, then ws01, ws03, ws04), and an earlier slot would move them.
  ...ep08SpaceActions,
] as const
