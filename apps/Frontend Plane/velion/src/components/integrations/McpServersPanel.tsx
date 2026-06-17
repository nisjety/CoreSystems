'use client'

import { useCallback, useEffect, useReducer, useRef, useState } from 'react'
import {
  AlertCircle,
  CheckCircle2,
  ChevronDown,
  Loader2,
  Plus,
  Terminal,
  Trash2,
  Wifi,
  X,
} from 'lucide-react'

// ── Types ────────────────────────────────────────────────────────────────────

type McpTransport = 'stdio' | 'http' | 'sse' | 'ws'

interface McpServer {
  id: string
  name: string
  transport: McpTransport
  command: string | null
  args: string[]
  env: Record<string, string>
  url: string | null
  headers: Record<string, string>
  enabled: boolean
  created_at: string
  updated_at: string
}

interface TestResult {
  connected: boolean
  tools_count: number
  error?: string | null
}

interface FormState {
  name: string
  transport: McpTransport
  command: string
  args: string        // space-separated
  url: string
  headersRaw: string  // "Key: Value" lines
}

const FORM_DEFAULTS: FormState = {
  name: '',
  transport: 'http',
  command: '',
  args: '',
  url: '',
  headersRaw: '',
}

// ── Helpers ──────────────────────────────────────────────────────────────────

function parseHeaders(raw: string): Record<string, string> {
  const result: Record<string, string> = {}
  for (const line of raw.split('\n')) {
    const colon = line.indexOf(':')
    if (colon < 1) continue
    const key = line.slice(0, colon).trim()
    const value = line.slice(colon + 1).trim()
    if (key) result[key] = value
  }
  return result
}

function transportLabel(t: McpTransport): string {
  return { stdio: 'stdio', http: 'HTTP', sse: 'SSE', ws: 'WebSocket' }[t]
}

function transportColor(t: McpTransport): string {
  return {
    stdio: 'bg-[#F0EDE8] text-[#5E5B55]',
    http: 'bg-[#EBF1FF] text-[#2F5BFF]',
    sse: 'bg-[#EDF8F0] text-[#1E7A3E]',
    ws: 'bg-[#FFF4E8] text-[#C25A00]',
  }[t]
}

// ── Sub-components ────────────────────────────────────────────────────────────

function TransportBadge({ transport }: { transport: McpTransport }) {
  return (
    <span
      className={`inline-flex items-center rounded-full px-2.5 py-0.5 text-[11px] font-medium uppercase tracking-wide ${transportColor(transport)}`}
    >
      {transportLabel(transport)}
    </span>
  )
}

