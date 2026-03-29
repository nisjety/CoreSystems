'use client';

import { m, AnimatePresence } from 'framer-motion';
import type { AttachmentOption } from './chatInputTypes';

interface AttachmentDropdownProps {
  isOpen: boolean;
  isDropUp: boolean;
  options: AttachmentOption[];
  onSelect: (id: string) => void;
}

export function AttachmentDropdown({ isOpen, isDropUp, options, onSelect }: AttachmentDropdownProps) {
  return (
    <AnimatePresence>
      {isOpen && (
        <m.div
          initial={{ opacity: 0, y: isDropUp ? -8 : 8, scale: 0.95 }}
          animate={{ opacity: 1, y: 0, scale: 1 }}
          exit={{ opacity: 0, y: isDropUp ? -8 : 8, scale: 0.95 }}
          transition={{ duration: 0.15, ease: 'easeOut' }}
          className={`absolute left-0 bg-white rounded-2xl p-2 w-72 shadow-[0_8px_32px_rgba(0,0,0,0.12)] z-50 ${
            isDropUp ? 'bottom-full mb-2' : 'top-full mt-2'
          }`}
        >
          {options.map((option, index) => (
            <m.button
              key={option.id}
              initial={{ opacity: 0, x: -8 }}
              animate={{ opacity: 1, x: 0 }}
              transition={{ delay: index * 0.03, duration: 0.2 }}
              className="w-full flex items-center gap-3 p-3 hover:bg-gray-50 rounded-xl text-left transition-all shadow-sm hover:shadow-md"
              onClick={() => onSelect(option.id)}
              type="button"
            >
              <div className="w-10 h-10 rounded-xl bg-gray-50 flex items-center justify-center shadow-inner">
                <option.icon className="w-5 h-5 text-gray-700" />
              </div>
              <div className="flex-1">
                <div className="font-medium text-gray-900 text-sm">{option.label}</div>
                <div className="text-xs text-gray-500">{option.description}</div>
              </div>
            </m.button>
          ))}
        </m.div>
      )}
    </AnimatePresence>
  );
}
