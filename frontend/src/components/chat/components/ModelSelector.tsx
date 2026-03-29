'use client';

/**
 * ModelSelector – centralized model definitions and selection
 * - Single source of truth for all available models
 * - Handles localization internally
 */

import { useState } from 'react';
import { ChevronDown, Zap } from 'lucide-react';
import { m, AnimatePresence } from 'framer-motion';
import { useI18n } from '@/components/chat/hooks/i18n';

// Model definitions - centralized source of truth
export interface ModelInfo {
  id: string;
  key: string;
  descriptionKey: string;
}

export const AVAILABLE_MODELS: ModelInfo[] = [
  {
    id: 'aquatiq-gpt',
    key: 'chat.models.aquatiqGpt',
    descriptionKey: 'chat.models.description.aquatiqGpt'
  },
  {
    id: 'chat-5',
    key: 'chat.models.chat5',
    descriptionKey: 'chat.models.description.chat5'
  },
  {
    id: 'chat-bilder',
    key: 'chat.models.chatBilder',
    descriptionKey: 'chat.models.description.chatBilder'
  }
];

// Utility functions for other components to use
export const getModelNames = (t: (key: string) => string): string[] => {
  return AVAILABLE_MODELS.map(model => t(model.key));
};

export const getModelDescription = (modelName: string, t: (key: string) => string): string => {
  const model = AVAILABLE_MODELS.find(m => t(m.key) === modelName);
  return model ? t(model.descriptionKey) : 'AI Model';
};

export const getDefaultModel = (t: (key: string) => string): string => {
  return t(AVAILABLE_MODELS[0].key); // Default to first model
};

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
  
  // Use centralized model definitions
  const availableModels = getModelNames(t);

  const isDropUp = dropDirection === 'up';

  return (
    <div className="relative select-none">
      <button
        type="button"
        onClick={() => setOpen(o => !o)}
        className="flex items-center gap-1.5 px-3 py-2 rounded-xl bg-white/70 backdrop-blur  text-sm text-gray-700 hover:bg-white shadow-sm"
        title={t('chat.models.selectModel')}
      >
        <Zap className="w-4 h-4 text-green-600" />
        <span className="truncate max-w-[120px]">{value}</span>
        <ChevronDown className={`w-4 h-4 transition-transform ${open ? 'rotate-180' : ''}`} />
      </button>

      <AnimatePresence>
        {open && (
          <m.ul
            initial={{ opacity: 0, y: isDropUp ? 4 : -4 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: isDropUp ? 4 : -4 }}
            className={`absolute right-0 z-30 w-56 rounded-2xl bg-white/95 backdrop-blur-xl shadow-[0_12px_40px_rgba(0,0,0,0.12)] p-1 text-sm ${
              isDropUp ? 'bottom-full mb-2' : 'top-full mt-2'
            }`}
          >
            {availableModels.map((modelName) => (
              <li key={modelName}>
                <button
                  type="button"
                  onClick={() => { onChange(modelName); setOpen(false); }}
                  className={`w-full text-left px-3 py-2.5 rounded-xl hover:bg-gray-50 ${modelName === value ? 'bg-blue-50/60 text-blue-700 font-medium' : 'text-gray-800'}`}
                >
                  <div className="flex items-center gap-3">
                    <div className="w-6 h-6 rounded-full bg-gray-50 flex items-center justify-center">
                      <Zap className="w-3 h-3 text-gray-600" />
                    </div>
                    <div className="flex-1">
                      <div className="font-medium">{modelName}</div>
                      <div className="text-xs text-gray-500 mt-0.5">
                        {getModelDescription(modelName, t)}
                      </div>
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
