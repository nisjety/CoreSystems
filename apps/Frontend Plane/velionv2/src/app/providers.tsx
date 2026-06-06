"use client";

import { ThemeProvider } from "@/lib/theme/theme-provider";
import { LocaleProvider } from "@/lib/i18n/locale-context";
import { QueryProvider } from "@/lib/net/query-provider";
import { ConvexClientProvider } from "@/lib/convex/convex-provider";

export function Providers({ children }: { children: React.ReactNode }) {
  return (
    <ThemeProvider defaultTheme="system" disableTransitionOnChange>
      <QueryProvider>
        <LocaleProvider>
          <ConvexClientProvider>{children}</ConvexClientProvider>
        </LocaleProvider>
      </QueryProvider>
    </ThemeProvider>
  );
}
