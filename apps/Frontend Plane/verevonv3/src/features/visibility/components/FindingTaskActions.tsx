import { createEffect, createSignal, For, onCleanup, Show } from 'solid-js'
import { A } from '@solidjs/router'
import { useI18n } from '@/shared/i18n'
import type { Analysis } from '@/shared/api/visibility/tools-client'
import { saveMarketingAuditFinding } from '@/shared/api/marketing/marketing-work-client'
import { auditRuleLabel } from '@/features/marketing/pulse/audit-work-labels'
export function FindingTaskActions(props:{data:Analysis;page:string;brokenOnly?:boolean;canWrite?:boolean}){
 const {tr}=useI18n();const [busy,setBusy]=createSignal(false);const [saved,setSaved]=createSignal<string[]>([]);const [failed,setFailed]=createSignal(false);let controller:AbortController|undefined
 createEffect(()=>{void props.data.websiteId;void props.data.websiteRevision;void props.data.coverage.auditId;setSaved([]);setBusy(false);setFailed(false);controller?.abort()});onCleanup(()=>controller?.abort())
 const key=(raw:string)=>{const u=new URL(raw);return u.hostname.replace(/^www\./,'')+u.pathname+u.search}
 const findings=()=>props.data.findings?.filter(f=>key(f.pageUrl)===key(props.page)&&(!props.brokenOnly||f.ruleId==='broken_checked_link'))??[]
 const save=async(id:string)=>{if(!props.canWrite||busy())return;const c=new AbortController();controller=c;setBusy(true);setFailed(false);try{await saveMarketingAuditFinding({websiteId:props.data.websiteId,site:props.data.site,websiteRevision:props.data.websiteRevision,auditId:props.data.coverage.auditId,findingId:id},c.signal);if(!c.signal.aborted)setSaved(v=>[...v,id])}catch{if(!c.signal.aborted)setFailed(true)}finally{if(!c.signal.aborted)setBusy(false)}}
 return <Show when={findings().length}><div class="visibility-finding-actions"><For each={findings()}>{f=><div><span>{auditRuleLabel(f.ruleId,tr)}</span><Show when={props.canWrite}><button type="button" disabled={busy()||saved().includes(f.id)} onClick={()=>void save(f.id)}>{saved().includes(f.id)?tr('Lagret i oppgavelisten','Saved to backlog'):tr('Lagre funn som oppgave','Save finding as task')}</button></Show></div>}</For><Show when={failed()}><p role="alert">{tr('Oppgaven kunne ikke lagres. Last grunnlaget på nytt.','Could not save the task. Reload the evidence.')}</p></Show><A href="/engagement/visibility">{tr('Åpne oppgavelisten og kontrollhistorikken','Open backlog and verification history')} →</A></div></Show>
}
