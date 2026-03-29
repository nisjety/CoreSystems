import { Search } from 'lucide-react';

interface SearchInputProps {
  value: string;
  onChange: (value: string) => void;
  onSearch: (query: string) => void;
  placeholder?: string;
}

export function SearchInput({
  value,
  onChange,
  onSearch,
  placeholder = 'Søk i selskapets kunnskap...',
}: SearchInputProps) {
  return (
    <div className="relative">
      <Search
        className="absolute left-4 top-1/2 -translate-y-1/2 text-[#C0B8AF]"
        size={17}
      />
      <input
        type="text"
        placeholder={placeholder}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter' && value.trim()) {
            onSearch(value.trim());
          }
        }}
        className="w-full rounded-2xl border border-[#E5E0D8] bg-white py-4 pl-11 pr-4 text-[14px] text-[#1A1A1A] shadow-sm outline-none transition-colors placeholder:text-[#C0B8AF] focus:border-[#C0B8AF] focus:shadow-md"
      />
    </div>
  );
}
