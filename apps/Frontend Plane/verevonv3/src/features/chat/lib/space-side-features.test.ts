import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  THREAD_REFUSAL_CODES,
  byoGatewaySwitchOn,
  isThreadRefusalCode,
  sideFeatureThreadOf,
  sideFeaturesWithheldOnSpace,
  subscriptionRouteWithheldOnSpace,
} from './space-side-features'

afterEach(() => {
  vi.unstubAllEnvs()
})

// The golden list of the typed thread refusals (DEC-V-R8), each with the status the gateway relays it under and the status
// Model Gateway sends it with. The SPA list, the gateway's `thread_refusal_passes` (apps/gateway/src/domains/ai.rs) and Model
// Gateway's `thread_connection.rs` are read here, so a code added, renamed or moved on one side fails in this file.
const GOLDEN_THREAD_REFUSALS: Readonly<Record<string, string>> = {
  connection_ambiguous: 'CONFLICT',
  scope_not_available_in_space: 'CONFLICT',
  side_chat_model_required: 'BAD_REQUEST',
  side_chat_thread_retention_refused: 'CONFLICT',
}

/** A file read with its line endings normalised to LF, from the SPA root (the working directory of the test run). */
function sourceText(relative: string): string {
  return readFileSync(resolve(process.cwd(), relative), 'utf8').replace(/\r\n/g, '\n')
}

/** The gateway's refusal table: each code `thread_refusal_passes` relays, under the status arm that lists it. */
function gatewayThreadRefusals(): Record<string, string> {
  const body = /fn thread_refusal_passes\(status: StatusCode, code: &str\) -> bool \{\n([\s\S]*?)\n\}\n/.exec(
    sourceText('apps/gateway/src/domains/ai.rs'),
  )?.[1] ?? ''
  const refusals: Record<string, string> = {}
  for (const arm of body.matchAll(/StatusCode::([A-Z_]+) => matches!\(\s*code,([^)]*)\)/g)) {
    for (const code of (arm[2] ?? '').matchAll(/"([a-z_]+)"/g)) refusals[code[1] ?? ''] = arm[1] ?? ''
  }
  for (const arm of body.matchAll(/StatusCode::([A-Z_]+) => code == "([a-z_]+)"/g)) {
    refusals[arm[2] ?? ''] = arm[1] ?? ''
  }
  return refusals
}

/** Every status Model Gateway's thread connection resolver sends with a code, as `typed(StatusCode::X, "code", ...)`. */
function modelGatewayThreadStatuses(): Record<string, string[]> {
  const sent: Record<string, string[]> = {}
  const text = sourceText('../../Model Plane/rust/services/model-gateway/src/thread_connection.rs')
  for (const match of text.matchAll(/StatusCode::([A-Z_]+),\s*"([a-z_]+)"/g)) {
    const code = match[2] ?? ''
    sent[code] = [...(sent[code] ?? []), match[1] ?? '']
  }
  return sent
}

describe('the typed thread refusal codes stay in step across the SPA, the gateway and Model Gateway (golden)', () => {
  it('the SPA list is the golden list of codes', () => {
    expect([...THREAD_REFUSAL_CODES].sort()).toEqual(Object.keys(GOLDEN_THREAD_REFUSALS).sort())
  })

  it('the gateway relays exactly the golden codes, each under its golden status', () => {
    expect(gatewayThreadRefusals()).toEqual(GOLDEN_THREAD_REFUSALS)
  })

  it('Model Gateway sends each golden code, and only under its golden status', () => {
    const sent = modelGatewayThreadStatuses()
    for (const [code, status] of Object.entries(GOLDEN_THREAD_REFUSALS)) {
      expect([...new Set(sent[code] ?? [])], code).toEqual([status])
    }
  })
})

