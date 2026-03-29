'use client';

import { m } from 'framer-motion';

export interface AcItem {
  id: string;
  icon: React.ReactNode;
  label: string;
  meta?: string; // 'date' | 'slash'
}

interface AutocompleteDropdownProps {
  category: string;
  items: AcItem[];
  selectedIndex: number;
  onSelect: (item: AcItem) => void;
  onHover: (index: number) => void;
}

const cn = (...cs: (string | false | null | undefined)[]) => cs.filter(Boolean).join(' ');

export function AutocompleteDropdown({
  category,
  items,
  selectedIndex,
  onSelect,
  onHover,
}: AutocompleteDropdownProps) {
  return (
    <m.div
      initial={{ opacity: 0, y: 8, scale: 0.97 }}
      animate={{ opacity: 1, y: 0, scale: 1 }}
      exit={{ opacity: 0, y: 8, scale: 0.97 }}
      transition={{ duration: 0.13, ease: [0.16, 1, 0.3, 1] }}
      className="min-w-[220px] bg-white rounded-2xl shadow-[0_8px_40px_rgba(0,0,0,0.13)] border border-black/[0.05] overflow-hidden"
    >
      {/* Category header */}
      <div className="px-4 pt-3 pb-1">
        <span className="text-[11px] font-semibold text-[#bbb] tracking-wider uppercase">
          {category}
        </span>
      </div>

      {/* Items */}
      <div className="pb-2">
        {items.map((item, i) => (
          <button
            key={item.id}
            type="button"
            // mouseDown prevents textarea blur before the click fires
            onMouseDown={e => {
              e.preventDefault();
              onSelect(item);
            }}
            onMouseEnter={() => onHover(i)}
            className={cn(
              'w-full flex items-center gap-3 px-4 py-2.5 text-[14px] text-[#1a1a1a] text-left transition-colors',
              i === selectedIndex ? 'bg-black/[0.04]' : 'hover:bg-black/[0.03]'
            )}
          >
            <span className="shrink-0 text-[#666]">{item.icon}</span>
            <span className="font-medium">{item.label}</span>
          </button>
        ))}
      </div>
    </m.div>
  );
}
