import { createRoot, createSignal } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { createPulseSync } from './pulse-sync'
import { prepareWebsite } from '@/shared/api/marketing/preparation-client'
import { requestAnalyticsSync } from '@/shared/api/marketing/analytics-sync-client'
import { requestPublicSignals } from '@/shared/api/marketing/public-signals-client'
import type { MarketingSource } from '@/shared/api/marketing/preparation-client'
vi.mock('@/shared/api/marketing/preparation-client',()=>({prepareWebsite:vi.fn(),loadWebsitePreparation:vi.fn()}))
vi.mock('@/shared/api/marketing/analytics-sync-client',()=>({requestAnalyticsSync:vi.fn(),loadAnalyticsSync:vi.fn()}))
vi.mock('@/shared/api/marketing/public-signals-client',()=>({requestPublicSignals:vi.fn(),loadPublicSignals:vi.fn()}))
const context={websiteId:`web_${'a'.repeat(32)}`,site:'https://triodelab.no/',websiteRevision:3,quarter:'Q4' as const}
const publicSnapshot={...context,source:'marketing_public_signals' as const,status:'complete' as const,enabled:true,report:{site:context.site,source:'public_web_signals' as const,observedAt:'2026-10-25T12:00:00Z',crux:[],commonCrawl:{status:'unavailable' as const,reason:'no_data' as const,captures:[],limit:20 as const,truncated:false}}}
const preparation={...context,source:'marketing_preparation' as const,status:'complete' as const,steps:[]}
const sources:MarketingSource[]=[{key:'search_console',implemented:true,mode:'oauth',status:'connected',refresh:'snapshot_read'}]
describe('one Visibility synchronization intent',()=>{
  afterEach(()=>vi.resetAllMocks())
  function setup(allowed=true,items:MarketingSource[]=sources){
    vi.mocked(prepareWebsite).mockResolvedValue(preparation)
    vi.mocked(requestPublicSignals).mockResolvedValue(publicSnapshot)
    vi.mocked(requestAnalyticsSync).mockResolvedValue({...context,source:'marketing_analytics_sync',enabled:true,status:'partial',steps:[{key:'gsc',status:'connected',observedAt:'2026-10-25T12:00:00Z'},{key:'ga4',status:'unavailable',reason:'not_connected',observedAt:'2026-10-25T12:00:00Z'},{key:'clarity',status:'unavailable',reason:'not_connected',observedAt:'2026-10-25T12:00:00Z'}]})
    const settled=vi.fn(),[selected,setSelected]=createSignal(context)
    let flow!:ReturnType<typeof createPulseSync>
    const dispose=createRoot(dispose=>{flow=createPulseSync(selected,()=>items,()=>allowed,settled);return dispose})
    return{flow,dispose,settled,setSelected}
  }
  it('uses one selected site/revision and skips providers that have not been connected',async()=>{
    const s=setup(true,[]);await s.flow.sync()
    expect(requestAnalyticsSync).not.toHaveBeenCalled()
    expect(prepareWebsite).toHaveBeenCalledWith(context,expect.any(AbortSignal))
    expect(requestPublicSignals).toHaveBeenCalledWith(context,expect.any(AbortSignal))
    expect(s.flow.results().every(result=>result.status==='complete')).toBe(true)
    expect(s.settled).toHaveBeenCalledOnce();s.dispose()
  })
  it('isolates provider failure and does not report disconnected providers as failures',async()=>{
    const s=setup();vi.mocked(requestPublicSignals).mockRejectedValue(new Error('upstream'))
    await s.flow.sync()
    expect(s.flow.results()).toContainEqual({key:'public',status:'attention'})
    expect(s.flow.results()).toContainEqual({key:'analytics',status:'complete'})
    expect(s.flow.busy()).toBe(false);s.dispose()
  })
  it('does not allow writes from a read-only actor',async()=>{
    const s=setup(false);await s.flow.sync();expect(prepareWebsite).not.toHaveBeenCalled();expect(requestAnalyticsSync).not.toHaveBeenCalled();s.dispose()
  })
  it('cancels client tracking and rejects a late result when the site changes',async()=>{
    const s=setup();let finish!:(value:typeof preparation)=>void
    vi.mocked(prepareWebsite).mockImplementationOnce(()=>new Promise(resolve=>{finish=resolve}))
    const pending=s.flow.sync();await s.flow.sync()
    expect(prepareWebsite).toHaveBeenCalledOnce()
    s.setSelected({...context,websiteRevision:4});finish(preparation);await pending
    expect(s.flow.results()).toEqual([]);expect(s.settled).not.toHaveBeenCalled();s.dispose()
  })
})
