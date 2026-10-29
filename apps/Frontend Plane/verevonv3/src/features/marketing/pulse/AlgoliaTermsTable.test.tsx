import { cleanup, render, screen } from '@solidjs/testing-library'
import { afterEach, expect, it, vi } from 'vitest'
import { AlgoliaTermsTable } from './AlgoliaTermsTable'
vi.mock('@/shared/i18n',()=>({useI18n:()=>({tr:(_:string,en:string)=>en})}))
afterEach(cleanup)
it('renders query text literally, including blank searches, without executing HTML',()=>{
  const {container}=render(()=><AlgoliaTermsTable title="Top searches" terms={{status:'connected',limit:20,rows:[{query:'',count:3},{query:'<script>alert(1)</script>',count:2,averageResults:1.5}]}} />)
  expect(screen.getByText('(Blank search)')).toBeTruthy();expect(screen.getByText('<script>alert(1)</script>')).toBeTruthy();expect(container.querySelector('script')).toBeNull();expect(screen.getByText('1.5')).toBeTruthy()
  expect(screen.getByText(/Up to 20 reported terms per list.*shortlist/)).toBeTruthy()
})
it('distinguishes a real empty result from an unavailable component without fake zero rows',()=>{
  render(()=><><AlgoliaTermsTable title="Top searches" terms={{status:'connected',limit:20,rows:[]}} /><AlgoliaTermsTable title="No-result terms" terms={{status:'unavailable',limit:20}} /></>)
  expect(screen.getByText('No search terms reported for this window.')).toBeTruthy();expect(screen.getByText(/This list is unavailable/)).toBeTruthy();expect(screen.queryByRole('row')).toBeNull()
})
