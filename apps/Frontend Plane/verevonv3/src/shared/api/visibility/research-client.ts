import { visibilityToolsContracts } from '@/shared/actions/visibility-tools'
import { validateDiscoveryContext, isPublicSitePage, type DiscoveryContext } from '../marketing/discovery-client'
export type GroupRule={id:string;name:string;entity:'query'|'page';contains:string;minImpressions:number;maxPosition:number}
export type Aggregate={key:string;clicks:number;impressions:number;position:number;days:number}
export type Observation={date:string;query:string;page:string;clicks:number;impressions:number;position:number}
export type Research=DiscoveryContext & {source:'gsc_date_query_page';status:'connected'|'unavailable';reason?:string;period:{startDate:string;endDate:string};observedAt:string;lastAttemptAt:string;nextRunAt:string;stale:boolean;truncated:boolean;partial:true;rows:Observation[];queries:Aggregate[];pages:Aggregate[];weeks:Aggregate[];relationships:{query:string;pages:Aggregate[];overlapDays:number;candidate:boolean}[];groups:{rule:GroupRule;items:Aggregate[];total:number;truncated:boolean}[];rules:GroupRule[]}
const obj=(v:unknown):Record<string,unknown>=>{if(!v||typeof v!=='object'||Array.isArray(v))throw Error('Invalid research object');return v as Record<string,unknown>}
const text=(v:unknown,max=2048)=>{if(typeof v!=='string'||v.length>max||/[\x00-\x1f\x7f]/.test(v))throw Error('Invalid research text');return v}
const n=(v:unknown,max=Number.MAX_SAFE_INTEGER,whole=false)=>{if(typeof v!=='number'||!Number.isFinite(v)||v<0||v>max||whole&&!Number.isSafeInteger(v))throw Error('Invalid research number');return v}
const flag=(v:unknown)=>{if(typeof v!=='boolean')throw Error('Invalid research flag');return v}
const list=(v:unknown,max:number)=>{if(!Array.isArray(v)||v.length>max)throw Error('Invalid research list');return v as unknown[]}
const day=(v:unknown,empty=false)=>{const s=text(v,10);if(empty&&s==='')return s;if(!/^\d{4}-\d\d-\d\d$/.test(s)||!Number.isFinite(Date.parse(s+'T00:00:00Z'))||new Date(s+'T00:00:00Z').toISOString().slice(0,10)!==s)throw Error('Invalid research day');return s}
const stamp=(v:unknown)=>{const s=text(v,40);if(s&&(!Number.isFinite(Date.parse(s))||!/^\d{4}-\d\d-\d\dT/.test(s)))throw Error('Invalid research time');return s}
export function sanitizeGroupRule(v:unknown):GroupRule{const r=obj(v);const id=text(r.id,40),entity=text(r.entity,5);if(!/^[a-z][a-z0-9_-]{0,39}$/.test(id)||!['query','page'].includes(entity)||!text(r.name,100))throw Error('Invalid rule');return {id,name:text(r.name,100),entity:entity as GroupRule['entity'],contains:text(r.contains,200),minImpressions:n(r.minImpressions,1e9,true),maxPosition:n(r.maxPosition,1000)}}
function aggregate(v:unknown):Aggregate{const r=obj(v),clicks=n(r.clicks,Number.MAX_SAFE_INTEGER,true),impressions=n(r.impressions,Number.MAX_SAFE_INTEGER,true);if(clicks>impressions)throw Error('Invalid counts');return {key:text(r.key),clicks,impressions,position:n(r.position,1e6),days:n(r.days,90,true)}}
export function sanitizeResearch(v:unknown,expected:DiscoveryContext):Research{
 const r=obj(v),c=validateDiscoveryContext(r),p=obj(r.period);if(c.websiteId!==expected.websiteId||c.site!==expected.site||c.websiteRevision!==expected.websiteRevision||r.source!=='gsc_date_query_page'||!['connected','unavailable'].includes(String(r.status))||r.partial!==true||r.reason!==undefined&&!['source_unavailable','authorization_required','storage_unavailable'].includes(String(r.reason)))throw Error('Unexpected research context')
 const startDate=day(p.startDate,true),endDate=day(p.endDate,true);if(r.status==='connected'&&(!startDate||!endDate||endDate<startDate||Date.parse(endDate)-Date.parse(startDate)>90*86400000))throw Error('Invalid research period')
 const rows=list(r.rows,5000).map(v=>{const x=obj(v),date=day(x.date),page=text(x.page),query=text(x.query,512);if(!query||!isPublicSitePage(c.site,page)||date<startDate||date>endDate)throw Error('Invalid research scope');const a=aggregate({...x,key:query,days:1});return {date,query,page,clicks:a.clicks,impressions:a.impressions,position:a.position}})
 if(new Set(rows.map(x=>`${x.date}\0${x.query}\0${x.page}`)).size!==rows.length||r.status==='unavailable'&&rows.length)throw Error('Invalid research evidence')
 const queries=list(r.queries,5000).map(aggregate),pages=list(r.pages,5000).map(aggregate);if(pages.some(p=>!isPublicSitePage(c.site,p.key)))throw Error('Invalid research page')
 const relationships=list(r.relationships,5000).map(v=>{const x=obj(v),pages=list(x.pages,5000).map(aggregate);if(pages.some(p=>!isPublicSitePage(c.site,p.key)))throw Error('Invalid relationship');return {query:text(x.query,512),pages,overlapDays:n(x.overlapDays,90,true),candidate:flag(x.candidate)}})
 const rules=list(r.rules,20).map(sanitizeGroupRule),groups=list(r.groups,20).map(v=>{const x=obj(v),parsed=sanitizeGroupRule(x.rule),items=list(x.items,250).map(aggregate);if(parsed.entity==='page'&&items.some(p=>!isPublicSitePage(c.site,p.key)))throw Error('Invalid group scope');const total=n(x.total,5000,true),truncated=flag(x.truncated);if(items.length>total||truncated!==(total>items.length))throw Error('Invalid group coverage');return {rule:parsed,items,total,truncated}})
 return {...c,source:'gsc_date_query_page',status:r.status as Research['status'],...(r.reason?{reason:r.reason as string}:{}),period:{startDate,endDate},observedAt:stamp(r.observedAt),lastAttemptAt:stamp(r.lastAttemptAt),nextRunAt:stamp(r.nextRunAt),stale:flag(r.stale),truncated:flag(r.truncated),partial:true,rows,queries,pages,weeks:list(r.weeks,15).map(aggregate),relationships,rules,groups}
}
async function request(c:DiscoveryContext,signal:AbortSignal|undefined,operation?:'refresh'|'save_rule'|'remove_rule',r?:GroupRule){validateDiscoveryContext(c);const path=operation&&operation!=='refresh'?visibilityToolsContracts.researchRules.path:visibilityToolsContracts.research.path;const response=await fetch(operation?path:`${path}?${new URLSearchParams({websiteId:c.websiteId,site:c.site})}`,{method:operation?'POST':'GET',credentials:'include',redirect:'error',signal,headers:{Accept:'application/json',...(operation?{'Content-Type':'application/json'}:{})},...(operation?{body:JSON.stringify({...c,...(r?{operation,ruleId:r.id,name:r.name,entity:r.entity,contains:r.contains,minImpressions:r.minImpressions,maxPosition:r.maxPosition}:{})})}:{})});if(!response.ok)throw Error(response.status===429?'capacity':response.status===409?'context_changed':'research_unavailable');return sanitizeResearch(obj(await response.json()).data,c)}
export const loadResearch=(c:DiscoveryContext,signal?:AbortSignal)=>request(c,signal)
export const refreshResearch=(c:DiscoveryContext,signal?:AbortSignal)=>request(c,signal,'refresh')
export const saveResearchRule=(c:DiscoveryContext,r:GroupRule,remove=false,signal?:AbortSignal)=>request(c,signal,remove?'remove_rule':'save_rule',r)

// Poll saved state only after an explicit sync. These reads never start a new
// provider collection, and navigating to another workspace cancels the wait.
export async function waitForResearchRefresh(c:DiscoveryContext, queued:Research, signal:AbortSignal):Promise<Research|null> {
 for(let attempt=0;attempt<35;attempt++) {
  await new Promise<void>((resolve,reject)=>{
   if(signal.aborted){reject(new DOMException('Aborted','AbortError'));return}
   const abort=()=>{clearTimeout(timer);reject(new DOMException('Aborted','AbortError'))}
   const timer=setTimeout(()=>{signal.removeEventListener('abort',abort);resolve()},2000)
   signal.addEventListener('abort',abort,{once:true})
  })
  const current=await loadResearch(c,signal)
  if(current.lastAttemptAt!==queued.lastAttemptAt||current.observedAt!==queued.observedAt||current.reason==='authorization_required')return current
 }
 return null
}
