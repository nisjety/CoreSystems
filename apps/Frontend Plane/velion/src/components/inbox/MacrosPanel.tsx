'use client'

import { useCallback, useEffect, useState } from 'react'
import { ChevronDown, ChevronRight, Play } from 'lucide-react'

interface Macro {
  id: number
  name: string
  [key: string]: unknown
}

interface MacrosPanelProps {
  ticketId: number | null
  onMacroExecuted?: () => void
}

export function MacrosPanel({ ticketId, onMacroExecuted }: MacrosPanelProps) {
  const [macros, setMacros] = useState<Macro[]>([])
  const [isExpanded, setIsExpanded] = useState(false)
  const [loading, setLoading] = useState(false)
  const [executingId, setExecutingId] = useState<number | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    setLoading(true)
    setError(null)

    fetch('/api/support/macros')
      .then((r) => {
        if (!r.ok) throw new Error(`Failed to load macros: ${r.status}`)
        return r.json() as Promise<Macro[]>
      })
      .then((data) => {
        if (!cancelled) setMacros(data)
      })
      .catch((err: unknown) => {
        if (!cancelled) setError(err instanceof Error ? err.message : 'Failed to load macros')
      })
      .finally(() => {
        if (!cancelled) setLoading(false)
      })

    return () => {
      cancelled = true
    }
  }, [])

  const executeMacro = useCallback(
    async (macroId: number) => {
      if (!ticketId || executingId !== null) return
      setExecutingId(macroId)
      setError(null)

      try {
        const res = await fetch(`/api/support/macros/${macroId}/execute`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ ticketId }),
        })
        if (!res.ok) throw new Error(`Macro execution failed: ${res.status}`)
        onMacroExecuted?.()
      } catch (err: unknown) {
        setError(err instanceof Error ? err.message : 'Macro execution failed')
      } finally {
        setExecutingId(null)
      }
    },
    [ticketId, executingId, onMacroExecuted],
  )

  return (
    <div className="border border-gray-200 rounded-lg bg-white overflow-hidden">
      <button
        type="button"
        onClick={() => setIsExpanded((v) => !v)}
        className="w-full flex items-center justify-between px-4 py-3 text-left hover:bg-gray-50 transition-colors"
      >
        <span className="text-[13px] font-semibold text-gray-900">Macros</span>
        {isExpanded ? (
          <ChevronDown className="size-4 text-gray-400" />
        ) : (
          <ChevronRight className="size-4 text-gray-400" />
        )}
      </button>

      {isExpanded && (
        <div className="border-t border-gray-200">
          {loading && (
            <div className="px-4 py-3 space-y-2">
              {[1, 2, 3].map((i) => (
                <div key={i} className="h-8 bg-gray-100 rounded animate-pulse" />
              ))}
            </div>
          )}

          {error && (
            <p className="px-4 py-3 text-[12px] text-red-600">{error}</p>
          )}

          {!loading && !error && macros.length === 0 && (
            <p className="px-4 py-3 text-[12px] text-gray-500">No macros available.</p>
          )}

          {!loading && macros.length > 0 && (
            <ul className="divide-y divide-gray-100">
              {macros.map((macro) => (
                <li key={macro.id} className="flex items-center justify-between px-4 py-2.5 hover:bg-gray-50">
                  <span className="text-[13px] text-gray-800 truncate flex-1 mr-2">{macro.name}</span>
                  <button
                    type="button"
                    disabled={!ticketId || executingId === macro.id}
                    onClick={() => executeMacro(macro.id)}
                    className="flex items-center gap-1 rounded px-2 py-1 text-[11px] font-medium bg-gray-100 text-gray-700 hover:bg-blue-50 hover:text-blue-700 transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
                    title={!ticketId ? 'Select a ticket first' : 'Run macro'}
                  >
                    <Play className={`size-3 ${executingId === macro.id ? 'animate-pulse' : ''}`} />
                    {executingId === macro.id ? 'Running…' : 'Run'}
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
    </div>
  )
}
