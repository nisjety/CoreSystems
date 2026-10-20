import { readdirSync, readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { describe, expect, it } from 'vitest'

// A catch site that reads `<error>.message` can put the raw text of a failure in
// front of the person: English operator wording, an upstream sentence, or a status
// line such as "Failed to fetch". The sites that matter were converted to
// `translateApiError(reason, i18n.tr, { no, en })`, which answers by the gateway code
// or the status class. This scan keeps them converted: a source file may only hold
// the number of raw-message reads listed below, so reverting a converted site, or
// writing a new one, fails here. A file that drops below its number must lower it
// (the test says so), so the list can only shrink.
//
// What counts as a read: any `.message` on an identifier that ends in err, error,
// reason, cause, failure or caught (case as written: `err`, `connError`, `loadError`,
// `query.error`, `result.error?.message`), or on a bare `e`. It does not matter
// whether the read sits in an `instanceof Error ?`, an `instanceof ApiError ?`, a
// `setX(err.message)` or a `return err.message`: all of those are the same thing.
// Comment lines are not read. This is a name heuristic, not a type check: a read
// through another name (`const text = fault.message`) is not seen, and a payload
// read that happens to end in `error` is counted and has to be listed with a reason.
//
// Round-5 review: the first version of this scan saw only `instanceof Error ? x.message`
// and `instanceof Error) return x.message`, so the `instanceof ApiError`,
// `setPickError(err.message)` and `return error.message` shapes passed unseen.
// Of the sites that shape hid, PluginsSection, DashboardComposer (pick error),
// RouterPolicyPage, NexiCheckout and StudioPage were converted (WorkspaceSettingsPage
// was tried and put back: it shows an integration's own sentence on purpose);
// the rest are listed here with what is known about each.

const RAW_MESSAGE_READ =
  /\b[A-Za-z0-9_$.]*(?:err|Err|error|Error|reason|Reason|cause|Cause|failure|Failure|caught)\??\.message\b|\be\??\.message\b/g

/** Counts the reads in one source text, comment lines excluded. */
function countRawMessageReads(source: string): number {
  const code = source
    .split(/\r?\n/)
    .filter((line) => !/^\s*(\/\/|\*|\/\*)/.test(line))
    .join('\n')
  return (code.match(RAW_MESSAGE_READ) ?? []).length
}

const UNREVIEWED =
  'not part of the sites converted in rounds 3 to 5; not yet reviewed for whether the raw text can reach a person'

const NOT_CONVERTED: Record<string, { sites: number; reason: string }> = {
  'features/agents/lib/use-chatbot-support-status.ts': { sites: 1, reason: UNREVIEWED },
  'features/auth/components/AuthPage.tsx': {
    sites: 6,
    reason:
      "shows Better Auth's own English message; auth-core owns that code set and the SPA has no copy table for it yet (open work, see the codes list section 7)",
  },
  'features/chat/components/use-chat-controller.ts': {
    sites: 3,
    reason:
      'two approval/denial catch sites are unreviewed; the third reads the stream failure text that streamFailurePresentation keys by code and only shows for a code it has no copy for',
  },
  'features/core/components/OrgDeletionBanner.tsx': {
    sites: 1,
    reason:
      'shows the backend sentence on purpose: the org-deletion replies are flat text without a code and the component tests pin them; converting needs a code from the backend first',
  },
  'features/dashboard/home/BrowserTimelineDetail.tsx': { sites: 1, reason: UNREVIEWED },
  'features/dashboard/home/DashboardCards.tsx': { sites: 3, reason: UNREVIEWED },
  'features/dashboard/home/DashboardComposer.tsx': {
    sites: 3,
    reason:
      'the knowledge import notice, the send failure and the gateway-load failure line are unreviewed (the pick error at the same place was converted in round 5)',
  },
  'features/inbox/components/InboxPage.tsx': {
    sites: 1,
    reason:
      'reviewed: safeConnectionStatusMessage shows the message only for three listed codes, under 180 characters and without a URL',
  },
  'features/ingestions/components/VerevonIngestionsPage.tsx': { sites: 7, reason: UNREVIEWED },
  'features/settings/components/OrgDeletionDangerZone.tsx': {
    sites: 1,
    reason:
      'shows the backend sentence on purpose: the org-name mismatch reply is flat text without a code and the component test pins it; converting needs a code from the backend first',
  },
  'features/settings/components/TwoFactorEnrollmentSection.tsx': {
    sites: 2,
    reason:
      "shows Better Auth's own English message for a wrong password or code; auth-core owns that code set and the SPA has no copy table for it yet",
  },
  'features/settings/components/WorkspaceSettingsPage.tsx': {
    sites: 2,
    reason:
      "shows the integration's own sentence for an integration_error on purpose (the library-picker test pins it); translateApiError has no copy for that code by design, so a conversion would hide it",
  },
  'features/spaces/components/SpaceRoomComposer.tsx': { sites: 1, reason: UNREVIEWED },
  'features/support/components/RemoteSupportPage.tsx': {
    sites: 4,
    reason: 'remote-core results and events carry their own English text; unreviewed (the page has pre-existing type errors)',
  },
  'features/voice/VoiceModeDialog.tsx': { sites: 1, reason: UNREVIEWED },
  'shared/api/ag-ui-client.ts': {
    sites: 2,
    reason:
      'a connection failure on the agent stream becomes RUN_ERROR with code connection_error, which the controller treats as a lost transport; the text is unreviewed',
  },
  'shared/api/chat-client.ts': {
    sites: 5,
    reason:
      'the chat stream connection failure (code connection_error, pinned by chat-client.test) and the mid-run refusal message carry the ApiError text; unreviewed for what a person sees',
  },
  'shared/api/http.ts': {
    sites: 3,
    reason:
      "reviewed: this is the transport itself reading the gateway envelope's message field into ApiError.message (one line); describeApiError and translateApiError never show that text",
  },
  'shared/api/knowledge-client.ts': { sites: 1, reason: UNREVIEWED },
  'shared/api/search-client.ts': { sites: 1, reason: UNREVIEWED },
}

const srcRoot = resolve(process.cwd(), 'src')

function sourceFiles(dir: string): string[] {
  const files: string[] = []
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = `${dir}/${entry.name}`
    if (entry.isDirectory()) files.push(...sourceFiles(path))
    else if (/\.tsx?$/.test(entry.name) && !/\.test\.tsx?$/.test(entry.name)) files.push(path)
  }
  return files.sort()
}