function ServerRow({
  server,
  onToggleEnabled,
  onTest,
  onDelete,
  testState,
}: {
  server: McpServer
  onToggleEnabled: (id: string, enabled: boolean) => void
  onTest: (id: string) => void
  onDelete: (id: string) => void
  testState: { loading: boolean; result: TestResult | null }
}) {
  return (
    <div className="flex flex-col gap-3 border-b border-[#E8E4DC] px-6 py-4 last:border-0 sm:flex-row sm:items-center sm:justify-between">
      <div className="flex min-w-0 flex-1 items-start gap-3">
        <div
          className={`mt-0.5 flex h-8 w-8 shrink-0 items-center justify-center rounded-full ${
            server.enabled ? 'bg-[#EBF1FF] text-[#2F5BFF]' : 'bg-[#F4F1EA] text-[#A09890]'
          }`}
        >
          {server.transport === 'stdio' ? (
            <Terminal size={14} strokeWidth={2} />
          ) : (
            <Wifi size={14} strokeWidth={2} />
          )}
        </div>

        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <span className="text-[14px] font-semibold text-[#1C1C1C]">{server.name}</span>
            <TransportBadge transport={server.transport} />
            {!server.enabled && (
              <span className="text-[11px] text-[#A09890]">disabled</span>
            )}
          </div>
          <p className="mt-0.5 truncate text-[12px] text-[#9A9590]">
            {server.transport === 'stdio'
              ? [server.command, ...(server.args ?? [])].filter(Boolean).join(' ')
              : server.url ?? '—'}
          </p>

          {testState.result && (
            <div className="mt-1.5 flex items-center gap-1.5">
              {testState.result.connected ? (
                <>
                  <CheckCircle2 size={12} className="shrink-0 text-[#1E7A3E]" />
                  <span className="text-[11px] text-[#1E7A3E]">
                    Connected · {testState.result.tools_count} tool
                    {testState.result.tools_count !== 1 ? 's' : ''}
                  </span>
                </>
              ) : (
                <>
                  <AlertCircle size={12} className="shrink-0 text-[#C0392B]" />
                  <span className="truncate text-[11px] text-[#C0392B]">
                    {testState.result.error ?? 'Connection failed'}
                  </span>
                </>
              )}
            </div>
          )}
        </div>
      </div>

      <div className="flex shrink-0 items-center gap-2 self-end sm:self-auto">
        {/* Enable toggle */}
        <button
          type="button"
          onClick={() => onToggleEnabled(server.id, !server.enabled)}
          className={`relative inline-flex h-5 w-9 shrink-0 cursor-pointer items-center rounded-full transition-colors focus-visible:outline-none ${
            server.enabled ? 'bg-[#2F5BFF]' : 'bg-[#D8D4CC]'
          }`}
          aria-label={server.enabled ? 'Disable server' : 'Enable server'}
        >
          <span
            className={`pointer-events-none inline-block h-3.5 w-3.5 transform rounded-full bg-white shadow-sm transition-transform ${
              server.enabled ? 'translate-x-4' : 'translate-x-0.5'
            }`}
          />
        </button>

        {/* Test */}
        <button
          type="button"
          onClick={() => onTest(server.id)}
          disabled={testState.loading}
          className="inline-flex h-8 items-center gap-1.5 rounded-full border border-[#DDD9D2] bg-white px-3 text-[12px] font-medium text-[#3A3A36] transition hover:border-[#C8C4BC] hover:text-black disabled:opacity-50"
        >
          {testState.loading ? (
            <Loader2 size={12} className="animate-spin" />
          ) : null}
          Test
        </button>

        {/* Delete */}
        <button
          type="button"
          onClick={() => onDelete(server.id)}
          className="inline-flex h-8 w-8 items-center justify-center rounded-full border border-[#DDD9D2] bg-white text-[#A09890] transition hover:border-[#F0D1CD] hover:bg-[#FFF7F6] hover:text-[#C0392B]"
          aria-label="Delete server"
        >
          <Trash2 size={13} strokeWidth={2} />
        </button>
      </div>
    </div>
  )
}

function TransportSelect({
  value,
  onChange,
}: {
  value: McpTransport
  onChange: (v: McpTransport) => void
}) {
  return (
    <div className="relative">
      <select
        value={value}
        onChange={(e) => onChange(e.target.value as McpTransport)}
        className="w-full appearance-none rounded-[10px] border border-[#DDD9D2] bg-white py-2.5 pl-3.5 pr-8 text-[13px] text-[#1C1C1C] focus:border-[#2F5BFF] focus:outline-none"
      >
        <option value="http">HTTP</option>
        <option value="sse">SSE (Server-Sent Events)</option>
        <option value="ws">WebSocket</option>
        <option value="stdio">stdio (subprocess)</option>
      </select>
      <ChevronDown
        size={13}
        className="pointer-events-none absolute right-3 top-1/2 -translate-y-1/2 text-[#9A9590]"
      />
    </div>
  )
}

