// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  BrowserSiteConsentCard,
  browserSiteConsentChoices,
  browserSiteConsentLabel,
} from '@/features/chat/components/ChatMessages'

afterEach(() => {
  cleanup()
})

describe('browserSiteConsentChoices', () => {
  it('offers only what the admin ceiling allows for the host', () => {
    expect(browserSiteConsentChoices('read', false)).toEqual(['allow_this_run', 'remember_read', 'deny'])
    expect(browserSiteConsentChoices('act', false)).toEqual([
      'allow_this_run',
      'remember_read',
      'remember_act',
      'deny',
    ])
  })

  it('never offers a remembered choice under zero data retention', () => {
    expect(browserSiteConsentChoices('act', true)).toEqual([])
    expect(browserSiteConsentChoices('read', true)).toEqual([])
  })
})

describe('browserSiteConsentLabel', () => {
  it('words every choice in Norwegian and in English', () => {
    const nb = (noText: string) => noText
    const en = (_noText: string, enText: string) => enText
    expect(browserSiteConsentLabel('remember_act', { tr: nb })).toBe('Husk: lesing og handling')
    expect(browserSiteConsentLabel('remember_act', { tr: en })).toBe('Remember: reading and acting')
    expect(browserSiteConsentLabel('deny', { tr: en })).toBe('Do not allow')
  })
})

describe('BrowserSiteConsentCard', () => {
  it('names the host and reports the choice the member makes', () => {
    const onDecide = vi.fn()
    render(() => (
      <BrowserSiteConsentCard host="shop.example.com" maxLevel="read" zdr={false} onDecide={onDecide} />
    ))
    expect(screen.getByText(/shop\.example\.com/)).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Husk: bare lesing' }))
    flush()
    expect(onDecide).toHaveBeenCalledWith('remember_read')
  })

  it('does not offer a remembered act when the ceiling is read', () => {
    render(() => (
      <BrowserSiteConsentCard host="shop.example.com" maxLevel="read" zdr={false} onDecide={vi.fn()} />
    ))
    expect(screen.queryByRole('button', { name: 'Husk: lesing og handling' })).toBeNull()
  })

  it('renders nothing under zero data retention', () => {
    render(() => (
      <BrowserSiteConsentCard host="shop.example.com" maxLevel="act" zdr={true} onDecide={vi.fn()} />
    ))
    expect(screen.queryByRole('group', { name: 'Nettside for agenten' })).toBeNull()
  })
})
