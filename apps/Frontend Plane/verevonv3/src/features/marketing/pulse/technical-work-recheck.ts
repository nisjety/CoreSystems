import { loadPageSpeed, startPageSpeed } from '../../../shared/api/marketing/pagespeed-client.ts'
import { verifyMarketingTask, type MarketingTask, type MarketingWorkSnapshot } from '../../../shared/api/marketing/marketing-work-client.ts'

function pause(signal: AbortSignal): Promise<void> {
  return new Promise((resolve, reject) => {
    const abort = () => { clearTimeout(timer); reject(new Error('Audit cancelled')) }
    const timer = setTimeout(() => { signal.removeEventListener('abort', abort); resolve() }, 5000)
    if (signal.aborted) abort()
    else signal.addEventListener('abort', abort, { once: true })
  })
}

/** Called only by an explicit user action. A cached result may legitimately be
 * rejected as old by Marketing Core; never retry provider or verification POSTs. */
export async function recheckTechnicalTask(task: MarketingTask, websiteRevision: number, signal: AbortSignal): Promise<MarketingWorkSnapshot> {
  if (task.evidence.kind !== 'pagespeed_check' || signal.aborted) throw new Error('Invalid technical recheck')
  const controller = new AbortController()
  const abort = () => controller.abort()
  signal.addEventListener('abort', abort, { once: true })
  const timer = setTimeout(abort, 120_000)
  try {
    let state = await startPageSpeed(task.site, task.evidence.strategy, controller.signal, task.websiteId)
    while (state.status === 'running') {
      await pause(controller.signal)
      state = await loadPageSpeed(task.site, task.evidence.strategy, controller.signal, task.websiteId)
    }
    if (controller.signal.aborted || state.status !== 'complete') throw new Error('Audit not complete')
    return await verifyMarketingTask({ websiteId: task.websiteId, site: task.site, websiteRevision, taskId: task.id, revision: task.revision }, controller.signal)
  } finally {
    clearTimeout(timer); signal.removeEventListener('abort', abort)
  }
}