function AddServerForm({
  onSave,
  onCancel,
  saving,
}: {
  onSave: (form: FormState) => void
  onCancel: () => void
  saving: boolean
}) {
  const [form, setForm] = useState<FormState>(FORM_DEFAULTS)

  const set = (field: keyof FormState, value: string) =>
    setForm((prev) => ({ ...prev, [field]: value }))

  const isNetwork = form.transport !== 'stdio'

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault()
    onSave(form)
  }

  return (
    <form
      onSubmit={handleSubmit}
      className="border-t border-[#E8E4DC] bg-[#FAFAF8] px-6 py-5"
    >
      <p className="mb-4 text-[13px] font-semibold text-[#1C1C1C]">Add MCP server</p>

      <div className="grid gap-3 sm:grid-cols-2">
        {/* Name */}
        <div className="sm:col-span-2">
          <label className="mb-1 block text-[11px] font-medium uppercase tracking-wider text-[#9A9590]">
            Server name
          </label>
          <input
            type="text"
            required
            value={form.name}
            onChange={(e) => set('name', e.target.value)}
            placeholder="e.g. Stripe tools"
            className="w-full rounded-[10px] border border-[#DDD9D2] bg-white px-3.5 py-2.5 text-[13px] text-[#1C1C1C] placeholder:text-[#B8B2AA] focus:border-[#2F5BFF] focus:outline-none"
          />
        </div>

        {/* Transport */}
        <div>
          <label className="mb-1 block text-[11px] font-medium uppercase tracking-wider text-[#9A9590]">
            Transport
          </label>
          <TransportSelect
            value={form.transport}
            onChange={(v) => set('transport', v)}
          />
        </div>

        {/* URL (network transports) */}
        {isNetwork && (
          <div>
            <label className="mb-1 block text-[11px] font-medium uppercase tracking-wider text-[#9A9590]">
              Server URL
            </label>
            <input
              type="url"
              required
              value={form.url}
              onChange={(e) => set('url', e.target.value)}
              placeholder="https://..."
              className="w-full rounded-[10px] border border-[#DDD9D2] bg-white px-3.5 py-2.5 text-[13px] text-[#1C1C1C] placeholder:text-[#B8B2AA] focus:border-[#2F5BFF] focus:outline-none"
            />
          </div>
        )}

        {/* Command (stdio) */}
        {!isNetwork && (
          <>
            <div>
              <label className="mb-1 block text-[11px] font-medium uppercase tracking-wider text-[#9A9590]">
                Command
              </label>
              <input
                type="text"
                required
                value={form.command}
                onChange={(e) => set('command', e.target.value)}
                placeholder="npx"
                className="w-full rounded-[10px] border border-[#DDD9D2] bg-white px-3.5 py-2.5 font-mono text-[12px] text-[#1C1C1C] placeholder:text-[#B8B2AA] focus:border-[#2F5BFF] focus:outline-none"
              />
            </div>
            <div>
              <label className="mb-1 block text-[11px] font-medium uppercase tracking-wider text-[#9A9590]">
                Arguments (space-separated)
              </label>
              <input
                type="text"
                value={form.args}
                onChange={(e) => set('args', e.target.value)}
                placeholder="-y @acme/mcp-server"
                className="w-full rounded-[10px] border border-[#DDD9D2] bg-white px-3.5 py-2.5 font-mono text-[12px] text-[#1C1C1C] placeholder:text-[#B8B2AA] focus:border-[#2F5BFF] focus:outline-none"
              />
            </div>
          </>
        )}

        {/* Headers (network only, optional) */}
        {isNetwork && (
          <div className="sm:col-span-2">
            <label className="mb-1 block text-[11px] font-medium uppercase tracking-wider text-[#9A9590]">
              Headers (optional, one per line: <code className="font-mono">Key: Value</code>)
            </label>
            <textarea
              rows={2}
              value={form.headersRaw}
              onChange={(e) => set('headersRaw', e.target.value)}
              placeholder={'Authorization: Bearer sk-...\nX-Custom-Header: value'}
              className="w-full rounded-[10px] border border-[#DDD9D2] bg-white px-3.5 py-2.5 font-mono text-[12px] text-[#1C1C1C] placeholder:text-[#B8B2AA] focus:border-[#2F5BFF] focus:outline-none"
            />
          </div>
        )}
      </div>

      <div className="mt-4 flex items-center justify-end gap-3">
        <button
          type="button"
          onClick={onCancel}
          className="inline-flex h-9 items-center rounded-full border border-[#DDD9D2] bg-white px-4 text-[13px] font-medium text-[#5E5B55] transition hover:border-[#C8C4BC]"
        >
          Cancel
        </button>
        <button
          type="submit"
          disabled={saving}
          className="inline-flex h-9 items-center gap-2 rounded-full bg-[#2F5BFF] px-4 text-[13px] font-medium text-white shadow-[0_8px_18px_rgba(47,91,255,0.22)] transition hover:opacity-90 disabled:opacity-60"
        >
          {saving ? <Loader2 size={13} className="animate-spin" /> : null}
          Save server
        </button>
      </div>
    </form>
  )
}