describe('isThreadRefusalCode', () => {
  it('recognises the typed refusals of a named thread, and no other code', () => {
    for (const code of [
      'scope_not_available_in_space',
      'side_chat_thread_retention_refused',
      'connection_ambiguous',
      'side_chat_model_required',
    ]) {
      expect(isThreadRefusalCode(code), code).toBe(true)
    }
    expect(isThreadRefusalCode('review_unavailable')).toBe(false)
    expect(isThreadRefusalCode('delegated_auth_unavailable')).toBe(false)
    expect(isThreadRefusalCode(null)).toBe(false)
    expect(isThreadRefusalCode(undefined)).toBe(false)
  })
})

describe('sideFeatureThreadOf', () => {
  it('names the open thread, a Space thread included, so Model Gateway decides it', () => {
    // A Space thread is named whatever the switch says: the gateway refuses it typed, and nothing falls to the platform.
    vi.stubEnv('VITE_BYO_GATEWAY', 'true')
    expect(sideFeatureThreadOf('thread-space-1', false)).toBe('thread-space-1')
    vi.stubEnv('VITE_BYO_GATEWAY', '')
    expect(sideFeatureThreadOf('thread-space-1', false)).toBe('thread-space-1')
  })

  it('names no thread for a temporary chat or for a page with no thread', () => {
    expect(sideFeatureThreadOf('thread-1', true)).toBeUndefined()
    expect(sideFeatureThreadOf('', false)).toBeUndefined()
    expect(sideFeatureThreadOf(null, false)).toBeUndefined()
    expect(sideFeatureThreadOf(undefined, false)).toBeUndefined()
  })
})

describe('sideFeaturesWithheldOnSpace', () => {
  it('withholds the side features on a Space thread while the BYO gateway switch is the literal true', () => {
    vi.stubEnv('VITE_BYO_GATEWAY', 'true')
    expect(byoGatewaySwitchOn()).toBe(true)
    expect(sideFeaturesWithheldOnSpace(true)).toBe(true)
  })

  it('never withholds them on a thread that is not a Space thread', () => {
    vi.stubEnv('VITE_BYO_GATEWAY', 'true')
    expect(sideFeaturesWithheldOnSpace(false)).toBe(false)
    expect(sideFeaturesWithheldOnSpace(undefined)).toBe(false)
  })

  it('changes nothing while the switch is off, on a Space thread or not', () => {
    vi.stubEnv('VITE_BYO_GATEWAY', '')
    expect(byoGatewaySwitchOn()).toBe(false)
    expect(sideFeaturesWithheldOnSpace(true)).toBe(false)
    expect(sideFeaturesWithheldOnSpace(false)).toBe(false)
  })

  it('reads the switch as the strict literal: every other spelling is off', () => {
    for (const value of ['True', 'TRUE', '1', 'yes', ' true']) {
      vi.stubEnv('VITE_BYO_GATEWAY', value)
      expect(sideFeaturesWithheldOnSpace(true), value).toBe(false)
    }
  })
})

describe('subscriptionRouteWithheldOnSpace', () => {
  it('withholds the ChatGPT subscription route on a Space thread while the BYO gateway switch is the literal true', () => {
    vi.stubEnv('VITE_BYO_GATEWAY', 'true')
    expect(subscriptionRouteWithheldOnSpace(true)).toBe(true)
  })

  it('never withholds it on a thread that is not a Space thread', () => {
    vi.stubEnv('VITE_BYO_GATEWAY', 'true')
    expect(subscriptionRouteWithheldOnSpace(false)).toBe(false)
    expect(subscriptionRouteWithheldOnSpace(undefined)).toBe(false)
  })

  it('changes nothing while the switch is off, on a Space thread or not', () => {
    vi.stubEnv('VITE_BYO_GATEWAY', '')
    expect(subscriptionRouteWithheldOnSpace(true)).toBe(false)
    expect(subscriptionRouteWithheldOnSpace(false)).toBe(false)
  })

  it('reads the switch as the strict literal: every other spelling is off', () => {
    for (const value of ['True', 'TRUE', '1', 'yes', ' true']) {
      vi.stubEnv('VITE_BYO_GATEWAY', value)
      expect(subscriptionRouteWithheldOnSpace(true), value).toBe(false)
    }
  })
})
