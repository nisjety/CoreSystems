"use client";

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type Dispatch,
  type SetStateAction,
} from "react";

export type Theme = "dark" | "light" | "system";
type ResolvedTheme = "dark" | "light";

type ThemeContextValue = {
  resolvedTheme: ResolvedTheme;
  setTheme: Dispatch<SetStateAction<Theme>>;
  systemTheme: ResolvedTheme;
  theme: Theme;
};

const ThemeContext = createContext<ThemeContextValue | null>(null);
const themes = new Set<Theme>(["dark", "light", "system"]);

function resolveTheme(theme: Theme, systemTheme: ResolvedTheme) {
  return theme === "system" ? systemTheme : theme;
}

function readStoredTheme(storageKey: string, fallback: Theme) {
  try {
    const stored = window.localStorage.getItem(storageKey);
    return stored && themes.has(stored as Theme) ? (stored as Theme) : fallback;
  } catch {
    return fallback;
  }
}

function writeStoredTheme(storageKey: string, theme: Theme) {
  try {
    window.localStorage.setItem(storageKey, theme);
  } catch {
    // Local storage can be unavailable in private or restricted contexts.
  }
}

function getSystemTheme(): ResolvedTheme {
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

function withoutTransitions(callback: () => void) {
  const style = document.createElement("style");
  style.appendChild(
    document.createTextNode(
      "*,*::before,*::after{transition:none!important}",
    ),
  );
  document.head.appendChild(style);
  callback();
  window.getComputedStyle(document.body);
  window.setTimeout(() => {
    style.remove();
  }, 1);
}

export function ThemeProvider({
  children,
  defaultTheme = "system",
  disableTransitionOnChange = true,
  storageKey = "theme",
}: {
  children: React.ReactNode;
  defaultTheme?: Theme;
  disableTransitionOnChange?: boolean;
  storageKey?: string;
}) {
  const [theme, setThemeState] = useState<Theme>(defaultTheme);
  const [systemTheme, setSystemTheme] = useState<ResolvedTheme>("light");

  useEffect(() => {
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const handleChange = () => setSystemTheme(getSystemTheme());
    const hydrationTimer = window.setTimeout(() => {
      setThemeState(readStoredTheme(storageKey, defaultTheme));
      setSystemTheme(getSystemTheme());
    }, 0);

    media.addEventListener("change", handleChange);
    return () => {
      window.clearTimeout(hydrationTimer);
      media.removeEventListener("change", handleChange);
    };
  }, [defaultTheme, storageKey]);

  const setTheme = useCallback<Dispatch<SetStateAction<Theme>>>(
    (nextTheme) => {
      setThemeState((currentTheme) => {
        const resolvedNext =
          typeof nextTheme === "function" ? nextTheme(currentTheme) : nextTheme;
        const safeTheme = themes.has(resolvedNext) ? resolvedNext : defaultTheme;
        writeStoredTheme(storageKey, safeTheme);
        return safeTheme;
      });
    },
    [defaultTheme, storageKey],
  );

  const resolvedTheme = resolveTheme(theme, systemTheme);

  useEffect(() => {
    const applyTheme = () => {
      document.documentElement.classList.toggle("dark", resolvedTheme === "dark");
      document.documentElement.style.colorScheme = resolvedTheme;
    };

    if (disableTransitionOnChange) {
      withoutTransitions(applyTheme);
      return;
    }

    applyTheme();
  }, [disableTransitionOnChange, resolvedTheme]);

  const value = useMemo<ThemeContextValue>(
    () => ({
      resolvedTheme,
      setTheme,
      systemTheme,
      theme,
    }),
    [resolvedTheme, setTheme, systemTheme, theme],
  );

  return <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>;
}

export function useTheme() {
  const context = useContext(ThemeContext);
  if (!context) {
    throw new Error("useTheme must be used within ThemeProvider.");
  }

  return context;
}
