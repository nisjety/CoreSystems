import { describe,it,expect } from 'vitest'
import { sanitizeCollectionInventory } from './collection-client'
import { inventory } from './__fixtures__/collection'
import { context } from './__fixtures__/tools'
describe('Collection inventory boundary',()=>{
 it('preserves measured zero, absent counts and provenance while stripping secrets',()=>{
  const value=inventory();expect(sanitizeCollectionInventory({...value,token:'private',datasets:value.datasets.map(d=>({...d,cookie:'private'})),summary:{...value.summary,bearer:'private'}},context,'Q4')).toEqual(value)
 })
 it('rejects mixed tenants/sites, invented zeroes, inconsistent totals and malformed receipts',()=>{
  const v=inventory()
  for(const changed of [{...v,websiteRevision:8},{...v,site:'https://other.no/'},{...v,quarter:'Q1'},{...v,summary:{...v.summary,available:99}},{...v,datasets:[...v.datasets,v.datasets[0]]},{...v,asOf:'today'},{...v,datasets:v.datasets.map(d=>d.status==='pending'?{...d,records:0}:d)},{...v,datasets:v.datasets.map(d=>({...d,period:{startDate:'2026-02-30',endDate:'2026-03-01'}}))}])expect(()=>sanitizeCollectionInventory(changed,context,'Q4')).toThrow()
 })
})
