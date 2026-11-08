// @vitest-environment jsdom

import { cleanup, render } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { ChatToolCall } from '@/features/chat/components/chat-types'
import { ComputerViewer, chatComputerPanelEnabled } from './ComputerViewer'

vi.mock('@/shared/i18n', () => ({ useI18n: () => ({ tr: (_no: string, en: string) => en }) }))

afterEach(() => {
  cleanup()
  vi.unstubAllEnvs()
})

const shell = (fields: Partial<ChatToolCall> = {}): ChatToolCall => ({ id: 'call-1', name: 'shell', ...fields })

describe('chatComputerPanelEnabled', () => {
  it('is off unless the build sets VITE_CHAT_COMPUTER_PANEL to true', () => {
    expect(chatComputerPanelEnabled()).toBe(false)
    for (const value of ['false', '1', 'TRUE']) {
      vi.stubEnv('VITE_CHAT_COMPUTER_PANEL', value)
      expect(chatComputerPanelEnabled()).toBe(false)
    }
    vi.stubEnv('VITE_CHAT_COMPUTER_PANEL', 'true')
    expect(chatComputerPanelEnabled()).toBe(true)
  })
})

describe('TerminalViewer', () => {
  it('shows a command as a prompt line and its plain output as text', () => {
    const { container } = render(() => (
      <ComputerViewer call={shell({ args: { command: 'ls -la' }, output: 'total 4' })} intent="terminal" />
    ))

    expect(container.querySelector('[data-computer-viewer="terminal"]')).not.toBeNull()
    expect(container.querySelector('.verevon-computer-viewer__meta')?.textContent).toBe('$ ls -la')
    expect(container.querySelector('pre')?.textContent).toBe('total 4')
  })

  it('splits a code run into its stdout and stderr, with the exit code', () => {
    const output = JSON.stringify({ stdout: 'done\n', stderr: 'warn\n', exit_code: 2 })
    const { container } = render(() => (
      <ComputerViewer call={{ id: 'call-1', name: 'code_interpreter', output }} intent="terminal" />
    ))

    expect(container.querySelector('.verevon-computer-viewer__output')?.textContent).toBe('done\n')
    expect(container.querySelector('.verevon-computer-viewer__stderr')?.textContent).toBe('warn\n')
    expect(container.textContent).toContain('Exit code 2')
  })

  it('draws a process the same way as a command', () => {
    const { container } = render(() => (
      <ComputerViewer
        call={{ id: 'call-2', name: 'process_read', args: { process_id: 'proc_1' }, output: 'hello' }}
        intent="process"
      />
    ))

    expect(container.querySelector('[data-computer-viewer="terminal"]')).not.toBeNull()
    expect(container.querySelector('pre')?.textContent).toBe('hello')
  })

  it('shows output as text, never as markup', () => {
    const { container } = render(() => (
      <ComputerViewer call={shell({ output: '<img src=x onerror="window.__pwned=1">' })} intent="terminal" />
    ))

    expect(container.querySelector('img')).toBeNull()
    expect(container.querySelector('pre')?.textContent).toBe('<img src=x onerror="window.__pwned=1">')
  })

  it('says when the output was cut short, which is not the same as complete', () => {
    const { container } = render(() => (
      <ComputerViewer call={shell({ output: '{"stdout":"abc","stderr":"","truncated":true}' })} intent="terminal" />
    ))

    expect(container.textContent).toContain('The output was truncated')
  })
})

describe('FileViewer', () => {
  it('shows the path the call named, and the file text under it', () => {
    const { container } = render(() => (
      <ComputerViewer
        call={{ id: 'call-3', name: 'workspace_read', args: { path: 'notes/plan.md' }, output: '# Plan' }}
        intent="file"
      />
    ))

    expect(container.querySelector('[data-computer-viewer="file"]')).not.toBeNull()
    expect(container.querySelector('.verevon-computer-viewer__meta')?.textContent).toBe('notes/plan.md')
    expect(container.querySelector('pre')?.textContent).toBe('# Plan')
  })

  it('says when the file was cut short', () => {
    const { container } = render(() => (
      <ComputerViewer
        call={{ id: 'call-3', name: 'workspace_read', args: { path: 'big.log' }, output: '{"truncated":true}' }}
        intent="file"
      />
    ))

    expect(container.textContent).toContain('The file was truncated')
  })

  it('shows file text as text, never as markup', () => {
    const { container } = render(() => (
      <ComputerViewer
        call={{ id: 'call-3', name: 'workspace_read', args: { path: 'page.html' }, output: '<script>alert(1)</script>' }}
        intent="file"
      />
    ))

    expect(container.querySelector('script')).toBeNull()
    expect(container.querySelector('pre')?.textContent).toBe('<script>alert(1)</script>')
  })
})

describe('ComputerViewer', () => {
  it('draws nothing for an intent the registry does not claim, such as a browser step', () => {
    const { container } = render(() => (
      <ComputerViewer call={{ id: 'call-4', name: 'browser.open', output: 'page' }} intent="browser" />
    ))

    expect(container.querySelector('[data-computer-viewer]')).toBeNull()
    expect(container.querySelector('.verevon-computer-viewer')).toBeNull()
  })
})