const found = new Map<string, number>()
for (const path of sourceFiles(srcRoot)) {
  const count = countRawMessageReads(readFileSync(path, 'utf8'))
  if (count > 0) found.set(path.slice(srcRoot.length + 1), count)
}

describe('the raw-message read pattern', () => {
  it('sees every shape a catch site can use, not only `instanceof Error ? x.message`', () => {
    const shapes = [
      "err instanceof ApiError ? err.message : 'Kunne ikke.'",
      "if (err instanceof ApiError) return err.message",
      "setPickError(err.message)",
      "message: error.message,",
      "return { outcome: 'refused', message: error.message }",
      "(err) => handlers.onError?.(err instanceof Error ? err.message : 'x')",
      "const m = connError instanceof Error ? connError.message : 'Connection failed'",
      "setErrorMessage(result.error?.message ?? 'x')",
      "{items.error instanceof Error ? items.error.message : 'x'}",
      "next.on('error', (e) => show(e.message))",
    ]
    for (const shape of shapes) expect(countRawMessageReads(shape), shape).toBeGreaterThanOrEqual(1)
  })

  it('does not count comments, payload reads or a translated message', () => {
    expect(countRawMessageReads('// shows err.message directly')).toBe(0)
    expect(countRawMessageReads(' * the raw error.message is never shown')).toBe(0)
    expect(countRawMessageReads('const text = payload.message ?? data.message')).toBe(0)
    expect(countRawMessageReads("setError(translateApiError(err, i18n.tr, { no: 'a', en: 'b' }))")).toBe(0)
  })
})

describe('catch sites that show the raw text of a failure', () => {
  it('has read the sources', () => {
    expect(found.size).toBeGreaterThan(5)
    // Converted files read clean.
    for (const converted of [
      'features/finetune/components/FinetuneJobsPage.tsx',
      'features/settings/components/PluginsSection.tsx',
      'features/router-policy/components/RouterPolicyPage.tsx',
      'features/billing/components/NexiCheckout.tsx',
      'features/studio/components/StudioPage.tsx',
    ]) {
      expect(found.has(converted), `${converted} reads clean`).toBe(false)
    }
  })

  it('has no file with more raw-message reads than it is listed with', () => {
    const over = [...found]
      .filter(([file, count]) => count > (NOT_CONVERTED[file]?.sites ?? 0))
      .map(([file, count]) => `${file}: ${count} (allowed ${NOT_CONVERTED[file]?.sites ?? 0}); use translateApiError(reason, i18n.tr, { no, en })`)
    expect(over, 'a catch site shows err.message instead of translateApiError').toEqual([])
  })

  it('keeps the list honest: a file that dropped below its number lowers it or leaves the list', () => {
    const stale = Object.entries(NOT_CONVERTED)
      .filter(([file, entry]) => (found.get(file) ?? 0) < entry.sites)
      .map(([file, entry]) => `${file}: listed ${entry.sites}, found ${found.get(file) ?? 0}`)
    expect(stale).toEqual([])
    for (const [file, entry] of Object.entries(NOT_CONVERTED)) {
      expect(entry.reason.length, `${file} names its reason`).toBeGreaterThan(20)
    }
  })
})
