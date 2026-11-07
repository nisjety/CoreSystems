import { describe, expect, it, vi } from 'vitest'
import { isAssistantTaskBusy, submitAssistantTask } from './assistant-task-routing'

function fixture(browserStatus: 'idle' | 'starting' | 'disconnected' = 'idle', navigationStatus: 'idle' | 'thinking' = 'idle', guiding = false) {
  return {
    browserStatus: () => browserStatus, navigationStatus: () => navigationStatus, guiding: () => guiding,
    resetNavigation: vi.fn(), sendBrowser: vi.fn(async () => {}), sendNavigation: vi.fn(async () => {}),
    openChatDraft: vi.fn(() => true),
    tr: (_no: string, en: string) => en,
  }
}

describe('assistant task routing', () => {
  it('sends an explicit URL to the real browser controller', async () => {
    const f = fixture()
    await submitAssistantTask('Inspect https://example.com/report.', f)
    expect(f.sendBrowser).toHaveBeenCalledWith('Inspect https://example.com/report.', 'https://example.com/report')
    expect(f.sendNavigation).not.toHaveBeenCalled()
  })

  it.each([
    ['open aquatiq.com in Nett, click a link and tell me what it found', 'https://aquatiq.com'],
    ['Åpne (aquatiq.com/no/om-oss), og oppsummer siden.', 'https://aquatiq.com/no/om-oss'],
  ])('runs a browser task with a bare domain in %s', async (prompt, target) => {
    const f = fixture()
    await submitAssistantTask(prompt, f)
    expect(f.sendBrowser).toHaveBeenCalledWith(prompt, target)
    expect(f.sendNavigation).not.toHaveBeenCalled()
  })

  it('leaves email addresses out of browser routing', async () => {
    const f = fixture()
    const prompt = 'Find the contact for person@aquatiq.com.'
    await submitAssistantTask(prompt, f)
    expect(f.sendNavigation).toHaveBeenCalledWith(prompt)
    expect(f.sendBrowser).not.toHaveBeenCalled()
  })

  it.each([
    'Compare aquatiq.com and example.com.',
    'Compare aquatiq.com and https://example.com.',
  ])('asks for one domain when the prompt names multiple hosts in %s', async (prompt) => {
    const f = fixture()
    await expect(submitAssistantTask(prompt, f)).rejects.toThrow('Start one task per domain.')
    expect(f.sendBrowser).not.toHaveBeenCalled()
    expect(f.sendNavigation).not.toHaveBeenCalled()
  })

  it('launches the first address and passes through all same-domain links and duplicate mentions', async () => {
    const f = fixture()
    const prompt = 'Open aquatiq.com, go to https://aquatiq.com/no/about-us, then compare https://AQUATIQ.COM with that page.'
    await submitAssistantTask(prompt, f)
    expect(f.sendBrowser).toHaveBeenCalledExactlyOnceWith(prompt, 'https://aquatiq.com')
    expect(f.sendNavigation).not.toHaveBeenCalled()
  })

  it('sends a request without a URL to the task planner', async () => {
    const f = fixture()
    await submitAssistantTask('Open knowledge', f)
    expect(f.sendNavigation).toHaveBeenCalledWith('Open knowledge')
    expect(f.sendBrowser).not.toHaveBeenCalled()
  })

  it('routes an explicit Chat draft locally even when the draft contains a web address', async () => {
    const f = fixture()
    await submitAssistantTask('Kan du åpne Chat og skrive at jeg fant https://example.com?', f)
    expect(f.openChatDraft).toHaveBeenCalledOnce()
    expect(f.sendBrowser).not.toHaveBeenCalled()
    expect(f.sendNavigation).not.toHaveBeenCalled()
  })

  it.each(['starting', 'disconnected'] as const)('never resets an %s run from the other entry point', async (status) => {
    const f = fixture(status)
    await expect(submitAssistantTask('Open knowledge', f)).rejects.toThrow('already running')
    expect(f.sendNavigation).not.toHaveBeenCalled()
    expect(isAssistantTaskBusy(status, 'idle', false)).toBe(true)
  })

  it('also protects an active navigation model call or guide', async () => {
    for (const f of [fixture('idle', 'thinking'), fixture('idle', 'idle', true)]) {
      await expect(submitAssistantTask('Inspect https://example.com', f)).rejects.toThrow('already running')
      expect(f.resetNavigation).not.toHaveBeenCalled()
      expect(f.sendBrowser).not.toHaveBeenCalled()
    }
  })
})
