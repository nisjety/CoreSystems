import { describe, expect, it } from 'vitest'
import { DEMO_UNAVAILABLE_BODY, classifyRequest, classifyTransport, internalPath, isApiPath, isUnofferedPath } from './guard-policy'

// The demo document lives at the same place as the production bundle, under /product-demo/.
const DOCUMENT = 'https://verevon.example/product-demo/index.html'
const BASE = '/product-demo/'

function get(url: string) {
  return classifyRequest({ url, method: 'GET' }, DOCUMENT, BASE)
}

describe('classifyRequest', () => {
  it('lets the demo read its own static files', () => {
    expect(get('/product-demo/assets/main-abc123.js')).toEqual({ action: 'allow' })
    expect(get('/product-demo/fonts/Geist.woff2')).toEqual({ action: 'allow' })
  })

  it('never sends product API calls to the network', () => {
    expect(get('/api/v1/chat/threads')).toEqual({ action: 'unhandled-api', path: '/api/v1/chat/threads' })
    expect(classifyRequest({ url: '/api/v1/navbar', method: 'POST' }, DOCUMENT, BASE)).toEqual({
      action: 'unhandled-api',
      path: '/api/v1/navbar',
    })
    expect(get('/health')).toEqual({ action: 'unhandled-api', path: '/health' })
  })

  it('blocks every other origin, including the real site', () => {
    expect(get('https://fonts.googleapis.com/css2?family=Geist')).toEqual({
      action: 'block',
      reason: 'third-party origin https://fonts.googleapis.com',
    })
    expect(get('https://verevon.example.net/product-demo/assets/x.js').action).toBe('block')
    expect(get('http://verevon.example/product-demo/index.html').action).toBe('block')
  })

  it('blocks same-origin requests outside the demo folder', () => {
    expect(get('/pricing')).toEqual({ action: 'block', reason: 'outside the demo /pricing' })
  })

  it('blocks writes, even to the demo folder', () => {
    expect(classifyRequest({ url: '/product-demo/x', method: 'post' }, DOCUMENT, BASE)).toEqual({
      action: 'block',
      reason: 'POST /product-demo/x',
    })
  })

  it('allows in-memory data and blob URLs', () => {
    expect(get('data:text/plain,hei').action).toBe('allow')
    expect(get('blob:https://verevon.example/1234').action).toBe('allow')
  })

  it('blocks invalid URLs', () => {
    expect(get('http://[::bad').action).toBe('block')
  })
})

describe('classifyTransport', () => {
  it('blocks sockets and beacons and names only the origin', () => {
    expect(classifyTransport('websocket', 'wss://live.example.net/feed?token=secret', DOCUMENT)).toEqual({
      action: 'block',
      reason: 'websocket to wss://live.example.net',
    })
    expect(classifyTransport('beacon', '/analytics', DOCUMENT)).toEqual({
      action: 'block',
      reason: 'beacon to https://verevon.example',
    })
  })
})

describe('internalPath', () => {
  it('turns same-origin hrefs into demo paths, keeping query and hash', () => {
    expect(internalPath('/chat?thread=abc#top', DOCUMENT)).toBe('/chat?thread=abc#top')
    expect(internalPath('https://verevon.example/dashboard?tab=web', DOCUMENT)).toBe('/dashboard?tab=web')
    expect(internalPath('dashboard', DOCUMENT)).toBe('/product-demo/dashboard')
  })

  it('returns null for anything that leaves the demo', () => {
    expect(internalPath('https://example.org/x', DOCUMENT)).toBeNull()
    expect(internalPath('mailto:ida@fjordform.example', DOCUMENT)).toBeNull()
    expect(internalPath('//elsewhere.example/x', DOCUMENT)).toBeNull()
    expect(internalPath('http://[::bad', DOCUMENT)).toBeNull()
  })
})

describe('isApiPath and the unavailable body', () => {
  it('recognises the product endpoints only', () => {
    expect(isApiPath('/api')).toBe(true)
    expect(isApiPath('/apiary')).toBe(false)
    expect(isApiPath('/product-demo/api.js')).toBe(false)
  })

  it('answers in Norwegian with a stable code', () => {
    expect(DEMO_UNAVAILABLE_BODY.error.code).toBe('demo_unavailable')
    expect(DEMO_UNAVAILABLE_BODY.error.message).toBe('Dette er ikke med i demoen.')
  })
})

describe('isUnofferedPath', () => {
  it('refuses the product utility pages that share the chat thread route', () => {
    expect(isUnofferedPath('/chat/findings')).toBe(true)
    expect(isUnofferedPath('/chat/reports?period=uke')).toBe(true)
    expect(isUnofferedPath('/chat/routines#top')).toBe(true)
  })

  it('offers the dashboard, the chat and ordinary threads', () => {
    expect(isUnofferedPath('/chat')).toBe(false)
    expect(isUnofferedPath('/chat/thr_demo_1')).toBe(false)
    expect(isUnofferedPath('/dashboard?tab=web')).toBe(false)
  })

  it('does not match a thread that only starts with a reserved word', () => {
    expect(isUnofferedPath('/chat/findings-2')).toBe(false)
    expect(isUnofferedPath('/chat/findings/extra')).toBe(false)
  })
})
