import { Show, createEffect, createSignal, onCleanup } from 'solid-js'
import { useI18n } from '@/shared/i18n'
import { loadAlgoliaCollection, queueAlgoliaCollection, type AlgoliaCollection } from '@/shared/api/marketing/algolia-collection-client'
import type { DiscoveryContext } from '@/shared/api/marketing/discovery-client'
import type { SiteSourceReport } from '@/shared/api/marketing/site-sources-client'
type Props={websiteId:string;site:string;revision:number;canManageIntegrations?:boolean;refreshKey?:number;snapshot?:SiteSourceReport|null;onReportChanged?:()=>void}
export function AlgoliaCollectionPanel(props:Props) {
  const i18n=useI18n(),[receipt,setReceipt]=createSignal<AlgoliaCollection|null>(null),[loading,setLoading]=createSignal(true),[queuing,setQueuing]=createSignal(false),[error,setError]=createSignal(''),[stopped,setStopped]=createSignal(false),[refresh,setRefresh]=createSignal(0)
  const allowed=()=>props.canManageIntegrations??false
  const context=():DiscoveryContext=>({websiteId:props.websiteId,site:props.site,websiteRevision:props.revision})
  let controller:AbortController|undefined,timer:ReturnType<typeof setTimeout>|undefined,generation=0,attempts=0
  async function read(current:AbortController,version:number,queue=false) {
    try {
      const value=await(queue?queueAlgoliaCollection:loadAlgoliaCollection)(context(),current.signal)
      if(current.signal.aborted||version!==generation||queue&&!allowed())return
      setReceipt(value);setError('');clearTimeout(timer)
      if(['queued','syncing'].includes(value.status)) {
        if(++attempts<=60)timer=setTimeout(()=>void read(current,version),2000)
        else setStopped(true)
      } else props.onReportChanged?.()
    } catch {if(!current.signal.aborted&&version===generation)setError(i18n.tr('Innsamlingsstatus kunne ikke leses. Oppdater status før et nytt forsøk.','Collection status could not be read. Refresh status before retrying.'))}
    finally{if(!current.signal.aborted&&version===generation){setLoading(false);setQueuing(false)}}
  }
  createEffect(()=>{
    context();refresh();void props.refreshKey;allowed();const version=++generation;controller?.abort();clearTimeout(timer);attempts=0
    const current=new AbortController();controller=current;setReceipt(null);setError('');setStopped(false);setLoading(true);setQueuing(false);void read(current,version)
    onCleanup(()=>{current.abort();clearTimeout(timer)})
  })
  const queue=()=>{if(!allowed()||loading()||queuing()||!controller)return;attempts=0;setStopped(false);setQueuing(true);void read(controller,generation,true)}
  const empty=()=>{const data=props.snapshot?.algolia;if(!data)return false;const lists=[data.topSearches,data.noResultTerms].filter(value=>value?.status==='connected');return(data.analytics.status==='connected'||lists.length>0)&&(data.analytics.status!=='connected'||data.analytics.searches===0)&&lists.every(value=>value?.rows?.length===0)}
  const label=()=>receipt()?.status==='ready'&&empty()?i18n.tr('Tilgjengelig rapport er tom','Available report is empty'):({idle:i18n.tr('Ikke synkronisert ennå','Not synced yet'),queued:i18n.tr('I kø','Queued'),syncing:i18n.tr('Synkroniserer','Syncing'),ready:i18n.tr('Klar','Ready'),needs_attention:i18n.tr('Trenger oppfølging','Needs attention'),reconnect_required:i18n.tr('Koble kontoen til på nytt','Reconnect account')}[receipt()?.status??'idle'])
  return <section class="algolia-connection" data-testid="algolia-collection" aria-busy={loading()||queuing()}><header><h2>{i18n.tr('Bakgrunnsinnsamling','Background collection')}</h2><div class="algolia-connection__actions"><button type="button" disabled={loading()||queuing()} onClick={()=>setRefresh(n=>n+1)}>{i18n.tr('Oppdater status','Refresh status')}</button><Show when={allowed()}><button type="button" disabled={loading()||queuing()||receipt()?.status==='queued'||receipt()?.status==='syncing'} onClick={queue}>{i18n.tr('Oppdater data','Update data')}</button></Show></div></header><Show when={loading()}><p role="status">{i18n.tr('Leser lagret innsamlingsstatus…','Reading saved collection status…')}</p></Show><Show when={error()}><p role="alert">{error()}</p></Show><Show when={receipt()}><p role="status">{label()}</p><Show when={receipt()?.reason}><p>{i18n.tr('Årsak','Reason')}: {receipt()?.reason}</p></Show><Show when={receipt()?.nextRunAt}><p>{i18n.tr('Neste forsøk','Next attempt')}: <time>{receipt()?.nextRunAt}</time></p></Show><Show when={receipt()?.lastAttemptAt}><p>{i18n.tr('Siste forsøk','Last attempt')}: <time>{receipt()?.lastAttemptAt}</time></p></Show><Show when={receipt()?.lastSuccessAt}><p>{i18n.tr('Siste vellykkede rapport','Last successful report')}: <time>{receipt()?.lastSuccessAt}</time></p></Show></Show><Show when={stopped()}><p>{i18n.tr('Statuskontroll er satt på pause. Jobben fortsetter på serveren; oppdater status senere.','Status checking is paused. The job continues on the server; refresh status later.')}</p></Show><p>{i18n.tr('Visningen leser lagrede rapporter. Lagring av en nettstedskobling starter første innsamling på serveren; senere oppdateringer krever ikke nytt leverandørsamtykke så lenge tilgangen er gyldig.','This view reads cached reports. Saving a site connection starts initial collection on the server; later updates need no new provider consent while access remains valid.')}</p></section>
}
