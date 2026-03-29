'use client';

import { Mic, Plus, ArrowUp, Zap, History } from 'lucide-react';
import { m } from 'framer-motion';
import { ModelSelector } from './ModelSelector';

const cn = (...classes: (string | undefined | null | false)[]): string =>
  classes.filter(Boolean).join(' ');

interface ChatInputToolbarProps {
  disabled: boolean;
  isAttachmentOpen: boolean;
  isToolsOpen: boolean;
  isHistoryOpen: boolean;
  onToggleAttachment: () => void;
  onToggleTools: () => void;
  onToggleHistory: () => void;
  selectedModel: string;
  onModelChange: (model: string) => void;
  dropDirection: 'up' | 'down';
  isRecording: boolean;
  onRecordingToggle: () => void;
  hasContent: boolean;
  inputDisabled: boolean;
  isLoading: boolean;
  t: (key: string) => string;
}

export function ChatInputToolbar({
  disabled,
  isAttachmentOpen,
  isToolsOpen,
  isHistoryOpen,
  onToggleAttachment,
  onToggleTools,
  onToggleHistory,
  selectedModel,
  onModelChange,
  dropDirection,
  isRecording,
  onRecordingToggle,
  hasContent,
  inputDisabled,
  isLoading,
  t,
}: ChatInputToolbarProps) {
  return (
    <div className="px-6 pb-6">
      <div className="flex items-center justify-between gap-4">
        {/* Left controls group */}
        <div className="flex items-center gap-2 p-1 bg-gray-50/50 rounded-xl">
          <m.button
            type="button"
            disabled={disabled}
            onClick={onToggleAttachment}
            whileHover={{ scale: 1.05 }}
            whileTap={{ scale: 0.95 }}
            className={cn(
              'flex-shrink-0 w-11 h-11 rounded-xl bg-white shadow-sm',
              'flex items-center justify-center text-gray-600 hover:text-gray-800',
              'hover:shadow-md transition-all duration-200 disabled:opacity-50',
              isAttachmentOpen && 'shadow-md text-blue-600 bg-blue-50'
            )}
            aria-label={t('chat.addAttachments')}
            aria-expanded={isAttachmentOpen}
          >
            <Plus className="w-5 h-5" />
          </m.button>

          <m.button
            type="button"
            disabled={disabled}
            onClick={onToggleTools}
            whileHover={{ scale: 1.02 }}
            whileTap={{ scale: 0.98 }}
            className={cn(
              'flex-shrink-0 inline-flex items-center gap-2 px-4 py-2.5 bg-white hover:bg-blue-50 rounded-xl text-sm font-medium text-gray-700 transition-all duration-200 shadow-sm disabled:opacity-50',
              isToolsOpen && 'shadow-md text-blue-700 bg-blue-50'
            )}
            aria-expanded={isToolsOpen}
          >
            <Zap className="w-4 h-4 text-blue-600" />
            <span>{t('chat.tools')}</span>
          </m.button>

          <m.button
            type="button"
            disabled={disabled}
            onClick={onToggleHistory}
            whileHover={{ scale: 1.02 }}
            whileTap={{ scale: 0.98 }}
            className={cn(
              'flex-shrink-0 inline-flex items-center gap-2 px-4 py-2.5 bg-white hover:bg-green-50 rounded-xl text-sm font-medium text-gray-700 transition-all duration-200 shadow-sm disabled:opacity-50',
              isHistoryOpen && 'shadow-md text-green-700 bg-green-50'
            )}
            aria-expanded={isHistoryOpen}
          >
            <History className="w-4 h-4 text-green-600" />
            <span>{t('chat.history')}</span>
          </m.button>
        </div>

        {/* Right controls group */}
        <div className="flex items-center gap-3 flex-shrink-0">
          <div className="bg-gray-50/50 rounded-xl px-3 py-2">
            <ModelSelector
              value={selectedModel}
              onChange={onModelChange}
              dropDirection={dropDirection}
            />
          </div>

          <m.button
            type="button"
            disabled={disabled}
            onClick={onRecordingToggle}
            whileHover={{ scale: 1.05 }}
            whileTap={{ scale: 0.95 }}
            className={cn(
              'w-11 h-11 flex items-center justify-center rounded-xl transition-all duration-200 shadow-sm disabled:opacity-50',
              isRecording
                ? 'bg-orange-50 text-orange-600 hover:bg-orange-100 shadow-md'
                : 'bg-white text-gray-500 hover:text-gray-700 hover:bg-gray-50'
            )}
            aria-label={t('chat.voiceInput')}
            aria-pressed={isRecording}
          >
            <Mic className="w-5 h-5" />
          </m.button>

          <m.button
            type="submit"
            disabled={!hasContent || inputDisabled}
            whileHover={hasContent && !inputDisabled ? { scale: 1.05 } : {}}
            whileTap={hasContent && !inputDisabled ? { scale: 0.95 } : {}}
            className={cn(
              'w-12 h-12 flex items-center justify-center rounded-xl transition-all duration-200 font-medium shadow-lg',
              hasContent && !inputDisabled
                ? 'bg-gradient-to-r from-blue-500 to-blue-600 hover:from-blue-600 hover:to-blue-700 text-white shadow-blue-500/25 hover:shadow-blue-500/40'
                : 'bg-gray-100 text-gray-400 cursor-not-allowed shadow-sm'
            )}
            aria-label={t('chat.sendMessage')}
          >
            {isLoading ? (
              <m.div
                animate={{ rotate: 360 }}
                transition={{ duration: 1, repeat: Infinity, ease: 'linear' }}
                className="w-5 h-5 border-2 border-white/30 border-t-white rounded-full"
              />
            ) : (
              <ArrowUp className="w-5 h-5" />
            )}
          </m.button>
        </div>
      </div>
    </div>
  );
}
