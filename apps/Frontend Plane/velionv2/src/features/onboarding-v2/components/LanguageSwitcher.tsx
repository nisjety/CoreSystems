"use client";

/**
 * Locale switcher wired to the shared `useLocale` provider. Used in the
 * onboarding top bar (and reusable on the auth page). NO/EN only.
 */

import { useState } from "react";
import { Check, ChevronDown, Globe } from "lucide-react";
import { useLocale, type Locale } from "@/lib/i18n/locale-context";
import { cn } from "@/lib/utils";

const LANGUAGES: { code: Locale; name: string; flag: string }[] = [
  { code: "nb", name: "Norsk", flag: "🇳🇴" },
  { code: "en", name: "English", flag: "🇬🇧" },
];

export function LanguageSwitcher({
  size = "sm",
  className,
}: {
  size?: "sm" | "md";
  className?: string;
}) {
  const { currentLocale, setLocale } = useLocale();
  const [open, setOpen] = useState(false);

  return (
    <div className={cn("relative flex items-center", className)}>
      <button
        type="button"
        aria-label="Bytt språk"
        aria-expanded={open}
        aria-haspopup="true"
        onClick={() => setOpen((value) => !value)}
        className={cn(
          "inline-flex items-center gap-2 rounded-lg bg-white/80 font-medium text-[#777169] shadow-sm backdrop-blur-sm transition-colors hover:text-[#1C1C1C] hover:shadow-md focus:outline-none focus-visible:ring-2 focus-visible:ring-[#191716]/30",
          size === "sm" ? "p-2 text-xs" : "p-2.5 text-sm",
        )}
      >
        <Globe className="size-4" />
        <span className="uppercase tracking-[0.08em]">{currentLocale}</span>
        <ChevronDown className={cn("size-3.5 transition-transform", open ? "rotate-180" : "rotate-0")} />
      </button>
      {open ? (
        <>
          <button
            type="button"
            aria-hidden="true"
            tabIndex={-1}
            className="fixed inset-0 z-[290] cursor-default"
            onClick={() => setOpen(false)}
          />
          <div className="absolute right-0 top-10 z-[300] min-w-[150px] rounded-lg border border-[#E7E5E4] bg-white py-1.5 shadow-lg">
            {LANGUAGES.map((language) => {
              const selected = currentLocale === language.code;
              return (
                <button
                  key={language.code}
                  type="button"
                  onClick={() => {
                    setLocale(language.code);
                    setOpen(false);
                  }}
                  className={cn(
                    "flex w-full items-center gap-3 px-4 py-2.5 text-sm transition-colors hover:bg-[#F5F4F2]",
                    selected ? "text-[#1C1C1C]" : "text-[#777169]",
                  )}
                >
                  <span className="text-base">{language.flag}</span>
                  <span className="flex-1 text-left">{language.name}</span>
                  {selected ? <Check className="size-4" /> : null}
                </button>
              );
            })}
          </div>
        </>
      ) : null}
    </div>
  );
}
