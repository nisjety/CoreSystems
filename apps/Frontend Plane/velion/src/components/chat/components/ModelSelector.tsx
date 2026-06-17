'use client';

/**
 * ModelSelector – centralized model definitions and selection
 * - Single source of truth for all available models
 * - Handles localization internally
 */

import { useMemo, useState } from 'react';
import { ChevronDown, Zap } from 'lucide-react';
import { m, AnimatePresence } from 'framer-motion';
import { useI18n } from '@/components/chat/hooks/i18n';

import { DEFAULT_MODEL } from '@/app/api/chat/_lib/models'
import { useModels, type ModelOption } from '@/components/chat/hooks/useModels'

// U2-2 / U3-2 (ui-ux-velion-gap.md §10): the picker is now driven by the
// live capability-core registry via the `useModels()` hook. The hook
// merges live entries with a static fallback so the dropdown never
// renders empty and previously-supported model ids are never stranded
// when the registry doesn't list them.
//
// Descriptions still come from a small static map keyed by id — capability
// rows in the registry don't yet carry a UI description string. When the
// hook returns a model id we don't have a description for, we fall back
// to "<provider> · <label>".

interface ModelInfo {
  id: string
  label: string
  description: string
}

const MODEL_DESCRIPTIONS: Record<string, string> = {
  'gpt-4o-mini': 'Azure · Fast · Cost-efficient · Great for most queries',
  'gpt-5-mini': 'Azure · Newer reasoning model · Higher quality on complex tasks',
  'claude-sonnet-4-5': 'Anthropic · Best coding + nuanced reasoning',
  'claude-opus-4-1': 'Anthropic · Maximum reasoning depth',
}

function toModelInfo(option: ModelOption): ModelInfo {
  return {
    id: option.id,
    label: option.label,
    description:
      MODEL_DESCRIPTIONS[option.id] ??
      `${option.provider ?? 'model'} · ${option.label}`,
  }
}

export const getDefaultModel = (_t?: (key: string) => string): string => DEFAULT_MODEL

interface ModelSelectorProps {
  value: string;
  onChange: (value: string) => void;
  models?: string[]; // Deprecated, will be ignored in favor of AVAILABLE_MODELS
  dropDirection?: 'up' | 'down'; // Direction for dropdown
}

export function ModelSelector({
  value,
  onChange,
  dropDirection = 'down'
}: ModelSelectorProps) {
  const [open, setOpen] = useState(false);
  const { t } = useI18n();
  const { models: rawModels } = useModels()
  const availableModels = useMemo(
    () => rawModels.map(toModelInfo),
    [rawModels],
  )

  const isDropUp = dropDirection === 'up';
  const currentModel = availableModels.find((m) => m.id === value)

  return (
    <div className="relative select-none">
      <button
        type="button"
        onClick={() => setOpen(o => !o)}
        className="flex items-center gap-1.5 px-3 py-2 rounded-[12px] bg-white/70 backdrop-blur text-sm text-gray-700 hover:bg-white shadow-sm"
        title={t('chat.models.selectModel')}
      >
        <Zap className="w-4 h-4 text-green-600" />
        <span className="truncate max-w-[120px]">{currentModel?.label ?? value}</span>
        <ChevronDown className={`w-4 h-4 transition-transform ${open ? 'rotate-180' : ''}`} />
      </button>

      <AnimatePresence>
        {open && (
          <m.ul
            initial={{ opacity: 0, y: isDropUp ? 4 : -4 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: isDropUp ? 4 : -4 }}
            className={`absolute right-0 z-30 w-64 rounded-2xl bg-white/95 backdrop-blur-xl shadow-[0_12px_40px_rgba(0,0,0,0.12)] p-1 text-sm ${
              isDropUp ? 'bottom-full mb-2' : 'top-full mt-2'
            }`}
          >
            {availableModels.map((model) => (
              <li key={model.id}>
                <button
                  type="button"
                  onClick={() => { onChange(model.id); setOpen(false); }}
                  className={`w-full text-left px-3 py-2.5 rounded-xl hover:bg-gray-50 ${model.id === value ? 'bg-blue-50/60 text-blue-700 font-medium' : 'text-gray-800'}`}
                >
                  <div className="flex items-center gap-3">
                    <div className="w-6 h-6 rounded-full bg-gray-50 flex items-center justify-center">
                      <Zap className="w-3 h-3 text-gray-600" />
                    </div>
                    <div className="flex-1">
                      <div className="font-medium">{model.label}</div>
                      <div className="text-xs text-gray-500 mt-0.5">{model.description}</div>
                    </div>
                  </div>
                </button>
              </li>
            ))}
          </m.ul>
        )}
      </AnimatePresence>
    </div>
  );
}
