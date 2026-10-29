import { cleanup, fireEvent, render, screen, within } from '@solidjs/testing-library'
import { createSignal } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { ConnectedServices, type PulseServiceKey } from './ConnectedServices'
import type { MarketingSource } from '@/shared/api/marketing/preparation-client'
vi.mock('@/shared/i18n', () => ({ useI18n: () => ({ tr: (_no: string, en: string) => en }) }))

describe('connected services and extension picker', () => {
  beforeEach(() => {
    Object.defineProperty(HTMLDialogElement.prototype,'showModal',{configurable:true,value:function(this:HTMLDialogElement){this.setAttribute('open','')}})
    Object.defineProperty(HTMLDialogElement.prototype,'close',{configurable:true,value:function(this:HTMLDialogElement){this.removeAttribute('open')}})
  })
  afterEach(() => { cleanup(); vi.restoreAllMocks() })
  const source = (key: MarketingSource['key'], status: MarketingSource['status']): MarketingSource => ({key,status,implemented:true,mode:'oauth',refresh:'snapshot_read'})
  function mount() {
    const [selected,setSelected]=createSignal<PulseServiceKey>('search_console')
    return render(()=><ConnectedServices sources={[source('search_console','connected'),source('ga4','not_connected'),source('clarity','authorization_required')]} loading={false} error={false} canManage selected={selected()} onSelect={setSelected} onRefresh={()=>{}} renderSetup={key=><p>Setup: {key}</p>}/>)
  }
  it('shows connected services and repairable connections, not unconnected providers', () => {
    mount(); const stage=screen.getByRole('group',{name:'Connected data sources'})
    expect(within(stage).getByRole('button',{name:/Search Console/}).getAttribute('aria-pressed')).toBe('true')
    expect(within(stage).getByRole('button',{name:/Microsoft Clarity.*Needs attention/})).toBeTruthy()
    expect(within(stage).queryByRole('button',{name:/Google Analytics|Cookiebot|Algolia/})).toBeNull()
    expect(within(stage).getByRole('button',{name:/Web.*Automatic/})).toBeTruthy()
    fireEvent.click(within(stage).getByRole('button',{name:/Microsoft Clarity/}))
    expect(within(stage).getByRole('button',{name:/Microsoft Clarity/}).getAttribute('aria-pressed')).toBe('true')
  })
  it('reveals setup only through plus and restores focus when dismissed', () => {
    mount(); const plus=screen.getByRole('button',{name:'Add integration'})
    fireEvent.click(plus)
    const dialog=screen.getByRole('dialog',{name:'Extend Visibility'})
    fireEvent.click(within(dialog).getByRole('button',{name:/Cookiebot/}))
    expect(within(dialog).getByText('Setup: cookiebot')).toBeTruthy()
    fireEvent.click(within(dialog).getByRole('button',{name:'All integrations'}))
    expect(within(dialog).queryByText('Setup: cookiebot')).toBeNull()
    fireEvent(dialog,new Event('cancel',{cancelable:true}))
    expect(dialog.hasAttribute('open')).toBe(false)
    expect(document.activeElement).toBe(plus)
  })
  it('opens the selected report and keeps arrows and the tab rail on the same real source', () => {
    const [selected,setSelected]=createSignal<PulseServiceKey>('search_console'),open=vi.fn()
    render(()=><ConnectedServices sources={[source('search_console','connected'),source('ga4','connected'),source('clarity','connected')]} loading={false} error={false} canManage selected={selected()} onSelect={setSelected} onOpenReport={open} onRefresh={()=>{}} renderSetup={()=>null}/>)
    const stage=screen.getByRole('group',{name:'Connected data sources'})
    fireEvent.click(within(stage).getByRole('button',{name:/Search Console/}))
    expect(open).toHaveBeenCalledOnce()
    fireEvent.click(within(stage).getByRole('button',{name:'Next source'}))
    expect(within(stage).getByRole('button',{name:/Google Analytics/}).getAttribute('aria-pressed')).toBe('true')
    const rail=screen.getByRole('navigation',{name:'All connected sources'})
    expect(within(rail).getByRole('button',{name:'Google Analytics'}).getAttribute('aria-pressed')).toBe('true')
    fireEvent.click(within(stage).getByRole('button',{name:'Previous source'}))
    expect(within(stage).getByRole('button',{name:/Search Console/}).getAttribute('aria-pressed')).toBe('true')
    expect(stage.querySelectorAll('.pulse-services__open')).toHaveLength(1)
  })
})
