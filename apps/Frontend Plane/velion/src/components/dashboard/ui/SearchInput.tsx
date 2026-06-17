'use client';

import { useRef, useState, useCallback } from 'react';
import { Search, Calendar } from 'lucide-react';
import { AnimatePresence } from 'framer-motion';
import { AutocompleteDropdown, type AcItem } from '../../chat/components/AutocompleteDropdown';

// ── Autocomplete helpers ───────────────────────────────────────────────────

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
  return { dayIndex: entry.dayIndex, start: pos - wordM[1].length, rawLen: wordM[1].length };
}

// ── Component ─────────────────────────────────────────────────────────────

interface SearchInputProps {
  value: string;
  onChange: (value: string) => void;
  onSearch: (query: string) => void;
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
  onSearch,
  placeholder = 'Søk i selskapets kunnskap...',
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
      if (e.key === 'Enter' && value.trim()) {
        onSearch(value.trim());
      }
    },
    [acState, acIndex, applySelection, value, onSearch]
  );

  return (
    <div className="relative">
      <Search
        className="absolute left-4 top-1/2 -translate-y-1/2 text-[#C0B8AF]"
        size={17}
      />
      <input
        ref={inputRef}
        type="text"
        placeholder={placeholder}
        value={value}
        onChange={handleChange}
        onKeyDown={handleKeyDown}
        className="w-full rounded-2xl border border-[#E5E0D8] bg-white py-4 pl-11 pr-4 text-[14px] text-[#1A1A1A] shadow-sm outline-none transition-colors placeholder:text-[#C0B8AF] focus:border-[#C0B8AF] focus:shadow-md"
      />

      {/* Autocomplete dropdown */}
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
