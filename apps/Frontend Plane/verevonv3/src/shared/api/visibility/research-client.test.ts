import { afterEach,describe,it,expect,vi } from 'vitest'
import { sanitizeResearch,waitForResearchRefresh } from './research-client'
import { context } from './__fixtures__/tools'
const sample=()=>({...context,source:'gsc_date_query_page',status:'connected',period:{startDate:'2026-07-28',endDate:'2026-10-25'},observedAt:'2026-10-28T12:00:00Z',lastAttemptAt:'2026-10-28T12:00:00Z',nextRunAt:'2026-10-29T12:00:00Z',stale:false,truncated:false,partial:true,rows:[{date:'2026-10-20',query:'web',page:context.site,clicks:0,impressions:4,position:3}],queries:[{key:'web',clicks:0,impressions:4,position:3,days:1}],pages:[{key:context.site,clicks:0,impressions:4,position:3,days:1}],weeks:[],relationships:[],rules:[],groups:[]})
describe('Search research contract',()=>{it('preserves observed zero and strips private values at every level',()=>{const v=sample();expect(sanitizeResearch({...v,token:'private',rows:v.rows.map(r=>({...r,token:'private'}))},context)).toEqual(v)});it('rejects cross-site, duplicate and absent evidence rather than inventing data',()=>{const v=sample();for(const changed of [{...v,site:'https://other.no/'},{...v,rows:[{...v.rows[0],page:'https://other.no/'}]},{...v,rows:[v.rows[0],v.rows[0]]},{...v,rows:[{...v.rows[0],impressions:null}]},{...v,status:'unavailable'},{...v,partial:false}])expect(()=>sanitizeResearch(changed,context)).toThrow()})})

describe('Explicit research sync confirmation',()=>{
 afterEach(()=>{vi.useRealTimers();vi.unstubAllGlobals()})
 it('reads saved state until a new attempt is recorded without starting more provider jobs',async()=>{
  vi.useFakeTimers();const queued=sanitizeResearch(sample(),context)
  const newer={...sample(),lastAttemptAt:'2026-10-28T13:00:00Z',observedAt:'2026-10-28T13:00:02Z'}
  const fetcher=vi.fn().mockResolvedValueOnce(new Response(JSON.stringify({data:sample()}))).mockResolvedValueOnce(new Response(JSON.stringify({data:newer})))
  vi.stubGlobal('fetch',fetcher);const result=waitForResearchRefresh(context,queued,new AbortController().signal)
  await vi.advanceTimersByTimeAsync(4000);expect((await result)?.observedAt).toBe(newer.observedAt)
  expect(fetcher).toHaveBeenCalledTimes(2);expect(fetcher.mock.calls.every(c=>c[1].method==='GET')).toBe(true)
 })
 it('cancels the pending wait when the site or workspace changes',async()=>{
  vi.useFakeTimers();const fetcher=vi.fn();vi.stubGlobal('fetch',fetcher);const controller=new AbortController()
  const result=waitForResearchRefresh(context,sanitizeResearch(sample(),context),controller.signal)
  const rejected=expect(result).rejects.toMatchObject({name:'AbortError'});controller.abort();await rejected
  await vi.advanceTimersByTimeAsync(10000);expect(fetcher).not.toHaveBeenCalled();expect(vi.getTimerCount()).toBe(0)
 })
})
