export type DesktopBrowserBounds = { x: number; y: number; width: number; height: number }

export type DesktopBrowserAction =
  | { kind: 'navigate'; url: string }
  | { kind: 'snapshot' }
  | { kind: 'click'; target: string }
  | { kind: 'fill'; target: string; text: string }
  | { kind: 'press'; key: string }
  | { kind: 'scroll'; deltaY: number }

export type DesktopBrowserCommand = 'back' | 'forward' | 'reload' | 'stop'

/** One cookie banner the desktop browser dismissed with a reject control (D-W5-4). The host only, never a path or query. */
export type DesktopConsentReceipt = { kind: 'consent_reject'; host: string; at: string }

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
  /** Dismissals since the previous snapshot, as the Quarry adapter returns them in `receipts`. Absent means none. */
  receipts?: DesktopConsentReceipt[]
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
  | { type: 'receipt'; kind: 'consent_reject'; host: string; at: string }

export type DesktopBrowserBridge = {
  getState(): Promise<DesktopBrowserState>
  setViewport(bounds: DesktopBrowserBounds | null): Promise<void>
  navigate(url: string): Promise<DesktopBrowserSnapshot>
  command(command: DesktopBrowserCommand): Promise<DesktopBrowserState>
  reset(): Promise<void>
  /** Opens a run. `consentReject: true` opts this run in to reject-only cookie dismissal; anything else leaves it off. */
  startRun(options?: { consentReject?: boolean }): Promise<string>
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
