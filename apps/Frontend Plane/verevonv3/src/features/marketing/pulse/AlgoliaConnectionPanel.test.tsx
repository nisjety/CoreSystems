import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { createSignal } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { AlgoliaConnectionPanel } from './AlgoliaConnectionPanel'
vi.mock('@/shared/i18n', () => ({ useI18n: () => ({ tr: (_no: string, en: string) => en }) }))
const ctx={websiteId:`web_${'1'.repeat(32)}`,site:'https://triodelab.no/',websiteRevision:7}
const ids={connectionId:'conn_test',applicationId:'APP1',indexName:'public_index'}
const unmapped={...ctx,status:'not_mapped'},mapped={...ctx,status:'mapped',...ids}
const response=(data:unknown)=>new Response(JSON.stringify({data}))
const consent={id:'cs_test',expiresAt:'2026-10-23T12:00:00Z',authorizationUrl:'https://dashboard.algolia.com/2/oauth/authorize?response_type=code&scope=public&resource=https%3A%2F%2Fmcp.algolia.com%2Fmcp&redirect_uri=http%3A%2F%2Flocalhost%3A3026%2Fapi%2Fv1%2Fproviders%2Falgolia%2Fcallback&client_id=public-client&state=opaque-state&code_challenge=abcdefghijklmnopqrstuvwxyz0123456789ABCDE12&code_challenge_method=S256'}
function setup(options:{syncFail?:boolean;mapping?:unknown}={}) {
  const fetcher=vi.fn().mockImplementation((url:string,init:RequestInit)=>{
    if(url.includes('algolia-mapping')) return Promise.resolve(response(init.method==='PUT'?mapped:init.method==='DELETE'?unmapped:options.mapping??unmapped))
    if(url.endsWith('/connections'))return Promise.resolve(response({connections:[{id:'conn_test',status:'active',displayName:'My Algolia account'},{id:'conn_other',status:'active',displayName:'Other grant'}]}))
    if(url.endsWith('/applications'))return Promise.resolve(response({applications:[{id:'APP1'}]}))
    if(url.includes('/indices?'))return Promise.resolve(response({indices:[{name:'public_index'}]}))
    if(url.endsWith('/site-sources/sync'))return Promise.resolve(options.syncFail?new Response('secret',{status:503}):response({...ctx,provider:'algolia',status:'not_configured',observedAt:'2026-10-23T10:00:00Z',window:{startDate:'2026-10-16',endDate:'2026-10-22'}}))
    return Promise.resolve(new Response('unavailable',{status:503}))
  });vi.stubGlobal('fetch',fetcher);return fetcher
}
const mount=()=>render(()=><AlgoliaConnectionPanel websiteId={ctx.websiteId} site={ctx.site} revision={7} canManageIntegrations />)
async function ready(){await screen.findByRole('option',{name:/My Algolia account|conn_test/});const advanced=screen.getByRole('button',{name:'Advanced connection controls'});if(advanced.getAttribute('aria-expanded')==='false')fireEvent.click(advanced)}
async function choose(){await ready();fireEvent.change(screen.getByLabelText('Account grant'),{target:{value:'conn_test'}});await screen.findByRole('option',{name:'public_index'});await waitFor(()=>expect((screen.getByLabelText('Index') as HTMLSelectElement).disabled).toBe(false));fireEvent.change(screen.getByLabelText('Index'),{target:{value:'public_index'}})}
describe('Algolia guided OAuth mapping',()=>{
  beforeEach(()=>{vi.useFakeTimers({toFake:['Date']});vi.setSystemTime(new Date('2026-10-23T11:00:00Z'))})
  afterEach(()=>{cleanup();vi.unstubAllGlobals();vi.useRealTimers()})
  it('loads metadata only and describes account-wide read-only access without API-key fields',async()=>{
    const fetcher=setup();mount();await screen.findByRole('option',{name:'My Algolia account · conn_test'})
    expect(fetcher.mock.calls.every((call)=>call[1].method==='GET')).toBe(true);expect(fetcher.mock.calls).toHaveLength(2)
    expect(screen.getByText(/enable Productivity MCP.*Algolia Dashboard/)).toBeTruthy();expect(screen.getByText(/account-wide.*read-only/)).toBeTruthy();expect(screen.queryByRole('textbox')).toBeNull()
  })
  it('preserves account discovery when site mapping access cannot be confirmed and blocks site writes',async()=>{
    const fetcher=setup(),original=fetcher.getMockImplementation()!
    fetcher.mockImplementation((url:string,init:RequestInit)=>url.includes('algolia-mapping')?Promise.resolve(new Response('private provider details',{status:403})):original(url,init))
    mount();await choose();await screen.findByText(/Site mapping access could not be confirmed/)
    const save=screen.getByRole('button',{name:'Save site connection'}) as HTMLButtonElement
    expect(save.disabled).toBe(true);fireEvent.click(save)
    expect(screen.queryByRole('button',{name:'Disconnect this site'})).toBeNull();expect(fetcher.mock.calls.every(call=>call[1].method==='GET')).toBe(true);expect(screen.queryByText('private provider details')).toBeNull()
  })
  it('does not hold account discovery behind a pending site mapping request',async()=>{
    const fetcher=setup(),original=fetcher.getMockImplementation()!;let finish:((value:Response)=>void)|undefined
    fetcher.mockImplementation((url:string,init:RequestInit)=>url.includes('algolia-mapping')?new Promise<Response>(resolve=>{finish=resolve}):original(url,init))
    mount();await choose();expect((screen.getByRole('button',{name:'Save site connection'}) as HTMLButtonElement).disabled).toBe(true)
    finish?.(response(unmapped));await waitFor(()=>expect((screen.getByRole('button',{name:'Save site connection'}) as HTMLButtonElement).disabled).toBe(false))
  })
  it('does not restore a mapped grant over an explicit empty selection while the site read is pending',async()=>{
    const fetcher=setup(),original=fetcher.getMockImplementation()!;let finish:((value:Response)=>void)|undefined
    fetcher.mockImplementation((url:string,init:RequestInit)=>url.includes('algolia-mapping')?new Promise<Response>(resolve=>{finish=resolve}):original(url,init))
    mount();await ready();fireEvent.change(screen.getByLabelText('Account grant'),{target:{value:''}});finish?.(response(mapped));await screen.findByRole('button',{name:'Disconnect this site'})
    expect((screen.getByLabelText('Account grant') as HTMLSelectElement).value).toBe('')
  })
  async function enterManual(id='USERAPP1') {
    await ready()
    fireEvent.change(screen.getByLabelText('Application source'),{target:{value:'provided'}})
    fireEvent.change(screen.getByLabelText('Account grant'),{target:{value:'conn_test'}})
    fireEvent.input(screen.getByLabelText('Application ID (user supplied)'),{target:{value:id}})
  }
  it('accepts a user-supplied application only through an explicit successful provider index read before mapping',async()=>{
    const fetcher=setup(),original=fetcher.getMockImplementation()!
    fetcher.mockImplementation((url:string,init:RequestInit)=>url.includes('algolia-mapping')&&init.method==='PUT'?Promise.resolve(response({...mapped,applicationId:'USERAPP1'})):original(url,init))
    mount();await enterManual();expect(screen.getByText(/User supplied, not discovered/)).toBeTruthy();expect((screen.getByRole('button',{name:'Save site connection'}) as HTMLButtonElement).disabled).toBe(true)
    fireEvent.click(screen.getByRole('button',{name:'Discover indices'}));await screen.findByRole('option',{name:'public_index'});await waitFor(()=>expect((screen.getByLabelText('Index') as HTMLSelectElement).disabled).toBe(false));fireEvent.change(screen.getByLabelText('Index'),{target:{value:'public_index'}});fireEvent.click(screen.getByRole('button',{name:'Save site connection'}));await screen.findByText(/Site connection saved/)
    expect(fetcher.mock.calls.some(call=>call[0].includes('/indices?applicationId=USERAPP1'))).toBe(true);expect(JSON.parse(fetcher.mock.calls.find(call=>call[1].method==='PUT')![1].body).applicationId).toBe('USERAPP1')
    expect(fetcher.mock.calls.some(call=>call[0].endsWith('/applications'))).toBe(false)
  })
  it('rejects unsafe manual identifiers and never treats the input as permission proof',async()=>{
    const fetcher=setup();mount();await enterManual('APP/secret')
    for(const id of ['APP/secret','space id','x'.repeat(65),'']) {fireEvent.input(screen.getByLabelText('Application ID (user supplied)'),{target:{value:id}});const discover=screen.getByRole('button',{name:'Discover indices'}) as HTMLButtonElement;expect(discover.disabled).toBe(true);fireEvent.click(discover)}
    expect(fetcher.mock.calls.some(call=>call[0].includes('/indices?'))).toBe(false);expect((screen.getByRole('button',{name:'Save site connection'}) as HTMLButtonElement).disabled).toBe(true)
  })
  it('clears provider proof immediately on manual ID changes and ignores a late old-ID index response',async()=>{
    const fetcher=setup(),original=fetcher.getMockImplementation()!;let finish:((value:Response)=>void)|undefined
    fetcher.mockImplementation((url:string,init:RequestInit)=>url.includes('/indices?')?new Promise<Response>(resolve=>{finish=resolve}):original(url,init))
    mount();await enterManual('FIRSTAPP');fireEvent.click(screen.getByRole('button',{name:'Discover indices'}));await waitFor(()=>expect(fetcher.mock.calls.some(call=>call[0].includes('applicationId=FIRSTAPP'))).toBe(true))
    fireEvent.input(screen.getByLabelText('Application ID (user supplied)'),{target:{value:'SECONDAPP'}});finish?.(response({indices:[{name:'old_index'}]}));await waitFor(()=>expect((screen.getByRole('button',{name:'Discover indices'}) as HTMLButtonElement).disabled).toBe(false));expect(screen.queryByRole('option',{name:'old_index'})).toBeNull();expect((screen.getByRole('button',{name:'Save site connection'}) as HTMLButtonElement).disabled).toBe(true)
  })
  it('does not report a late index error against a different application choice',async()=>{
    const fetcher=setup(),original=fetcher.getMockImplementation()!;let finish:((value:Response)=>void)|undefined
    fetcher.mockImplementation((url:string,init:RequestInit)=>url.includes('/indices?')?new Promise<Response>(resolve=>{finish=resolve}):original(url,init))
    mount();await enterManual('FIRSTAPP');fireEvent.click(screen.getByRole('button',{name:'Discover indices'}));fireEvent.input(screen.getByLabelText('Application ID (user supplied)'),{target:{value:'SECONDAPP'}});finish?.(new Response('private',{status:503}));await waitFor(()=>expect((screen.getByRole('button',{name:'Discover indices'}) as HTMLButtonElement).disabled).toBe(false));expect(screen.queryByRole('alert')).toBeNull()
  })
  it('clears a verified manual index when switching input mode or grant',async()=>{
    setup();mount();await enterManual();fireEvent.click(screen.getByRole('button',{name:'Discover indices'}));await screen.findByRole('option',{name:'public_index'});await waitFor(()=>expect((screen.getByLabelText('Index') as HTMLSelectElement).disabled).toBe(false));fireEvent.change(screen.getByLabelText('Index'),{target:{value:'public_index'}})
    fireEvent.change(screen.getByLabelText('Application source'),{target:{value:'discovered'}});expect(screen.queryByRole('option',{name:'public_index'})).toBeNull();expect((screen.getByRole('button',{name:'Save site connection'}) as HTMLButtonElement).disabled).toBe(true)
    fireEvent.change(screen.getByLabelText('Account grant'),{target:{value:''}});expect(screen.queryByRole('option',{name:'public_index'})).toBeNull()
  })
  it('discovers only explicit grant/app choices then saves mapping and separately fetches aggregates',async()=>{
    const fetcher=setup();mount();await choose();fireEvent.click(screen.getByRole('button',{name:'Save site connection'}))
    await screen.findByText(/Site connection saved/)
    const writes=fetcher.mock.calls.filter((call)=>call[1].method!=='GET');expect(writes.map(call=>call[1].method)).toEqual(['PUT'])
    expect(JSON.parse(writes[0]![1].body)).toEqual({...ctx,...ids})
  })
  it('keeps a successful mapping distinct from a failed initial analytics fetch',async()=>{
    setup({syncFail:true});mount();await choose();fireEvent.click(screen.getByRole('button',{name:'Save site connection'}))
    await screen.findByText(/Site connection saved/);expect(screen.queryByText('secret')).toBeNull()
  })
  it('disconnects the site only without revoking the entire account grant or falling back to keys',async()=>{
    const fetcher=setup({mapping:mapped});mount();await ready();await waitFor(()=>expect((screen.getByRole('button',{name:'Disconnect this site'}) as HTMLButtonElement).disabled).toBe(false));fireEvent.click(screen.getByRole('button',{name:'Disconnect this site'}));await screen.findByText(/Site mapping disconnected.*account grant remains/)
    const writes=fetcher.mock.calls.filter(call=>call[1].method!=='GET');expect(writes).toHaveLength(1);expect(writes[0]![1].method).toBe('DELETE');expect(JSON.parse(writes[0]![1].body)).toEqual(ctx)
  })
  it('shows reconnect/access-check requirements without assuming OAuth succeeded',async()=>{
    setup({mapping:{...mapped,status:'access_check_required'}});mount();await screen.findByText(/Access must be checked again/)
    expect(screen.queryByText('Connected')).toBeNull()
  })
  it('aborts discovery and drops old-site choices after revision changes',async()=>{
    const fetcher=setup();let finish:((v:Response)=>void)|undefined
    fetcher.mockImplementation((url:string)=>url.endsWith('/applications')?new Promise<Response>(resolve=>{finish=resolve}):Promise.resolve(response(url.endsWith('/connections')?{connections:[{id:'conn_test',status:'active'}]}:{...unmapped,websiteRevision:8})))
    const[revision,setRevision]=createSignal(8);render(()=><AlgoliaConnectionPanel websiteId={ctx.websiteId} site={ctx.site} revision={revision()} canManageIntegrations />)
    await ready();fireEvent.change(screen.getByLabelText('Account grant'),{target:{value:'conn_test'}});fireEvent.click(screen.getByRole('button',{name:'Discover applications'}));await waitFor(()=>expect(fetcher.mock.calls.some(call=>call[0].endsWith('/applications'))).toBe(true))
    const call=fetcher.mock.calls.find(call=>call[0].endsWith('/applications'))!;setRevision(9);await waitFor(()=>expect((call[1].signal as AbortSignal).aborted).toBe(true));finish?.(response({applications:[{id:'OLDAPP'}]}));await Promise.resolve();await Promise.resolve();expect(screen.queryByRole('option',{name:'OLDAPP'})).toBeNull()
  })
  it('starts consent only on explicit click and reloads metadata after completed status without assuming a site mapping',async()=>{
    const fetcher=setup(), original=fetcher.getMockImplementation()!
    fetcher.mockImplementation((url:string,init:RequestInit)=>url.endsWith('/connect-session')?Promise.resolve(response(consent)):url.endsWith('/status')?Promise.resolve(response({id:consent.id,status:'completed',expiresAt:consent.expiresAt})):original(url,init))
    mount();await ready();expect(fetcher.mock.calls.some(call=>call[0].endsWith('/connect-session'))).toBe(false)
    fireEvent.click(screen.getByRole('button',{name:'Sign in with Algolia'}));const link=await screen.findByRole('link',{name:'Continue to Algolia'});expect(link.getAttribute('href')).toBe(consent.authorizationUrl);expect(link.getAttribute('rel')).toBe('noopener noreferrer')
    await screen.findByText(/Nothing is mapped/);fireEvent.click(link)
    await waitFor(()=>expect(fetcher.mock.calls.filter(call=>call[0].endsWith('/connections')).length).toBe(2));expect(screen.queryByRole('button',{name:'Disconnect this site'})).toBeNull()
  })
  it.each(['failed','expired'])('shows %s consent honestly with no mapping or analytics fetch',async(status)=>{
    const fetcher=setup(), original=fetcher.getMockImplementation()!
    fetcher.mockImplementation((url:string,init:RequestInit)=>url.endsWith('/connect-session')?Promise.resolve(response(consent)):url.endsWith('/status')?Promise.resolve(response({id:consent.id,status,expiresAt:consent.expiresAt})):original(url,init))
    mount();await ready();fireEvent.click(screen.getByRole('button',{name:'Sign in with Algolia'}));fireEvent.click(await screen.findByRole('link',{name:'Continue to Algolia'}));await screen.findByText(/Sign-in expired or failed/);expect(fetcher.mock.calls.some(call=>call[1].method==='PUT'||call[0].endsWith('/site-sources/sync'))).toBe(false)
  })
  it('stops status polling at expiry and exposes a safe retry instruction',async()=>{
    const fetcher=setup(),original=fetcher.getMockImplementation()!
    fetcher.mockImplementation((url:string,init:RequestInit)=>url.endsWith('/connect-session')?Promise.resolve(response({...consent,expiresAt:'2026-10-23T10:59:59Z'})):original(url,init))
    mount();await ready();fireEvent.click(screen.getByRole('button',{name:'Sign in with Algolia'}));fireEvent.click(await screen.findByRole('link',{name:'Continue to Algolia'}));await screen.findByText(/Status checking ended/);expect(fetcher.mock.calls.some(call=>call[0].endsWith('/status'))).toBe(false)
  })
  it('fails closed when consent status cannot be checked',async()=>{
    const fetcher=setup(),original=fetcher.getMockImplementation()!
    fetcher.mockImplementation((url:string,init:RequestInit)=>url.endsWith('/connect-session')?Promise.resolve(response(consent)):url.endsWith('/status')?Promise.resolve(new Response('provider-private',{status:503})):original(url,init))
    mount();await ready();fireEvent.click(screen.getByRole('button',{name:'Sign in with Algolia'}));fireEvent.click(await screen.findByRole('link',{name:'Continue to Algolia'}));await screen.findByText(/Consent status could not be read/);expect(screen.queryByText('provider-private')).toBeNull()
  })
  it('polls a pending session with bounded intervals until completion',async()=>{
    const fetcher=setup(),original=fetcher.getMockImplementation()!;let checks=0
    fetcher.mockImplementation((url:string,init:RequestInit)=>url.endsWith('/connect-session')?Promise.resolve(response(consent)):url.endsWith('/status')?Promise.resolve(response({id:consent.id,status:++checks===1?'pending':'completed',expiresAt:consent.expiresAt})):original(url,init))
    mount();await ready();fireEvent.click(screen.getByRole('button',{name:'Sign in with Algolia'}));fireEvent.click(await screen.findByRole('link',{name:'Continue to Algolia'}));await waitFor(()=>expect(fetcher.mock.calls.filter(call=>call[0].endsWith('/connections'))).toHaveLength(2),{timeout:4000});expect(checks).toBe(2)
  })
  it('does not request metadata when integration management is not allowed',async()=>{
    const fetcher=setup();render(()=><AlgoliaConnectionPanel websiteId={ctx.websiteId} site={ctx.site} revision={7} />);await Promise.resolve();expect(fetcher).not.toHaveBeenCalled();expect(screen.queryByTestId('algolia-connection-panel')).toBeNull()
  })
  it('shows discovery and load errors without relaying provider content',async()=>{
    const fetcher=setup();fetcher.mockResolvedValue(new Response('sensitive provider error',{status:503}));mount();await screen.findByText(/setup could not be loaded/);expect(screen.queryByText('sensitive provider error')).toBeNull()
    setup();fireEvent.click(screen.getByRole('button',{name:'Refresh account status'}));await ready();const requests=vi.mocked(fetch);requests.mockResolvedValue(new Response('private',{status:503}));fireEvent.change(screen.getByLabelText('Account grant'),{target:{value:'conn_test'}});fireEvent.click(screen.getByRole('button',{name:'Discover applications'}));await screen.findByRole('alert');expect(screen.getByRole('alert').textContent).toContain('Application discovery could not be confirmed. You can provide an Application ID and verify its indices; no access is assumed.')
  })
  it('handles empty application and index discovery and reconnect requirements without fabricating resources',async()=>{
    const fetcher=setup({mapping:{...mapped,status:'reconnect_required'}}),original=fetcher.getMockImplementation()!
    fetcher.mockImplementation((url:string,init:RequestInit)=>url.endsWith('/applications')?Promise.resolve(response({applications:[]})):original(url,init))
    mount();await ready();await screen.findByText(/Account access must reconnect/);await screen.findByText(/No applications available/)
    fetcher.mockImplementation((url:string,init:RequestInit)=>url.includes('/indices?')?Promise.resolve(response({indices:[]})):original(url,init));fireEvent.click(screen.getByRole('button',{name:'Discover applications'}));await screen.findByRole('option',{name:'APP1'});await waitFor(()=>expect((screen.getByLabelText('Application') as HTMLSelectElement).disabled).toBe(false));fireEvent.change(screen.getByLabelText('Application'),{target:{value:'APP1'}});fireEvent.click(screen.getByRole('button',{name:'Discover indices'}));await screen.findByText(/No indices are available/)
  })
})
