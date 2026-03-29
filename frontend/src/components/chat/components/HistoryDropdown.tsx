'use client';

import { Clock } from 'lucide-react';
import { m, AnimatePresence } from 'framer-motion';
import type { HistoryOption } from './chatInputTypes';

interface HistoryDropdownProps {
  isOpen: boolean;
  isDropUp: boolean;
  options: HistoryOption[];
  onSelect: (id: string) => void;
}

export function HistoryDropdown({ isOpen, isDropUp, options, onSelect }: HistoryDropdownProps) {
  return (
    <AnimatePresence>
      {isOpen && (
        <m.div
          initial={{ opacity: 0, y: isDropUp ? -8 : 8, scale: 0.95 }}
          animate={{ opacity: 1, y: 0, scale: 1 }}
          exit={{ opacity: 0, y: isDropUp ? -8 : 8, scale: 0.95 }}
          transition={{ duration: 0.15, ease: 'easeOut' }}
          className={`absolute left-0 bg-white rounded-2xl p-2 w-80 shadow-[0_8px_32px_rgba(0,0,0,0.12)] z-50 ${
            isDropUp ? 'bottom-full mb-2' : 'top-full mt-2'
          }`}
        >
          {options.map((option, index) => (
            <m.button
              key={option.id}
              initial={{ opacity: 0, x: -8 }}
              animate={{ opacity: 1, x: 0 }}
              transition={{ delay: index * 0.03, duration: 0.2 }}
              className="w-full flex items-start gap-3 p-3 hover:bg-gray-50 rounded-xl text-left transition-all shadow-sm hover:shadow-md"
              onClick={() => onSelect(option.id)}
              type="button"
            >
              <div className="w-8 h-8 rounded-lg bg-green-50 flex items-center justify-center shadow-inner mt-0.5">
                <Clock className="w-4 h-4 text-green-600" />
              </div>
              <div className="flex-1 min-w-0">
                <div className="font-medium text-gray-900 text-sm truncate">{option.label}</div>
                <div className="text-xs text-gray-400 mb-1">{option.timestamp}</div>
                <div className="text-xs text-gray-500 line-clamp-2">{option.preview}</div>
              </div>
            </m.button>
          ))}
        </m.div>
      )}
    </AnimatePresence>
  );
}