// ── State management ──────────────────────────────────────────────────────────

type TestMap = Record<string, { loading: boolean; result: TestResult | null }>

type State = {
  servers: McpServer[]
  loading: boolean
  error: string | null
  showForm: boolean
  saving: boolean
  tests: TestMap
}

type Action =
  | { type: 'FETCH_START' }
  | { type: 'FETCH_OK'; servers: McpServer[] }
  | { type: 'FETCH_ERR'; error: string }
  | { type: 'SHOW_FORM' }
  | { type: 'HIDE_FORM' }
  | { type: 'SAVE_START' }
  | { type: 'SAVE_OK'; server: McpServer }
  | { type: 'SAVE_ERR' }
  | { type: 'DELETE'; id: string }
  | { type: 'TOGGLE'; id: string; enabled: boolean }
  | { type: 'TEST_START'; id: string }
  | { type: 'TEST_DONE'; id: string; result: TestResult }

function reducer(state: State, action: Action): State {
  switch (action.type) {
    case 'FETCH_START':
      return { ...state, loading: true, error: null }
    case 'FETCH_OK':
      return { ...state, loading: false, servers: action.servers }
    case 'FETCH_ERR':
      return { ...state, loading: false, error: action.error }
    case 'SHOW_FORM':
      return { ...state, showForm: true }
    case 'HIDE_FORM':
      return { ...state, showForm: false }
    case 'SAVE_START':
      return { ...state, saving: true }
    case 'SAVE_OK':
      return {
        ...state,
        saving: false,
        showForm: false,
        servers: [...state.servers, action.server],
      }
    case 'SAVE_ERR':
      return { ...state, saving: false }
    case 'DELETE':
      return { ...state, servers: state.servers.filter((s) => s.id !== action.id) }
    case 'TOGGLE':
      return {
        ...state,
        servers: state.servers.map((s) =>
          s.id === action.id ? { ...s, enabled: action.enabled } : s,
        ),
      }
    case 'TEST_START':
      return { ...state, tests: { ...state.tests, [action.id]: { loading: true, result: null } } }
    case 'TEST_DONE':
      return {
        ...state,
        tests: { ...state.tests, [action.id]: { loading: false, result: action.result } },
      }
    default:
      return state
  }
}

const INITIAL_STATE: State = {
  servers: [],
  loading: true,
  error: null,
  showForm: false,
  saving: false,
  tests: {},
}

// ── Main component ─────────────────────────────────────────────────────────────

