// @vitest-environment jsdom
import { fireEvent, render, screen } from '@solidjs/testing-library'
import { createSignal, flush } from 'solid-js'
import { describe, expect, it, vi } from 'vitest'
import type { DashboardTab } from './dashboard-home-types'

vi.mock('./DashboardComposer', () => ({ DashboardComposer: (props: { message: string; onMessageChange: (value: string) => void }) => {
  const [attachment, setAttachment] = createSignal(false)
  return <><input aria-label="Draft" value={props.message} onInput={(event) => props.onMessageChange(event.currentTarget.value)} /><button onClick={() => setAttachment(true)}>Attach test file</button><span>{attachment() ? 'File retained' : 'No file'}</span></>
} }))
vi.mock('./DashboardSearchPanel', () => ({ SearchPanel: () => <p>Search tool</p> }))
vi.mock('./KnowledgeComposer', () => ({ KnowledgeComposer: () => <p>Crawl tool</p> }))
import { DashboardComposerPanel } from './DashboardHomeChrome'

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
