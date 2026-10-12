import { describe, expect, it } from 'vitest'
import { demoAllows } from './allowlist'

describe('demoAllows', () => {
  it('allows the dashboard, the chat and one chat thread, as same-origin paths', () => {
    expect(demoAllows('/dashboard')).toBe(true)
    expect(demoAllows('/chat')).toBe(true)
    expect(demoAllows('/chat/thr_tilbud_solhaug')).toBe(true)
    expect(demoAllows('/chat/thr_reklamasjon_bordplate#siste')).toBe(true)
  })

  it('allows the two home tabs as the only query on the dashboard', () => {
    expect(demoAllows('/dashboard?tab=web')).toBe(true)
    expect(demoAllows('/dashboard?tab=extract')).toBe(true)
    expect(demoAllows('/dashboard?overview=spaces')).toBe(false)
    expect(demoAllows('/dashboard?tab=web&overview=spaces')).toBe(false)
    expect(demoAllows('/dashboard?tab=chat')).toBe(false)
    expect(demoAllows('/dashboard?tab=')).toBe(false)
    expect(demoAllows('/chat?thread=thr_tilbud_solhaug')).toBe(false)
  })

  it('refuses every other page of the product', () => {
    for (const href of [
      '/',
      '/knowledge',
      '/settings',
      '/settings/workspace',
      '/agents',
      '/ingestions',
      '/login',
      '/chat/',
      '/chat/a/b',
      '/dashboard/',
      '/Dashboard',
      '/chatt',
    ]) {
      expect(demoAllows(href), href).toBe(false)
    }
  })

  it('refuses absolute URLs, other origins and every non-page scheme', () => {
    for (const href of [
      'https://verevon.com/dashboard',
      'http://localhost:5173/chat',
      'https://naeringsnytt.example/artikler/lager',
      '//evil.example/dashboard',
      '/\\evil.example/dashboard',
      '\\\\evil.example\\share',
      'mailto:ida.berg@fjordform.example',
      'tel:+4790000000',
      'javascript:alert(1)',
      'data:text/html,<p>hei</p>',
      'blob:https://verevon.com/abc',
      'ftp://fjordform.example/x',
    ]) {
      expect(demoAllows(href), href).toBe(false)
    }
  })

  it('refuses paths that only reach an allowed page after rewriting', () => {
    for (const href of ['/chat/%2e%2e/dashboard', '/chat/../dashboard', '/chat/./thr_tilbud_solhaug', '/dashboard%3Ftab=web']) {
      expect(demoAllows(href), href).toBe(false)
    }
  })

  it('refuses empty, spaced, relative and fragment-only hrefs and control characters', () => {
    for (const href of ['', ' /dashboard', '/dashboard ', '/da\tshboard', '/dashboard\n', 'dashboard', 'chat', '#main', '#']) {
      expect(demoAllows(href), JSON.stringify(href)).toBe(false)
    }
  })

  it('refuses a value that is not a string', () => {
    expect(demoAllows(undefined as unknown as string)).toBe(false)
    expect(demoAllows(null as unknown as string)).toBe(false)
  })
})