export function McpServersPanel() {
  const [state, dispatch] = useReducer(reducer, INITIAL_STATE)
  const mountedRef = useRef(true)

  useEffect(() => {
    mountedRef.current = true
    return () => { mountedRef.current = false }
  }, [])

  const load = useCallback(async () => {
    dispatch({ type: 'FETCH_START' })
    try {
      const res = await fetch('/api/mcp/servers')
      if (!res.ok) throw new Error(`Failed to load MCP servers (${res.status})`)
      const data: McpServer[] = await res.json()
      if (mountedRef.current) dispatch({ type: 'FETCH_OK', servers: data })
    } catch (err) {
      if (mountedRef.current)
        dispatch({ type: 'FETCH_ERR', error: err instanceof Error ? err.message : 'Load failed' })
    }
  }, [])

  useEffect(() => { void load() }, [load])

  const handleSave = useCallback(async (form: FormState) => {
    dispatch({ type: 'SAVE_START' })
    try {
      const isNetwork = form.transport !== 'stdio'
      const body = {
        name: form.name.trim(),
        transport: form.transport,
        ...(isNetwork
          ? {
              url: form.url.trim(),
              headers: parseHeaders(form.headersRaw),
            }
          : {
              command: form.command.trim(),
              args: form.args.trim() ? form.args.trim().split(/\s+/) : [],
            }),
      }
      const res = await fetch('/api/mcp/servers', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      })
      const data = await res.json()
      if (!res.ok) throw new Error(data.error ?? 'Failed to save')
      if (mountedRef.current) dispatch({ type: 'SAVE_OK', server: data as McpServer })
    } catch {
      if (mountedRef.current) dispatch({ type: 'SAVE_ERR' })
    }
  }, [])

  const handleToggle = useCallback(async (id: string, enabled: boolean) => {
    dispatch({ type: 'TOGGLE', id, enabled })
    await fetch(`/api/mcp/servers/${id}`, {
      method: 'PATCH',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ enabled }),
    })
  }, [])

  const handleTest = useCallback(async (id: string) => {
    dispatch({ type: 'TEST_START', id })
    try {
      const res = await fetch(`/api/mcp/servers/${id}/test`, { method: 'POST' })
      const result: TestResult = await res.json()
      if (mountedRef.current) dispatch({ type: 'TEST_DONE', id, result })
    } catch {
      if (mountedRef.current)
        dispatch({ type: 'TEST_DONE', id, result: { connected: false, tools_count: 0, error: 'Request failed' } })
    }
  }, [])

  const handleDelete = useCallback(async (id: string) => {
    dispatch({ type: 'DELETE', id })
    await fetch(`/api/mcp/servers/${id}`, { method: 'DELETE' })
  }, [])

  return (
    <section className="mt-8 overflow-hidden rounded-[24px] border border-[#DFDDD7] bg-white shadow-[0_26px_50px_rgba(24,20,12,0.08)]">
      {/* Header */}
      <div className="flex items-center justify-between px-6 py-5">
        <div>
          <h2 className="text-[18px] font-semibold tracking-[-0.03em] text-[#1C1C1C]">
            MCP Servers
          </h2>
          <p className="mt-1 text-[13px] leading-5 text-[#7D7D76]">
            Connect external Model Context Protocol servers to extend agent tools.
          </p>
        </div>
        {!state.showForm && (
          <button
            type="button"
            onClick={() => dispatch({ type: 'SHOW_FORM' })}
            className="inline-flex h-9 items-center gap-2 rounded-full bg-[#2F5BFF] px-4 text-[13px] font-medium text-white shadow-[0_8px_18px_rgba(47,91,255,0.22)] transition hover:opacity-90"
          >
            <Plus size={14} strokeWidth={2.5} />
            Add server
          </button>
        )}
        {state.showForm && (
          <button
            type="button"
            onClick={() => dispatch({ type: 'HIDE_FORM' })}
            className="inline-flex h-9 w-9 items-center justify-center rounded-full border border-[#DDD9D2] bg-white text-[#9A9590] transition hover:bg-[#F4F1EA]"
            aria-label="Close form"
          >
            <X size={14} strokeWidth={2} />
          </button>
        )}
      </div>

      {/* Error banner */}
      {state.error && (
        <div className="mx-6 mb-4 rounded-[14px] border border-[#F0D1CD] bg-[#FFF7F6] px-4 py-3 text-[13px] text-[#985554]">
          {state.error}
        </div>
      )}

      {/* Server list */}
      {state.loading ? (
        <div className="flex items-center justify-center border-t border-[#E8E4DC] py-10">
          <Loader2 size={20} className="animate-spin text-[#B8B2AA]" />
        </div>
      ) : state.servers.length === 0 && !state.showForm ? (
        <div className="border-t border-[#E8E4DC] px-6 py-10 text-center">
          <p className="text-[14px] text-[#9A9590]">
            No MCP servers configured. Add one to extend your agents with external tools.
          </p>
        </div>
      ) : state.servers.length > 0 ? (
        <div className="border-t border-[#E8E4DC]">
          {state.servers.map((server) => (
            <ServerRow
              key={server.id}
              server={server}
              onToggleEnabled={handleToggle}
              onTest={handleTest}
              onDelete={handleDelete}
              testState={state.tests[server.id] ?? { loading: false, result: null }}
            />
          ))}
        </div>
      ) : null}

      {/* Add form */}
      {state.showForm && (
        <AddServerForm
          onSave={handleSave}
          onCancel={() => dispatch({ type: 'HIDE_FORM' })}
          saving={state.saving}
        />
      )}
    </section>
  )
}
