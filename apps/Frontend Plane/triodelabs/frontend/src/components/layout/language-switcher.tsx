"use client";

import { usePathname } from "next/navigation";
import { cn } from "@/lib/utils";
import { htmlLang, locales, type Locale } from "@/lib/i18n/locales";
import { getAlternatePath } from "@/lib/i18n/routes";

const languages: Record<Locale, { label: string; name: string }> = {
  nb: { label: "NO", name: "Norsk" },
  en: { label: "EN", name: "English" },
};

const switcherLabel: Record<Locale, string> = {
  nb: "Velg språk",
  en: "Choose language",
};

/**
 * Links to the same page in each language. Plain anchors on purpose: the two
 * languages use separate root layouts, so switching is a full page load.
 */
export function LanguageSwitcher({
  className,
  locale,
}: {
  className?: string;
  locale: Locale;
}) {
  const pathname = usePathname() ?? "/";

  return (
    <nav
      aria-label={switcherLabel[locale]}
      className={cn(
        "flex items-center gap-1 font-label text-[0.72rem] font-light uppercase leading-none tracking-[0.12em]",
        className,
      )}
    >
      {locales.map((target, index) => {
        const isCurrent = target === locale;

        return (
          <span className="flex items-center gap-1" key={target}>
            {index > 0 && (
              <span aria-hidden="true" className="opacity-35">
                /
              </span>
            )}
            <a
              aria-current={isCurrent ? "true" : undefined}
              aria-label={languages[target].name}
              className={cn(
                "inline-grid min-h-8 min-w-8 place-items-center rounded-sm transition-opacity focus-visible:outline focus-visible:outline-1 focus-visible:outline-offset-2 focus-visible:outline-current",
                isCurrent ? "opacity-100" : "opacity-50 hover:opacity-90",
              )}
              href={getAlternatePath(pathname, target)}
              hrefLang={htmlLang[target]}
              lang={htmlLang[target]}
            >
              {languages[target].label}
            </a>
          </span>
        );
      })}
    </nav>
  );
}
