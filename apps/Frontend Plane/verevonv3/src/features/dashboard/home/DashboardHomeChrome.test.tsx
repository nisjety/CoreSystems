// @vitest-environment jsdom
import { fireEvent, render, screen } from '@solidjs/testing-library'
import { createSignal, flush } from 'solid-js'
import { describe, expect, it, vi } from 'vitest'
import type { DashboardTab } from './dashboard-home-types'

const searchPanel = vi.hoisted(() => ({ mounts: 0, unmounts: 0, remember: undefined as boolean | undefined }))

vi.mock('@/shared/i18n', () => ({ useI18n: () => ({ tr: (_no: string, en: string) => en, locale: () => 'en' }) }))
vi.mock('./DashboardComposer', () => ({ DashboardComposer: (props: { message: string; onMessageChange: (value: string) => void }) => {
  const [attachment, setAttachment] = createSignal(false)
  return <><input aria-label="Draft" value={props.message} onInput={(event) => props.onMessageChange(event.currentTarget.value)} /><button onClick={() => setAttachment(true)}>Attach test file</button><span>{attachment() ? 'File retained' : 'No file'}</span></>
} }))
vi.mock('./DashboardSearchPanel', async () => {
  const { createEffect, onCleanup } = await import('solid-js')
  return { SearchPanel: (props: { rememberSearches?: boolean }) => {
    searchPanel.mounts += 1
    createEffect(() => props.rememberSearches, (remember) => { searchPanel.remember = remember })
    onCleanup(() => { searchPanel.unmounts += 1 })
    return <p>Search tool</p>
  } }
})
vi.mock('./KnowledgeComposer', () => ({ KnowledgeComposer: () => <p>Crawl tool</p> }))
import { DashboardComposerPanel, DashboardTabs } from './DashboardHomeChrome'

describe('Home tool switching', () => {
  it('keeps the chat draft and attachment state through Search, Crawl and layout comparison', () => {
    const [tab, setTab] = createSignal<DashboardTab>('Chat')
    const [message, setMessage] = createSignal('')
    const [preview, setPreview] = createSignal(true)
    const noop = () => undefined
    render(() => <DashboardComposerPanel activeTab={tab()} persistentChat={preview()} message={message()} onMessageChange={setMessage} onKnowledgePreviewActiveChange={noop} onLaunchStart={noop} onSearchExpandedChange={noop} onSearchPreviewActiveChange={noop} previewCollapsed={false} searchExpanded={false} />)
    const draft = screen.getByLabelText('Draft')
    fireEvent.input(draft, { target: { value: 'Keep this draft' } })
    fireEvent.click(screen.getByRole('button', { name: 'Attach test file' }))
    flush()
    setTab('Søk'); flush()
    expect(screen.getByText('Search tool')).toBeTruthy()
    expect(draft.closest('[hidden]')).toBeTruthy()
    setTab('Crawl'); flush()
    expect(screen.getByText('Crawl tool')).toBeTruthy()
    setTab('Chat'); flush()
    setPreview(false); flush()
    expect(screen.getByLabelText('Draft')).toBe(draft)
    expect((draft as HTMLInputElement).value).toBe('Keep this draft')
    expect(screen.getByText('File retained')).toBeTruthy()
  })
})

describe('Search panel reset key', () => {
  it('remounts the search panel when the key changes, and only then', () => {
    searchPanel.mounts = 0
    searchPanel.unmounts = 0
    const [resetKey, setResetKey] = createSignal(0)
    const [collapsed, setCollapsed] = createSignal(false)
    const [message, setMessage] = createSignal('')
    const noop = () => undefined
    render(() => <DashboardComposerPanel activeTab="Søk" persistentChat message={message()} onMessageChange={setMessage} onKnowledgePreviewActiveChange={noop} onLaunchStart={noop} onSearchExpandedChange={noop} onSearchPreviewActiveChange={noop} previewCollapsed={collapsed()} searchResetKey={resetKey()} searchExpanded={false} />)
    expect(searchPanel.mounts).toBe(1)
    const draft = screen.getByLabelText('Draft')
    fireEvent.input(draft, { target: { value: 'Keep this draft' } })
    setCollapsed(true); flush()
    expect(searchPanel.mounts).toBe(1)
    setResetKey(1); flush()
    expect(searchPanel.mounts).toBe(2)
    expect(searchPanel.unmounts).toBe(1)
    expect(screen.getAllByText('Search tool')).toHaveLength(1)
    // The chat composer next to it is not rebuilt and keeps its draft.
    expect(screen.getByLabelText('Draft')).toBe(draft)
    expect((draft as HTMLInputElement).value).toBe('Keep this draft')
    setResetKey(1); flush()
    expect(searchPanel.mounts).toBe(2)
  })
})

