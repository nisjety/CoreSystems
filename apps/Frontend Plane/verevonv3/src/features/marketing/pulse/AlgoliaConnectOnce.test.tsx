import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, expect, it, vi } from 'vitest'
import { AlgoliaConnectionPanel } from './AlgoliaConnectionPanel'
vi.mock('@/shared/i18n',()=>({useI18n:()=>({tr:(_:string,en:string)=>en})}))
const ctx={websiteId:`web_${'1'.repeat(32)}`,site:'https://triodelab.no/',websiteRevision:7}
afterEach(()=>{cleanup();vi.unstubAllGlobals()})
function setup(grants=1,indices=['a','b']) {const fetcher=vi.fn().mockImplementation((url:string,init:RequestInit)=>Promise.resolve(new Response(JSON.stringify({data:url.includes('algolia-mapping')?{...ctx,status:init.method==='PUT'?'mapped':'not_mapped',...(init.method==='PUT'?{connectionId:'conn_0',applicationId:'APP1',indexName:'b'}:{})}:url.endsWith('/connections')?{connections:Array.from({length:grants},(_,n)=>({id:`conn_${n}`,status:'active'}))}:url.endsWith('/applications')?{applications:[{id:'APP1'}]}:{indices:indices.map(name=>({name}))}}))));vi.stubGlobal('fetch',fetcher);return fetcher}
it('automatically discovers unique grant/app resources but asks for an explicit choice among multiple indices',async()=>{
  const fetcher=setup();render(()=><AlgoliaConnectionPanel websiteId={ctx.websiteId} site={ctx.site} revision={7} canManageIntegrations />)
  await screen.findByRole('option',{name:'b'});expect((screen.getByLabelText('Account grant') as HTMLSelectElement).value).toBe('conn_0');expect((screen.getByLabelText('Application') as HTMLSelectElement).value).toBe('APP1');expect((screen.getByLabelText('Index') as HTMLSelectElement).value).toBe('');expect(screen.queryByRole('button',{name:'Discover applications'})).toBeNull()
  await waitFor(()=>expect((screen.getByLabelText('Index') as HTMLSelectElement).disabled).toBe(false));fireEvent.change(screen.getByLabelText('Index'),{target:{value:'b'}});fireEvent.click(screen.getByRole('button',{name:'Save site connection'}));await screen.findByText(/Site connection saved/)
  expect(fetcher.mock.calls.filter(call=>call[1].method!=='GET').map(call=>call[1].method)).toEqual(['PUT'])
})
it('does not guess among multiple eligible account grants',async()=>{
  const fetcher=setup(2);render(()=><AlgoliaConnectionPanel websiteId={ctx.websiteId} site={ctx.site} revision={7} canManageIntegrations />);await screen.findByRole('option',{name:'conn_0'});expect(fetcher.mock.calls.some(call=>call[0].endsWith('/applications'))).toBe(false);fireEvent.change(screen.getByLabelText('Account grant'),{target:{value:'conn_0'}});await screen.findByRole('option',{name:'b'})
})
it('rechecks indices after explicit rediscovery of the same unique application',async()=>{
  const fetcher=setup();render(()=><AlgoliaConnectionPanel websiteId={ctx.websiteId} site={ctx.site} revision={7} canManageIntegrations />);await screen.findByRole('option',{name:'b'});await waitFor(()=>expect((screen.getByLabelText('Index') as HTMLSelectElement).disabled).toBe(false));fireEvent.click(screen.getByRole('button',{name:'Advanced connection controls'}));fireEvent.click(screen.getByRole('button',{name:'Discover applications'}));await waitFor(()=>expect(fetcher.mock.calls.filter(call=>call[0].includes('/indices?'))).toHaveLength(2))
})
it('restores a saved index only after current grant and application discovery confirms it',async()=>{
  const fetcher=setup(2),original=fetcher.getMockImplementation()!
  fetcher.mockImplementation((url:string,init:RequestInit)=>url.includes('algolia-mapping')?Promise.resolve(new Response(JSON.stringify({data:{...ctx,status:'mapped',connectionId:'conn_0',applicationId:'APP1',indexName:'b'}}))):original(url,init))
  render(()=><AlgoliaConnectionPanel websiteId={ctx.websiteId} site={ctx.site} revision={7} canManageIntegrations />)
  await screen.findByRole('option',{name:'b'});await waitFor(()=>expect((screen.getByLabelText('Index') as HTMLSelectElement).disabled).toBe(false));expect((screen.getByLabelText('Index') as HTMLSelectElement).value).toBe('b');expect((screen.getByRole('button',{name:'Save site connection'}) as HTMLButtonElement).disabled).toBe(true);expect(fetcher.mock.calls.every(call=>call[1].method==='GET')).toBe(true)
})
it('automatically checks an explicitly chosen application without clearing its in-flight proof',async()=>{
  const fetcher=setup(),original=fetcher.getMockImplementation()!
  fetcher.mockImplementation((url:string,init:RequestInit)=>url.endsWith('/applications')?Promise.resolve(new Response(JSON.stringify({data:{applications:[{id:'APP1'},{id:'APP2'}]}}))):original(url,init))
  render(()=><AlgoliaConnectionPanel websiteId={ctx.websiteId} site={ctx.site} revision={7} canManageIntegrations />)
  await screen.findByRole('option',{name:'APP2'});await waitFor(()=>expect((screen.getByLabelText('Application') as HTMLSelectElement).disabled).toBe(false));expect((screen.getByLabelText('Application') as HTMLSelectElement).value).toBe('');fireEvent.change(screen.getByLabelText('Application'),{target:{value:'APP2'}});await screen.findByRole('option',{name:'b'});expect(fetcher.mock.calls.some(call=>call[0].includes('applicationId=APP2'))).toBe(true)
})
it('visually restores the mapped grant when mapping arrives before account options',async()=>{
  const fetcher=setup(2),original=fetcher.getMockImplementation()!;let finish:((value:Response)=>void)|undefined
  fetcher.mockImplementation((url:string,init:RequestInit)=>url.endsWith('/connections')?new Promise<Response>(resolve=>{finish=resolve}):url.includes('algolia-mapping')?Promise.resolve(new Response(JSON.stringify({data:{...ctx,status:'mapped',connectionId:'conn_1',applicationId:'APP1',indexName:'b'}}))):original(url,init))
  render(()=><AlgoliaConnectionPanel websiteId={ctx.websiteId} site={ctx.site} revision={7} canManageIntegrations />);await screen.findByRole('button',{name:'Disconnect this site'});finish?.(new Response(JSON.stringify({data:{connections:[{id:'conn_0',status:'active'},{id:'conn_1',status:'active'}]}})));await screen.findByRole('option',{name:'b'});expect((screen.getByLabelText('Account grant') as HTMLSelectElement).value).toBe('conn_1')
})
