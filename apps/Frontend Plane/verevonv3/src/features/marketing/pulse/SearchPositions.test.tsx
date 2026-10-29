import { cleanup, render, screen } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { SearchSnapshot } from '@/shared/api/marketing/search-client'
import { SearchPositions } from './SearchPositions'
vi.mock('@/shared/i18n', () => ({useI18n:()=>({tr:(_no:string,en:string)=>en,locale:()=> 'en'}),localeDateTime:()=> 'en-GB'}))
afterEach(cleanup)
const report: SearchSnapshot = {site:'https://triodelab.no/',source:'google_search_console',status:'connected',period:{startDate:'2026-10-01',endDate:'2026-10-27'},queries:[],pages:[],opportunities:[],rowLimit:100,queriesAtLimit:true,pagesAtLimit:false}
describe('reported Google query positions', () => {
 it('keeps decimal boundary positions and unknown observations honest', () => {
  const queries = [1,3,3.1,10,10.1,20,20.1,0].map((position,index)=>({key:`query ${index}`,position,clicks:0,impressions:2,ctr:0}))
  queries.push({key:'no impressions',position:1,clicks:0,impressions:0,ctr:0})
  const {container} = render(()=> <SearchPositions snapshot={{...report,queries}} onExplore={()=>{}} />)
  expect([...container.querySelectorAll('dd')].map(node=>node.textContent)).toEqual(['2','2','2','1','2'])
  expect(screen.getByText(/row limit was reached/)).toBeTruthy()
 })
 it('clears the sample while loading another site and distinguishes empty results', () => {
  const {container} = render(()=> <SearchPositions snapshot={null} onExplore={()=>{}} />)
  expect([...container.querySelectorAll('dd')].map(node=>node.textContent)).toEqual(['—','—','—','—','—'])
  expect(screen.getByRole('status').textContent).toContain('Waiting')
  cleanup()
  const empty = render(()=> <SearchPositions snapshot={{...report,queriesAtLimit:false}} onExplore={()=>{}} />)
  expect([...empty.container.querySelectorAll('dd')].map(node=>node.textContent)).toEqual(['0','0','0','0','0'])
 })
})
