import type { EnTranslations } from './translations/en';
import type { NoTranslations } from './translations/no';

export const languages = ['en', 'no'] as const;
export type Language = typeof languages[number];

export interface TranslationBundle {
  en: EnTranslations;
  no: NoTranslations;
}

export type PathImpl<T, P extends string> = T extends object
  ? { [K in keyof T]: K extends string
      ? T[K] extends object
        ? PathImpl<T[K], `${P}${K}.`> | `${P}${K}`
        : `${P}${K}`
      : never }[keyof T]
  : never;

export type TranslationPaths = PathImpl<EnTranslations, ''>;

export type ReplacePlaceholders<S extends string> = S extends `${string}{{${string}}}${infer Rest}` ? ReplacePlaceholders<Rest> : S;

export type TranslationValue<Path extends string> = Path extends keyof EnTranslations ? EnTranslations[Path] : string;

export function resolvePath<T extends Record<string, unknown>, R = unknown>(obj: T, path: string): R | undefined {
  return path.split('.').reduce<unknown | undefined>((acc, key) => (acc && typeof acc === 'object' ? (acc as Record<string, unknown>)[key] : undefined), obj) as R | undefined;
}
