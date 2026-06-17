'use client'

import { Sparkles } from 'lucide-react'

interface AiDraftButtonProps {
  onSuggest: () => void
  isStreaming: boolean
}

export function AiDraftButton({ onSuggest, isStreaming }: AiDraftButtonProps) {
  return (
    <button
      type="button"
      onClick={onSuggest}
      disabled={isStreaming}
      className="p-1.5 text-gray-400 hover:text-blue-600 hover:bg-blue-50 rounded-md flex items-center gap-1 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
      title="Suggest reply with AI"
    >
      <Sparkles className={`size-3.5 ${isStreaming ? 'animate-pulse text-blue-500' : ''}`} />
      <span className="text-[11px] font-medium">
        {isStreaming ? 'Drafting...' : 'Suggest reply'}
      </span>
    </button>
  )
}
