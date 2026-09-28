/**
 * Types shared by every module under `chat-export/`.
 *
 * Deliberately its own file with zero runtime code: `docx.ts`, `xlsx.ts` and
 * `pptx.ts` need `MarkdownBlock` from `chat-types.ts`, and that file (through
 * `chat-media-markdown.tsx`'s sibling `chat-types.ts` imports) sits next to
 * modules that pull in solid-js JSX. As long as every import of it here is
 * `import type`, `verbatimModuleSyntax` erases the import entirely — nothing
 * is actually loaded at runtime, in the browser bundle or under a plain
 * `node --experimental-strip-types` run of `sample-exports.mts`. Keeping the
 * exported types in one small file makes that invariant easy to see and easy
 * to keep true.
 */

/** One entry of a turn's citation list, as the export menu offers it. */
export type ExportSource = {
  title: string
  url?: string
}

/**
 * Norwegian-first, matching the rest of the chat UI's `i18n.tr('norsk',
 * 'English')` convention. The builders below have no `i18n` context of their
 * own (they are plain data transforms, not Solid components) — the caller
 * decides once, from the live UI locale, and passes the answer down here.
 */
export type ExportLocale = 'no' | 'en'

/** What every export format needs beyond the parsed markdown body. */
export type ExportMeta = {
  title: string
  /** ISO 8601. Defaults to "now" wherever a builder needs a date and this is absent. */
  createdAt?: string
  sources?: ExportSource[]
  locale?: ExportLocale
}
