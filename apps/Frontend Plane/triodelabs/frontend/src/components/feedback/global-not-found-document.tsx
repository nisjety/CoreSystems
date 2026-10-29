"use client";

import { usePathname } from "next/navigation";
import { htmlLang } from "@/lib/i18n/locales";
import { getLocaleFromPathname } from "@/lib/i18n/routes";
import { NotFoundView } from "./status-pages";

export function GlobalNotFoundDocument() {
  const locale = getLocaleFromPathname(usePathname() ?? "/");

  return (
    <html lang={htmlLang[locale]}>
      <body className="font-sans antialiased relative">
        <NotFoundView locale={locale} />
      </body>
    </html>
  );
}
