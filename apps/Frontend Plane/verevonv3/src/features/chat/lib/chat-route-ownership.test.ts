/**
 * Chat owns its own route.
 *
 * Other surfaces used to link sideways into `/chat` — an "Åpne i Chat" link in
 * the Inbox rail, a Space-activity fallback, and five `/chat` shortcuts spread
 * across unrelated sidebar sections. Two problems followed. Chat became the
 * app's dumping ground, and a `/chat?thread_id=` link ADOPTS a thread Chat does
 * not own: since those threads' `origin` is not `chat`, the origin guard in
 * `initializeChat` marks them foreign and renders them read-only, so the link
 * promised the full workspace and delivered a transcript you could not
 * continue.
 *
 * A surface that wants Verevon's help LAUNCHES a chat of its own instead —
 * `writePendingChatLaunch` (Ticketing, Dashboard search) creates a real
 * `origin: 'chat'` thread and is unaffected by this rule.
 *
 * Enforced as a test rather than an ESLint rule because `pnpm lint` cannot run
 * in this repo today (`typescript-eslint` does not support the pinned
 * TypeScript 7), and an unenforceable rule is worse than none. Reading source
 * text follows the precedent in `mid-run-input.test.ts`.
 */

import { describe, expect, it } from 'vitest'

/** Deep-linking a thread into Chat. This is the adoption we forbid. */
const THREAD_ADOPTION = /['"`]\/chat\?thread_id=/

async function sourceFiles(): Promise<Array<{ path: string; text: string }>> {
  const { readdirSync, readFileSync, statSync } = await import('node:fs')
  const { join, resolve } = await import('node:path')
  const root = resolve(process.cwd(), 'src')
  const out: Array<{ path: string; text: string }> = []
  const walk = (dir: string) => {
    for (const entry of readdirSync(dir)) {
      const full = join(dir, entry)
      if (statSync(full).isDirectory()) {
        walk(full)
        continue
      }
      if (!/\.tsx?$/.test(entry) || /\.test\.tsx?$/.test(entry)) continue
      // The path separator is written via char code so no backslash literal
      // appears in this file (a heredoc collapsed the escape once already).
      const sep = String.fromCharCode(92)
      const relative = full.slice(root.length + 1).split(sep).join('/')
      out.push({ path: relative, text: readFileSync(full, 'utf8') })
    }
  }
  walk(root)
  return out
}

describe('chat route ownership', () => {
  it('no surface outside the chat feature deep-links a thread into /chat', async () => {
    const offenders = (await sourceFiles())
      .filter((file) => !file.path.startsWith('features/chat/'))
      .filter((file) => THREAD_ADOPTION.test(file.text))
      .map((file) => file.path)

    expect(
      offenders,
      'a `/chat?thread_id=` link adopts a thread Chat does not own and renders read-only; '
        + 'launch a real chat with `writePendingChatLaunch` instead',
    ).toEqual([])
  })

  it('the sidebar offers exactly one Chat destination', async () => {
    const files = await sourceFiles()
    const nav = files.find((file) => file.path === 'features/core/lib/sidebar-navigation.ts')
    expect(nav, 'sidebar-navigation.ts moved; update this guard').toBeDefined()
    const hrefs = nav!.text.match(/href: '\/chat'/g) ?? []
    expect(
      hrefs.length,
      'Chat is one destination. Five shortcuts in unrelated sections made the rail '
        + 'imply five different chats.',
    ).toBe(1)
  })
})
