import type { DashboardComposerSubmitPayload } from '@/features/dashboard/home/DashboardComposer'
import type { AssistantAgentView } from '@/features/core/components/GlobalAssistant'
import type { AssistantNavigationAgentState } from './assistant-navigation-agent'
import { assistantBrowserTarget } from './assistant-browser-agent'
import { resolveAssistantChatDraft, type AssistantChatDraftAction } from '@/shared/actions/assistant-chat-draft'

type Translate = (no: string, en: string) => string

const bareBrowserAddress = /^(?:[\p{L}\d](?:[\p{L}\d-]*[\p{L}\d])?\.)+(?:\p{L}{2,}|xn--[a-z\d-]+)(?::\d+)?(?:[/?#]\S*)?$/iu

function assistantBrowserTargets(text: string): string[] {
  const targets: { address: string; index: number }[] = []
  const trimEnd = (value: string) => value.replace(/[.,!?;:)\]}]+$/, '')
  const remaining = text.replace(/https?:\/\/[^\s<>"'`]+/gi, (url, index: number) => {
    targets.push({ address: trimEnd(url), index })
    return ' '.repeat(url.length)
  })
  for (const token of remaining.matchAll(/[^\s<>"'`]+/g)) {
    const address = trimEnd(token[0].replace(/^[(\[{]+/, ''))
    if (bareBrowserAddress.test(address)) targets.push({ address: `https://${address}`, index: token.index })
  }
  return targets.sort((a, b) => a.index - b.index).map(({ address }) => address)
}

export function isAssistantTaskBusy(browser: AssistantAgentView['status'], navigation: AssistantNavigationAgentState['status'], guiding: boolean): boolean {
  return ['starting', 'running', 'stopping', 'disconnected'].includes(browser) || navigation === 'thinking' || guiding
}

/** Both entry points share one router; a running or unconfirmed task is never reset. */
export async function submitAssistantTask(payload: DashboardComposerSubmitPayload | string, options: {
  browserStatus: () => AssistantAgentView['status']
  navigationStatus: () => AssistantNavigationAgentState['status']
  guiding: () => boolean
  resetNavigation: () => void
  sendBrowser: (goal: string, target: string) => Promise<void>
  sendNavigation: (goal: string) => Promise<void>
  openChatDraft: (action: AssistantChatDraftAction) => boolean
  tr: Translate
}): Promise<void> {
  const { tr } = options
  if (typeof payload !== 'string' && (payload.attachments.length || payload.actions.length || payload.tools.length || payload.agentMode || payload.browserContext || payload.connectorScope?.length || (payload.retrievalScope && payload.retrievalScope !== 'auto'))) {
    throw new Error(tr('Denne assistenten kan foreløpig bare finne og åpne sider. Utkastet er beholdt.', 'This assistant can currently only find and open pages. Your draft has been kept.'))
  }
  if (isAssistantTaskBusy(options.browserStatus(), options.navigationStatus(), options.guiding())) throw new Error(tr('En oppgave er allerede i gang.', 'A task is already running.'))
  const text = typeof payload === 'string' ? payload : payload.text
  const chatDraft = resolveAssistantChatDraft(text)
  if (chatDraft) {
    if (!options.openChatDraft(chatDraft)) {
      throw new Error(tr('Kunne ikke åpne utkastet i Chat. Teksten er beholdt her.', 'Could not open the draft in Chat. Your text is still here.'))
    }
    options.resetNavigation()
    return
  }
  const [target, ...otherTargets] = assistantBrowserTargets(text)
  if (target) {
    const hosts = new Set<string>()
    for (const address of [target, ...otherTargets]) {
      const url = assistantBrowserTarget(address)
      if (!url) throw new Error(tr('Oppgi en gyldig nettadresse.', 'Provide a valid web address.'))
      hosts.add(url.hostname)
    }
    if (hosts.size > 1) throw new Error(tr('Start én oppgave per domene. Du kan bruke flere adresser på samme domene.', 'Start one task per domain. You can include multiple addresses on the same domain.'))
    options.resetNavigation()
    await options.sendBrowser(text, target)
    return
  }
  await options.sendNavigation(text)
}
