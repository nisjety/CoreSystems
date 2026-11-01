import { describe, expect, it } from 'vitest'

import {
  buildFailureDetails,
  classifyClientError,
  clientBuildId,
  routeClassOf,
} from '@/shared/ui/resilience/classify-client-error'

function apiError(status: number, code: string | null = null): Error {
  const error = new Error('request failed')
  error.name = 'ApiError'
  Object.assign(error, { status, code })
  return error
}

describe('classifyClientError', () => {
  it.each([
    'Failed to fetch dynamically imported module: https://app.example.com/assets/Page-abc.js',
    'error loading dynamically imported module: /assets/x.js',
    'Importing a module script failed.',
    'Unable to preload CSS for /assets/x.css',
  ])('classifies %j as stale_deploy', (message) => {
    expect(classifyClientError(new TypeError(message))).toBe('stale_deploy')
    expect(classifyClientError(new Error(message))).toBe('stale_deploy')
  })

  it('classifies a string rejection that looks like a stale import', () => {
    expect(classifyClientError('Failed to fetch dynamically imported module')).toBe('stale_deploy')
  })

  it('classifies a 401 or an unauthenticated code as session_expired', () => {
    expect(classifyClientError(apiError(401))).toBe('session_expired')
    expect(classifyClientError(apiError(403, 'unauthenticated'))).toBe('session_expired')
  })

  it.each([502, 503, 504])('classifies an ApiError %d as gateway_unreachable', (status) => {
    expect(classifyClientError(apiError(status))).toBe('gateway_unreachable')
  })

  it('classifies a bare network TypeError as gateway_unreachable', () => {
    expect(classifyClientError(new TypeError('Failed to fetch'))).toBe('gateway_unreachable')
    expect(classifyClientError(new TypeError('NetworkError when attempting to fetch resource.'))).toBe('gateway_unreachable')
    expect(classifyClientError(new TypeError('Load failed'))).toBe('gateway_unreachable')
  })

  it('classifies other ApiErrors and plain render failures as render_error', () => {
    expect(classifyClientError(apiError(500))).toBe('render_error')
    expect(classifyClientError(apiError(404, 'not_found'))).toBe('render_error')
    expect(classifyClientError(new TypeError("Cannot read properties of undefined (reading 'x')"))).toBe('render_error')
    expect(classifyClientError(new Error('boom'))).toBe('render_error')
  })

  it('never throws on odd inputs and falls back to render_error', () => {
    for (const input of [undefined, null, 42, {}, [], { message: 7 }, Symbol.iterator]) {
      expect(classifyClientError(input)).toBe('render_error')
    }
  })
})

describe('routeClassOf', () => {
  it('returns the first path segment', () => {
    expect(routeClassOf('/spaces/abc123/room')).toBe('spaces')
    expect(routeClassOf('/settings')).toBe('settings')
    expect(routeClassOf('/Chat/')).toBe('chat')
  })

  it('returns root for the empty path and other for an id-shaped first segment', () => {
    expect(routeClassOf('/')).toBe('root')
    expect(routeClassOf('')).toBe('root')
    expect(routeClassOf('/0f8fad5b-d9cb-469f-a165-70867728950e/x')).toBe('other')
    expect(routeClassOf('/a.b/c')).toBe('other')
  })
})

describe('buildFailureDetails', () => {
  it('carries exactly class, build_id, region, at and route_class', () => {
    const details = buildFailureDetails({
      errorClass: 'render_error',
      region: 'outlet',
      pathname: '/spaces/secret-id/room',
      now: new Date('2026-10-27T10:00:00.000Z'),
    })
    expect(Object.keys(details).sort()).toEqual(['at', 'build_id', 'class', 'region', 'route_class'])
    expect(details).toEqual({
      class: 'render_error',
      build_id: clientBuildId(),
      region: 'outlet',
      at: '2026-10-27T10:00:00.000Z',
      route_class: 'spaces',
    })
    expect(JSON.stringify(details)).not.toContain('secret-id')
  })
})