describe('Search panel history gate', () => {
  const panel = (remember: boolean | undefined) => {
    const [message, setMessage] = createSignal('')
    const noop = () => undefined
    return render(() => <DashboardComposerPanel activeTab="Søk" persistentChat message={message()} onMessageChange={setMessage} onKnowledgePreviewActiveChange={noop} onLaunchStart={noop} onSearchExpandedChange={noop} onSearchPreviewActiveChange={noop} previewCollapsed={false} rememberSearches={remember} searchExpanded={false} />)
  }

  it('hands the search panel exactly what Home decided about remembering searches', () => {
    for (const remember of [true, false, undefined]) {
      searchPanel.remember = remember === undefined ? true : undefined
      const view = panel(remember)
      flush()
      expect(searchPanel.remember).toBe(remember)
      view.unmount()
    }
  })

  it('follows a change without remounting the panel', () => {
    searchPanel.mounts = 0
    const [remember, setRemember] = createSignal<boolean | undefined>(undefined)
    const [message, setMessage] = createSignal('')
    const noop = () => undefined
    render(() => <DashboardComposerPanel activeTab="Søk" persistentChat message={message()} onMessageChange={setMessage} onKnowledgePreviewActiveChange={noop} onLaunchStart={noop} onSearchExpandedChange={noop} onSearchPreviewActiveChange={noop} previewCollapsed={false} rememberSearches={remember()} searchExpanded={false} />)
    expect(searchPanel.remember).toBeUndefined()
    setRemember(true); flush()
    expect(searchPanel.remember).toBe(true)
    setRemember(false); flush()
    expect(searchPanel.remember).toBe(false)
    expect(searchPanel.mounts).toBe(1)
  })
})

describe('Home tab double-click', () => {
  const tabButton = (name: 'Chat' | 'Web' | 'Extract') => screen.getByRole('button', { name })
  // What a browser sends for a double-click: click, click, dblclick.
  const doubleClick = (button: HTMLElement) => {
    fireEvent.click(button, { detail: 1 }); flush()
    fireEvent.click(button, { detail: 2 }); flush()
    fireEvent.dblClick(button, { detail: 2 }); flush()
  }
  const setup = (initial: DashboardTab = 'Chat', withHandler = true) => {
    const [active, setActive] = createSignal<DashboardTab>(initial)
    const onTabChange = vi.fn((next: DashboardTab) => setActive(next))
    const onActiveTabDoubleClick = vi.fn()
    render(() => <DashboardTabs activeTab={active()} onTabChange={onTabChange} onActiveTabDoubleClick={withHandler ? onActiveTabDoubleClick : undefined} />)
    return { active, onActiveTabDoubleClick, onTabChange }
  }

  it('reports a double-click on the tab that was already active', () => {
    const { onActiveTabDoubleClick, onTabChange } = setup('Søk')
    doubleClick(tabButton('Web'))
    expect(onActiveTabDoubleClick).toHaveBeenCalledTimes(1)
    expect(onActiveTabDoubleClick).toHaveBeenCalledWith('Søk')
    expect(onTabChange).toHaveBeenCalledTimes(2)
  })

  it('only switches when the double-clicked tab was not active', () => {
    const { active, onActiveTabDoubleClick, onTabChange } = setup('Chat')
    doubleClick(tabButton('Web'))
    expect(active()).toBe('Søk')
    expect(onTabChange).toHaveBeenCalledWith('Søk')
    expect(onActiveTabDoubleClick).not.toHaveBeenCalled()
    // The very next double-click is on an active tab and does count.
    doubleClick(tabButton('Web'))
    expect(onActiveTabDoubleClick).toHaveBeenCalledTimes(1)
    expect(onActiveTabDoubleClick).toHaveBeenCalledWith('Søk')
  })

  it('does not report a double-click on a different tab than the active one', () => {
    const { active, onActiveTabDoubleClick } = setup('Søk')
    doubleClick(tabButton('Extract'))
    expect(active()).toBe('Crawl')
    expect(onActiveTabDoubleClick).not.toHaveBeenCalled()
    doubleClick(tabButton('Chat'))
    expect(active()).toBe('Chat')
    expect(onActiveTabDoubleClick).not.toHaveBeenCalled()
  })

  it('names the tab for each active tab, so Home can decide what to reset', () => {
    const { onActiveTabDoubleClick } = setup('Crawl')
    doubleClick(tabButton('Extract'))
    expect(onActiveTabDoubleClick).toHaveBeenCalledWith('Crawl')
  })

  it('reads a lone dblclick (no click before it) from the tab state at that moment', () => {
    const { onActiveTabDoubleClick, onTabChange } = setup('Søk')
    fireEvent.dblClick(tabButton('Extract')); flush()
    expect(onActiveTabDoubleClick).not.toHaveBeenCalled()
    fireEvent.dblClick(tabButton('Web')); flush()
    expect(onActiveTabDoubleClick).toHaveBeenCalledTimes(1)
    expect(onTabChange).not.toHaveBeenCalled()
  })

  it('does not treat two slow clicks as a double-click', () => {
    const { onActiveTabDoubleClick } = setup('Søk')
    fireEvent.click(tabButton('Web'), { detail: 1 }); flush()
    fireEvent.click(tabButton('Web'), { detail: 1 }); flush()
    expect(onActiveTabDoubleClick).not.toHaveBeenCalled()
  })

  it('does nothing extra when no handler is given', () => {
    const { active, onTabChange } = setup('Søk', false)
    expect(() => doubleClick(tabButton('Web'))).not.toThrow()
    expect(() => doubleClick(tabButton('Extract'))).not.toThrow()
    expect(active()).toBe('Crawl')
    expect(onTabChange).toHaveBeenCalledTimes(4)
  })
})
