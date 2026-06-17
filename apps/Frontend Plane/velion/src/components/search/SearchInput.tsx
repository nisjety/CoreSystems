'use client';

import { useRef, useState, useCallback } from 'react';
import { Search, X, ArrowRight, Calendar } from 'lucide-react';
import { AnimatePresence } from 'framer-motion';
import { AutocompleteDropdown, type AcItem } from '../chat/components/AutocompleteDropdown';

// ── Autocomplete helpers (mirrored from ChatInput) ────────────────────────

const DAY_ENTRIES = [
  { name: 'Monday',    dayIndex: 1 },
  { name: 'Tuesday',   dayIndex: 2 },
  { name: 'Wednesday', dayIndex: 3 },
  { name: 'Thursday',  dayIndex: 4 },
  { name: 'Friday',    dayIndex: 5 },
  { name: 'Saturday',  dayIndex: 6 },
  { name: 'Sunday',    dayIndex: 0 },
];

function getUpcomingDates(dayIndex: number): AcItem[] {
  const today = new Date();
  const items: AcItem[] = [];
  const d = new Date(today);
  while (items.length < 2) {
    d.setDate(d.getDate() + 1);
    if (d.getDay() === dayIndex) {
      const label = d.toLocaleDateString('en-US', {
        weekday: 'short',
        month: 'short',
        day: 'numeric',
      });
      items.push({ id: `date-${label}`, icon: <Calendar size={16} />, label, meta: 'date' });
    }
  }
  return items;
}

function detectDayTrigger(text: string, pos: number) {
  const before = text.slice(0, pos);
  const wordM = before.match(/\b([A-Za-z]{3,})$/);
  if (!wordM) return null;
  const lower = wordM[1].toLowerCase();
  const entry = DAY_ENTRIES.find(e => e.name.toLowerCase().startsWith(lower));
  if (!entry) return null;
  return {
    dayIndex: entry.dayIndex,
    start: pos - wordM[1].length,
    rawLen: wordM[1].length,
  };
}

// ── Component ─────────────────────────────────────────────────────────────

interface SearchInputProps {
  value: string;
  onChange: (v: string) => void;
  onSubmit: (query: string) => void;
  isLoading?: boolean;
  placeholder?: string;
}

interface AcState {
  items: AcItem[];
  category: string;
  triggerStart: number;
  triggerLen: number;
}

export function SearchInput({
  value,
  onChange,
  onSubmit,
  isLoading = false,
  placeholder = 'Spør om selskapet, dokumenter, kollegaer…',
}: SearchInputProps) {
  const inputRef = useRef<HTMLInputElement>(null);
  const [acState, setAcState] = useState<AcState | null>(null);
  const [acIndex, setAcIndex] = useState(0);

  const updateAc = useCallback((text: string, pos: number) => {
    const trigger = detectDayTrigger(text, pos);
    if (!trigger) { setAcState(null); return; }
    const items = getUpcomingDates(trigger.dayIndex);
    if (items.length) {
      setAcState({ items, category: 'Schedule', triggerStart: trigger.start, triggerLen: trigger.rawLen });
      setAcIndex(0);
    } else {
      setAcState(null);
    }
  }, []);

  const handleChange = useCallback(
    (e: React.ChangeEvent<HTMLInputElement>) => {
      const text = e.target.value;
      const pos = e.target.selectionStart ?? text.length;
      onChange(text);
      updateAc(text, pos);
    },
    [onChange, updateAc]
  );

  const applySelection = useCallback(
    (item: AcItem) => {
      if (!acState) return;
      const { triggerStart, triggerLen } = acState;
      const before = value.slice(0, triggerStart);
      const after = value.slice(triggerStart + triggerLen);
      const newVal = before + item.label + after;
      onChange(newVal);
      setAcState(null);
      requestAnimationFrame(() => {
        const el = inputRef.current;
        if (el) {
          el.focus();
          const cur = triggerStart + item.label.length;
          el.setSelectionRange(cur, cur);
        }
      });
    },
    [acState, value, onChange]
  );

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLInputElement>) => {
      if (acState) {
        if (e.key === 'ArrowDown') {
          e.preventDefault();
          setAcIndex(i => Math.min(i + 1, acState.items.length - 1));
          return;
        }
        if (e.key === 'ArrowUp') {
          e.preventDefault();
          setAcIndex(i => Math.max(i - 1, 0));
          return;
        }
        if (e.key === 'Enter' || e.key === 'Tab') {
          e.preventDefault();
          applySelection(acState.items[acIndex]);
          return;
        }
        if (e.key === 'Escape') {
          setAcState(null);
          return;
        }
      }
      if (e.key === 'Enter') e.currentTarget.form?.requestSubmit();
    },
    [acState, acIndex, applySelection]
  );

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (acState) return;
    const q = value.trim();
    if (q) onSubmit(q);
  };

  const handleClear = () => {
    onChange('');
    setAcState(null);
    inputRef.current?.focus();
  };

  return (
    <div className="relative w-full">
      <form onSubmit={handleSubmit} className="relative w-full">
        <Search
          size={16}
          strokeWidth={1.5}
          className="pointer-events-none absolute left-4 top-1/2 -translate-y-1/2 text-[#C8C1B3]"
        />

        <input
          ref={inputRef}
          type="text"
          value={value}
          onChange={handleChange}
          onKeyDown={handleKeyDown}
          placeholder={placeholder}
          className="w-full border border-[#D8D2C6] bg-[#F4F1EB] py-4 pl-11 pr-24 font-inter text-[15px] text-[#2B2B2B] outline-none transition-colors placeholder:text-[#C8C1B3] focus:border-[#2B2B2B] focus:bg-white"
        />

        <div className="absolute right-2 top-1/2 flex -translate-y-1/2 items-center gap-1">
          {value && (
            <button
              type="button"
              onClick={handleClear}
              className="flex h-7 w-7 items-center justify-center text-[#C8C1B3] transition-colors hover:text-[#2B2B2B]"
              aria-label="Tøm søk"
            >
              <X size={13} strokeWidth={1.5} />
            </button>
          )}
          <button
            type="submit"
            disabled={!value.trim() || isLoading}
            className="flex h-8 items-center gap-1.5 border border-[#D8D2C6] bg-[#EAE6DF] px-3 font-inter text-[11px] uppercase tracking-widest text-[#4A4A48] transition-colors hover:border-[#2B2B2B] hover:bg-[#2B2B2B] hover:text-white disabled:cursor-not-allowed disabled:opacity-40"
          >
            Søk
            <ArrowRight size={11} strokeWidth={1.5} />
          </button>
        </div>
      </form>

      {/* Autocomplete dropdown — positioned above the input */}
      <AnimatePresence>
        {acState && (
          <div className="absolute bottom-full mb-2 left-0 z-50">
            <AutocompleteDropdown
              category={acState.category}
              items={acState.items}
              selectedIndex={acIndex}
              onSelect={applySelection}
              onHover={setAcIndex}
            />
          </div>
        )}
      </AnimatePresence>
    </div>
  );
}
