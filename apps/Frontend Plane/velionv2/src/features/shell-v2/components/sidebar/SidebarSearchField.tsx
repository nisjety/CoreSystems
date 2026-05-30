"use client";

import { Search } from "lucide-react";
import { sidebarType } from "@/features/shell-v2/lib/sidebar-style";
import { cn } from "@/lib/utils";

export function SidebarSearchField({
  ariaLabel,
  className,
  placeholder = "Filter this section",
  value,
  onChange,
}: {
  ariaLabel: string;
  className?: string;
  placeholder?: string;
  value: string;
  onChange: (value: string) => void;
}) {
  return (
    <div className={cn("relative", className)}>
      <Search className="velion-sidebar-search-icon" strokeWidth={1.8} />
      <input
        type="search"
        value={value}
        onChange={(event) => onChange(event.target.value)}
        placeholder={placeholder}
        autoComplete="off"
        autoCorrect="off"
        autoCapitalize="none"
        spellCheck={false}
        className={cn(
          "velion-sidebar-search-input",
          sidebarType.input,
        )}
        aria-label={ariaLabel}
      />
    </div>
  );
}
