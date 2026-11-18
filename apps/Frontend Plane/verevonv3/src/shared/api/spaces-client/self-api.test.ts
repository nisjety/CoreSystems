import { describe, expect, it } from 'vitest'

import { spaceSelfApiSwitchFrom } from './self-api'

const reply = {
  binding_ref: 'bind_1',
  self_api_enabled: true,
  projection_version: 4,
  changed: true,
}

describe('spaceSelfApiSwitchFrom (WS18-S6)', () => {
  it('reads a well-formed reply', () => {
    expect(spaceSelfApiSwitchFrom(reply)).toEqual({
      bindingRef: 'bind_1',
      selfApiEnabled: true,
      projectionVersion: 4,
      changed: true,
    })
  })

  it('returns a null projection version when the reply carries none', () => {
    expect(spaceSelfApiSwitchFrom({ ...reply, projection_version: null }).projectionVersion).toBeNull()
    const { projection_version: _unused, ...withoutVersion } = reply
    expect(spaceSelfApiSwitchFrom(withoutVersion).projectionVersion).toBeNull()
  })

  it('throws for a reply that is not an object', () => {
    for (const value of [null, undefined, 'reply', 7, true, [reply]]) {
      expect(() => spaceSelfApiSwitchFrom(value)).toThrow('invalid_self_api_response')
    }
  })

  it('throws for a blank, missing or non-string binding_ref', () => {
    for (const bindingRef of ['', '   ', 9, undefined]) {
      expect(() => spaceSelfApiSwitchFrom({ ...reply, binding_ref: bindingRef })).toThrow('invalid_self_api_response')
    }
  })

  it('throws for a non-boolean self_api_enabled or changed', () => {
    for (const value of ['true', 1, null, undefined]) {
      expect(() => spaceSelfApiSwitchFrom({ ...reply, self_api_enabled: value })).toThrow('invalid_self_api_response')
      expect(() => spaceSelfApiSwitchFrom({ ...reply, changed: value })).toThrow('invalid_self_api_response')
    }
  })

  it('throws for a non-numeric projection_version', () => {
    for (const value of ['4', Number.NaN, {}, true]) {
      expect(() => spaceSelfApiSwitchFrom({ ...reply, projection_version: value })).toThrow('invalid_self_api_response')
    }
  })
})
