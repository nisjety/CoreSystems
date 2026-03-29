'use client';

import { useRef } from 'react';
import { Search, X, ArrowRight } from 'lucide-react';

interface SearchInputProps {
  value: string;
  onChange: (v: string) => void;
  onSubmit: (query: string) => void;
  isLoading?: boolean;
  placeholder?: string;
  autoFocus?: boolean;
}

export function SearchInput({
  value,
  onChange,
  onSubmit,
  isLoading = false,
  placeholder = 'Spør om selskapet, dokumenter, kollegaer…',
  autoFocus = false,
}: SearchInputProps) {
  const inputRef = useRef<HTMLInputElement>(null);

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    const q = value.trim();
    if (q) onSubmit(q);
  };

  const handleClear = () => {
    onChange('');
    inputRef.current?.focus();
  };

  return (
    <form onSubmit={handleSubmit} className="relative w-full">
      {/* Search icon */}
      <Search
        size={16}
        strokeWidth={1.5}
        className="pointer-events-none absolute left-4 top-1/2 -translate-y-1/2 text-[#C8C1B3]"
      />

      <input
        ref={inputRef}
        type="text"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder}
        autoFocus={autoFocus}
        className="w-full border border-[#D8D2C6] bg-[#F4F1EB] py-4 pl-11 pr-24 font-inter text-[15px] text-[#2B2B2B] outline-none transition-colors placeholder:text-[#C8C1B3] focus:border-[#2B2B2B] focus:bg-white"
        onKeyDown={(e) => {
          if (e.key === 'Enter') e.currentTarget.form?.requestSubmit();
        }}
      />

      {/* Actions */}
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
  );
}
