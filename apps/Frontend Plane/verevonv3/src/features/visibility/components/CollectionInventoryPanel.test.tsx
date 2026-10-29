// @vitest-environment jsdom
import { cleanup,fireEvent,render,screen,waitFor } from '@solidjs/testing-library'
import { createSignal } from 'solid-js'
import { afterEach,beforeEach,it,expect,vi } from 'vitest'
import { I18nProvider } from '@/shared/i18n'
import { context } from '@/shared/api/visibility/__fixtures__/tools'
import { inventory } from '@/shared/api/visibility/__fixtures__/collection'
import { loadCollectionInventory,type CollectionInventory } from '@/shared/api/visibility/collection-client'
import { CollectionInventoryPanel } from './CollectionInventoryPanel'
vi.mock('@/shared/api/visibility/collection-client',()=>({loadCollectionInventory:vi.fn()}))
beforeEach(()=>vi.mocked(loadCollectionInventory).mockReset())
afterEach(cleanup)
it('renders measured zero separately from pending data and filters gaps without starting collection',async()=>{
 vi.mocked(loadCollectionInventory).mockResolvedValue(inventory());const manage=vi.fn()
 render(()=><I18nProvider><CollectionInventoryPanel context={context} quarter="Q4" onManage={manage}/></I18nProvider>)
 await screen.findByText(/Datakilder, dekning og historikk|Sources, coverage and history/)
 expect(screen.getByText(/0 søkeordrader|0 query rows/)).toBeTruthy()
 fireEvent.click(screen.getByRole('checkbox',{name:/Vis bare mangler|Show gaps/}))
 expect(screen.queryByText(/0 søkeordrader|0 query rows/)).toBeNull()
 await waitFor(()=>expect(screen.getByRole('button',{name:/Les lagret status|Read saved status/}).hasAttribute('disabled')).toBe(false))
 fireEvent.click(screen.getByRole('button',{name:/Les lagret status|Read saved status/}))
 await waitFor(()=>expect(loadCollectionInventory).toHaveBeenCalledTimes(2))
 await screen.findByRole('button',{name:/Administrer nettsted|Manage website/})
 fireEvent.click(screen.getByRole('button',{name:/Administrer nettsted|Manage website/}));expect(manage).toHaveBeenCalledTimes(1)
})
it('aborts and clears old-site inventory before accepting the next response',async()=>{
 let old!:(v:CollectionInventory)=>void
 vi.mocked(loadCollectionInventory).mockImplementationOnce(()=>new Promise(resolve=>{old=resolve})).mockRejectedValueOnce(Error('unavailable'))
 const [current,setCurrent]=createSignal(context)
 render(()=><I18nProvider><CollectionInventoryPanel context={current()} quarter="Q4" onManage={()=>{}}/></I18nProvider>)
 const signal=vi.mocked(loadCollectionInventory).mock.calls[0]![2]!
 setCurrent({...context,websiteRevision:8});expect(signal.aborted).toBe(true);old(inventory())
 await screen.findByRole('alert');expect(screen.queryByText(/0 søkeordrader|0 query rows/)).toBeNull()
})
