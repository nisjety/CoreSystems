import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { createSignal } from 'solid-js'
import { afterEach, expect, it, vi } from 'vitest'
import { AlgoliaCollectionPanel } from './AlgoliaCollectionPanel'
vi.mock('@/shared/i18n',()=>({useI18n:()=>({tr:(_:string,en:string)=>en})}))
const ctx={websiteId:`web_${'1'.repeat(32)}`,site:'https://triodelab.no/',websiteRevision:7}
const response=(data:unknown,status=200)=>new Response(JSON.stringify({data}),{status})
afterEach(()=>{cleanup();vi.unstubAllGlobals();vi.useRealTimers()})
it('reads cached job state without provider writes and queues only an explicit admin request',async()=>{
  const fetcher=vi.fn().mockImplementation((_url,init)=>Promise.resolve(response({...ctx,provider:'algolia',status:init.method==='GET'?'idle':'queued'},init.method==='GET'?200:202)));vi.stubGlobal('fetch',fetcher)
  render(()=><AlgoliaCollectionPanel websiteId={ctx.websiteId} site={ctx.site} revision={7} canManageIntegrations />)
  await screen.findByText('Not synced yet');expect(fetcher).toHaveBeenCalledOnce();fireEvent.click(screen.getByRole('button',{name:'Update data'}));await screen.findByText('Queued');expect(JSON.parse(fetcher.mock.calls[1]![1].body)).toEqual({...ctx,provider:'algolia'})
})
it('polls only GET while queued and refreshes cached report after terminal completion',async()=>{
  const onReportChanged=vi.fn(),fetcher=vi.fn().mockResolvedValueOnce(response({...ctx,provider:'algolia',status:'syncing'})).mockResolvedValue(response({...ctx,provider:'algolia',status:'ready',lastSuccessAt:'2026-10-23T12:00:00Z'}));vi.stubGlobal('fetch',fetcher)
  render(()=><AlgoliaCollectionPanel websiteId={ctx.websiteId} site={ctx.site} revision={7} onReportChanged={onReportChanged} />)
  await screen.findByText('Ready',{},{timeout:4000});expect(fetcher.mock.calls.every(call=>call[1].method==='GET')).toBe(true);expect(onReportChanged).toHaveBeenCalled();expect(screen.queryByRole('button',{name:'Update data'})).toBeNull()
})
it('aborts an old site receipt and never renders it under a new revision',async()=>{
  let finish:((value:Response)=>void)|undefined
  const fetcher=vi.fn().mockImplementationOnce(()=>new Promise<Response>(resolve=>{finish=resolve})).mockResolvedValue(response({...ctx,websiteRevision:8,provider:'algolia',status:'idle'}));vi.stubGlobal('fetch',fetcher)
  const[revision,setRevision]=createSignal(7);render(()=><AlgoliaCollectionPanel websiteId={ctx.websiteId} site={ctx.site} revision={revision()} />);await waitFor(()=>expect(fetcher).toHaveBeenCalledOnce());const signal=fetcher.mock.calls[0]![1].signal as AbortSignal;setRevision(8);await screen.findByText('Not synced yet');expect(signal.aborted).toBe(true);finish?.(response({...ctx,provider:'algolia',status:'ready'}));await Promise.resolve();expect(screen.queryByText('Ready')).toBeNull()
})
it('renders independently dated attention and reconnect receipts without inventing counts',async()=>{
  const fetcher=vi.fn().mockResolvedValue(response({...ctx,provider:'algolia',status:'needs_attention',reason:'partial_report',nextRunAt:'2026-10-23T13:00:00Z',lastAttemptAt:'2026-10-23T12:00:00Z',lastSuccessAt:'2026-10-22T12:00:00Z'}));vi.stubGlobal('fetch',fetcher)
  render(()=><AlgoliaCollectionPanel websiteId={ctx.websiteId} site={ctx.site} revision={7} />);await screen.findByText('Needs attention');expect(screen.getByText(/partial_report/)).toBeTruthy();expect(screen.getByText(/Next attempt/)).toBeTruthy();expect(screen.getByText(/Last attempt/)).toBeTruthy();expect(screen.getByText(/Last successful report/)).toBeTruthy()
  fetcher.mockResolvedValue(response({...ctx,provider:'algolia',status:'reconnect_required'}));fireEvent.click(screen.getByRole('button',{name:'Refresh status'}));await screen.findByText('Reconnect account')
})
it('labels an explicit empty observed report without treating unavailable data as zero',async()=>{
  vi.stubGlobal('fetch',vi.fn().mockResolvedValue(response({...ctx,provider:'algolia',status:'ready'})))
  const[snapshot,setSnapshot]=createSignal({algolia:{analytics:{status:'unavailable'},topSearches:{status:'connected',limit:20,rows:[]}}})
  render(()=><AlgoliaCollectionPanel websiteId={ctx.websiteId} site={ctx.site} revision={7} snapshot={snapshot() as never} />);await screen.findByText('Available report is empty')
  setSnapshot({algolia:{analytics:{status:'unavailable'},topSearches:{status:'unavailable',limit:20,rows:[]}}});await screen.findByText('Ready')
})
it('fails safely and aborts an in-flight queue when management permission is lost',async()=>{
  let finish:((value:Response)=>void)|undefined
  const fetcher=vi.fn().mockImplementation((_url,init)=>init.method==='POST'?new Promise<Response>(resolve=>{finish=resolve}):Promise.resolve(response({...ctx,provider:'algolia',status:'idle'})));vi.stubGlobal('fetch',fetcher)
  const[admin,setAdmin]=createSignal(true);render(()=><AlgoliaCollectionPanel websiteId={ctx.websiteId} site={ctx.site} revision={7} canManageIntegrations={admin()} />);await screen.findByText('Not synced yet');fireEvent.click(screen.getByRole('button',{name:'Update data'}));const signal=fetcher.mock.calls[1]![1].signal as AbortSignal;setAdmin(false);await screen.findByText('Not synced yet');expect(signal.aborted).toBe(true);finish?.(response({...ctx,provider:'algolia',status:'queued'},202));await Promise.resolve();expect(screen.queryByText('Queued')).toBeNull()
  fetcher.mockResolvedValue(new Response('private provider details',{status:503}));fireEvent.click(screen.getByRole('button',{name:'Refresh status'}));await screen.findByRole('alert');expect(screen.queryByText('private provider details')).toBeNull()
})
it('bounds polling and leaves the durable job running on the server',async()=>{
  vi.useFakeTimers();const fetcher=vi.fn().mockImplementation(()=>Promise.resolve(response({...ctx,provider:'algolia',status:'queued'})));vi.stubGlobal('fetch',fetcher)
  render(()=><AlgoliaCollectionPanel websiteId={ctx.websiteId} site={ctx.site} revision={7} />);await vi.advanceTimersByTimeAsync(125000);expect(screen.getByText(/Status checking is paused/)).toBeTruthy();expect(fetcher).toHaveBeenCalledTimes(61);await vi.advanceTimersByTimeAsync(10000);expect(fetcher).toHaveBeenCalledTimes(61)
})
