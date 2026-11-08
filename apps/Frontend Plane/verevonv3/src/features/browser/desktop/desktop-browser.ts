export type DesktopBrowserBounds = { x: number; y: number; width: number; height: number }

export type DesktopBrowserAction =
  | { kind: 'navigate'; url: string }
  | { kind: 'snapshot' }
  | { kind: 'click'; target: string }
  | { kind: 'fill'; target: string; text: string }
  | { kind: 'press'; key: string }
  | { kind: 'scroll'; deltaY: number }

export type DesktopBrowserCommand = 'back' | 'forward' | 'reload' | 'stop'

export type DesktopBrowserSnapshot = {
  url: string
  title: string
  heading: string | null
  text: string
  targets: Array<{ id: string; role: string; label: string; href?: string; checked?: boolean }>
  loadError?: string
  status?: number
  canGoBack: boolean
  canGoForward: boolean
}

export type DesktopBrowserState = {
  url: string
  title: string
  isLoading: boolean
  canGoBack: boolean
  canGoForward: boolean
  loadError?: string
  status?: number
}

export type DesktopBrowserEvent =
  | ({ type: 'state' } & DesktopBrowserState)
  | { type: 'cursor'; x: number; y: number; action: string }
  | { type: 'handoff' }

export type DesktopBrowserBridge = {
  getState(): Promise<DesktopBrowserState>
  setViewport(bounds: DesktopBrowserBounds | null): Promise<void>
  navigate(url: string): Promise<DesktopBrowserSnapshot>
  command(command: DesktopBrowserCommand): Promise<DesktopBrowserState>
  reset(): Promise<void>
  startRun(): Promise<string>
  execute(runId: string, action: DesktopBrowserAction): Promise<DesktopBrowserSnapshot>
  endRun(runId: string): Promise<void>
  subscribe(listener: (event: DesktopBrowserEvent) => void): () => void
}

declare global {
  interface Window {
    verevonDesktopBrowser?: DesktopBrowserBridge
  }
}

export function getDesktopBrowser(): DesktopBrowserBridge | undefined {
  return typeof window === 'undefined' ? undefined : window.verevonDesktopBrowser
}
