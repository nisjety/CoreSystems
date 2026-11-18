import { describe, expect, it } from 'vitest'

import { spaceCanvasToolsSwitchFrom } from './canvas-tools'

const reply = {
  binding_ref: 'bind_1',
  canvas_tools_enabled: true,
  projection_version: 4,
  changed: true,
}

describe('spaceCanvasToolsSwitchFrom (W3-DEC-CANVAS)', () => {
  it('reads a well-formed reply', () => {
    expect(spaceCanvasToolsSwitchFrom(reply)).toEqual({
      bindingRef: 'bind_1',
      canvasToolsEnabled: true,
      projectionVersion: 4,
      changed: true,
    })
  })

  it('throws for a reply that is not an object', () => {
    for (const value of [null, undefined, 'reply', 7, true, [reply]]) {
      expect(() => spaceCanvasToolsSwitchFrom(value)).toThrow('invalid_canvas_tools_response')
    }
  })

  it('throws for a blank, missing or non-string binding_ref', () => {
    for (const bindingRef of ['', '   ', 9, undefined]) {
      expect(() => spaceCanvasToolsSwitchFrom({ ...reply, binding_ref: bindingRef })).toThrow(
        'invalid_canvas_tools_response',
      )
    }
  })

  it('throws for a non-boolean canvas_tools_enabled or changed', () => {
    for (const value of ['true', 1, null, undefined]) {
      expect(() => spaceCanvasToolsSwitchFrom({ ...reply, canvas_tools_enabled: value })).toThrow(
        'invalid_canvas_tools_response',
      )
      expect(() => spaceCanvasToolsSwitchFrom({ ...reply, changed: value })).toThrow('invalid_canvas_tools_response')
    }
  })

  it('throws for a non-numeric projection_version', () => {
    for (const value of ['4', Number.NaN, {}, true]) {
      expect(() => spaceCanvasToolsSwitchFrom({ ...reply, projection_version: value })).toThrow(
        'invalid_canvas_tools_response',
      )
    }
  })
})
