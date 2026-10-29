import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { ProviderSetup } from './ProviderSetup'
import { connectWebsiteGoogle, websiteGoogleStatus, verifyWebsite, type Website } from '@/shared/api/marketing/website-client'
vi.mock('@/shared/i18n',()=>({useI18n:()=>({tr:(_no:string,en:string)=>en})}))
vi.mock('./AlgoliaConnectionPanel',()=>({AlgoliaConnectionPanel:()=>null}))
vi.mock('@/shared/api/marketing/website-client',()=>({connectWebsiteGoogle:vi.fn(),websiteGoogleStatus:vi.fn(),verifyWebsite:vi.fn(),saveClarityMapping:vi.fn()}))
const website:Website={id:`web_${'a'.repeat(32)}`,site:'https://triodelab.no/',revision:1,createdAt:'2026-10-25T12:00:00Z',updatedAt:'2026-10-25T12:00:00Z',ownership:{status:'unverified'},integrations:{searchConsole:{status:'needs_connection'},ga4:{status:'not_connected'},clarity:{status:'not_connected'},pageSpeed:{status:'available'}}}
describe('focused provider setup',()=>{
  afterEach(()=>{cleanup();vi.resetAllMocks();vi.restoreAllMocks()})
  it('does not claim a connection or collect a key when protected setup is missing',async()=>{
    const check=vi.fn().mockResolvedValue('pending')
    render(()=><ProviderSetup provider="cookiebot" website={website} canManage onUpdated={()=>{}} onCheck={check}/>)
    fireEvent.click(screen.getByRole('button',{name:'Check setup'}))
    await screen.findByText(/No connection has been added yet/)
    expect(check).toHaveBeenCalledOnce();expect(screen.queryByRole('textbox')).toBeNull()
    expect(screen.queryByText(/connection is ready/i)).toBeNull()
  })
  it('requires an administrator for setup actions',()=>{
    const check=vi.fn()
    render(()=><ProviderSetup provider="cookiebot" website={website} canManage={false} onUpdated={()=>{}} onCheck={check}/>)
    expect(screen.getByText(/workspace administrator/)).toBeTruthy()
    expect(screen.queryByRole('button',{name:'Check setup'})).toBeNull();expect(check).not.toHaveBeenCalled()
  })
  it('finishes Google consent by checking the callback and mappings automatically',async()=>{
    vi.spyOn(window,'open').mockReturnValue(null)
    vi.mocked(connectWebsiteGoogle).mockResolvedValue({sessionId:'test-session',authorizationUrl:'https://accounts.google.com/o/oauth2/v2/auth',expiresAt:'2099-10-25T13:00:00Z'})
    vi.mocked(websiteGoogleStatus).mockResolvedValue({status:'completed'})
    vi.mocked(verifyWebsite).mockResolvedValue(website)
    const updated=vi.fn(),check=vi.fn().mockResolvedValue('ready')
    render(()=><ProviderSetup provider="search_console" website={website} canManage onUpdated={updated} onCheck={check}/>)
    fireEvent.click(screen.getByRole('button',{name:'Connect Google'}))
    await waitFor(()=>expect(updated).toHaveBeenCalledWith(website))
    expect(websiteGoogleStatus).toHaveBeenCalledWith(website.id,'test-session',expect.any(AbortSignal))
    expect(verifyWebsite).toHaveBeenCalledOnce();expect(check).toHaveBeenCalledOnce()
    expect(screen.queryByRole('button',{name:'Check connection'})).toBeNull()
  })
})
