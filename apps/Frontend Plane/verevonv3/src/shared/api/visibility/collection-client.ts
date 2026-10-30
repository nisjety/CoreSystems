import { visibilityToolsContracts } from '@/shared/actions/visibility-tools'
import { validateDiscoveryContext, type DiscoveryContext } from '../marketing/discovery-client'

export const collectionStatuses = ['available','empty','pending','stale','failed','reconnect','setup','unsupported'] as const
export type CollectionStatus = typeof collectionStatuses[number]
export type Dataset = {
 key:string;provider:string;category:'public'|'account'|'market'|'first_party';status:CollectionStatus;reason?:string
 coverage:string;partial:boolean;records:number|null;unit:string;lastSuccessAt?:string;lastAttemptAt?:string;nextSyncAt?:string
 reportingResource?:string;period?:{startDate:string;endDate:string};history:string;collectionMode:string
}
export type CollectionInventory=DiscoveryContext & {
 source:'visibility_collection_inventory';asOf:string;quarter:string;datasets:Dataset[]
 summary:{available:number;empty:number;pending:number;stale:number;attention:number;unsupported:number}
}
const object=(v:unknown):Record<string,unknown>=>{if(!v||typeof v!=='object'||Array.isArray(v))throw Error('Invalid inventory');return v as Record<string,unknown>}
const text=(v:unknown,max=256)=>{if(typeof v!=='string'||v.length>max||/[\x00-\x1f\x7f]/.test(v))throw Error('Invalid inventory text');return v}
const stamp=(v:unknown)=>{const s=text(v,40);if(!s||!/^\d{4}-\d\d-\d\dT/.test(s)||!Number.isFinite(Date.parse(s)))throw Error('Invalid inventory receipt');return s}
const count=(v:unknown)=>{if(typeof v!=='number'||!Number.isSafeInteger(v)||v<0)throw Error('Invalid inventory count');return v}
const member=<T extends string>(v:unknown,choices:readonly T[]):T=>{if(!choices.includes(v as T))throw Error('Invalid inventory value');return v as T}
export function sanitizeCollectionInventory(value:unknown,expected:DiscoveryContext,quarter:string):CollectionInventory {
 const r=object(value),scope=validateDiscoveryContext(r)
 if(scope.websiteId!==expected.websiteId||scope.site!==expected.site||scope.websiteRevision!==expected.websiteRevision||r.source!=='visibility_collection_inventory'||r.quarter!==quarter||!Array.isArray(r.datasets)||r.datasets.length>32)throw Error('Unexpected inventory context')
 const datasets=r.datasets.map((value):Dataset=>{
  const d=object(value),key=text(d.key,64);if(!/^[a-z][a-z0-9_]{0,63}$/.test(key)||typeof d.partial!=='boolean')throw Error('Invalid dataset')
  const out:Dataset={key,provider:text(d.provider,64),category:member(d.category,['public','account','market','first_party']),status:member(d.status,collectionStatuses),coverage:text(d.coverage,64),partial:d.partial,records:d.records===null?null:count(d.records),unit:text(d.unit,40),history:member(d.history,['latest_snapshot','bounded_audits','bounded_comparisons','rolling_90_days','retained_samples','none']),collectionMode:member(d.collectionMode,['scheduled','manual','paused','source_managed','unconfigured'])}
  if(d.reason!==undefined)out.reason=member(d.reason,['authorization_required','setup_required','plan_required','expired','no_coverage','quota_limited','source_unavailable','invalid_receipt','additional_source_required'])
  for(const key of ['lastSuccessAt','lastAttemptAt','nextSyncAt'] as const)if(d[key]!==undefined)out[key]=stamp(d[key])
  if(d.reportingResource!==undefined)out.reportingResource=text(d.reportingResource)
  if(d.period!==undefined){const p=object(d.period),startDate=text(p.startDate,10),endDate=text(p.endDate,10);for(const s of [startDate,endDate])if(s&&(!/^\d{4}-\d\d-\d\d$/.test(s)||!Number.isFinite(Date.parse(s+'T00:00:00Z'))||new Date(s+'T00:00:00Z').toISOString().slice(0,10)!==s))throw Error('Invalid inventory period');if(Boolean(startDate)!==Boolean(endDate)||endDate<startDate)throw Error('Invalid inventory period');out.period={startDate,endDate}}
  if(out.status==='available'&&!out.lastSuccessAt||['setup','reconnect','pending','unsupported'].includes(out.status)&&out.records!==null)throw Error('Absent inventory evidence')
  return out
 })
 if(new Set(datasets.map(d=>d.key)).size!==datasets.length)throw Error('Duplicate dataset')
 const s=object(r.summary),summary={available:count(s.available),empty:count(s.empty),pending:count(s.pending),stale:count(s.stale),attention:count(s.attention),unsupported:count(s.unsupported)}
 const computed={available:0,empty:0,pending:0,stale:0,attention:0,unsupported:0};for(const d of datasets){if(d.status==='failed'||d.status==='reconnect'||d.status==='setup')computed.attention++;else computed[d.status]++}
 if(JSON.stringify(summary)!==JSON.stringify(computed))throw Error('Inventory summary mismatch')
 return {...scope,source:'visibility_collection_inventory',asOf:stamp(r.asOf),quarter,datasets,summary}
}
export async function loadCollectionInventory(c:DiscoveryContext,quarter:string,signal?:AbortSignal):Promise<CollectionInventory>{
 validateDiscoveryContext(c);if(!/^(Q[1-4]|FY)$/.test(quarter))throw Error('Invalid inventory quarter')
 const response=await fetch(`${visibilityToolsContracts.collection.path}?${new URLSearchParams({websiteId:c.websiteId,site:c.site,quarter})}`,{credentials:'include',redirect:'error',headers:{Accept:'application/json'},signal})
 if(!response.ok)throw Error('collection_unavailable')
 return sanitizeCollectionInventory(object(await response.json()).data,c,quarter)
}
