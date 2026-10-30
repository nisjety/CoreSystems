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

// Round-6 review: `setError(String(reason))` (an Error prints as "Error: <English message>")
// and `${reason}` in a template read the same raw text without the word `.message`, and
// two such sites in the chat organizer sat outside every list. Both shapes count now.
// `String(x)` counts for every catch name above; the template shape counts only for a
// bare `${err}`, `${error}`, `${e}`, `${cause}`, `${caught}` or `${failure}`, because
// `${reason}` and `${proposal.reason}` are mostly a domain field (a triage reason, a
// label), not a caught value. A message that is passed as the FALLBACK copy of translateApiError
// (`{ no: event.message, en: event.message }`) is a third way to put it on screen:
// it is counted separately, in RAW_MESSAGE_AS_FALLBACK below.
const CATCH_NAME = '[A-Za-z0-9_$.]*(?:err|Err|error|Error|reason|Reason|cause|Cause|failure|Failure|caught)'
const RAW_MESSAGE_READ = new RegExp(
  [
    `\\b${CATCH_NAME}\\??\\.message\\b`,
    '\\be\\??\\.message\\b',
    `\\bString\\(\\s*(?:${CATCH_NAME}|e)\\s*\\)`,
    '\\$\\{\\s*(?:err|error|e|cause|caught|failure)\\s*\\}',
  ].join('|'),
  'g',
)
const RAW_MESSAGE_AS_FALLBACK = /\b(?:no|en):\s*[A-Za-z0-9_$.?]+\.message\b/g

function withoutCommentLines(source: string): string {
  return source
    .split(/\r?\n/)
    .filter((line) => !/^\s*(\/\/|\*|\/\*)/.test(line))
    .join('\n')
}

/** Counts the reads in one source text, comment lines excluded. */
function countRawMessageReads(source: string): number {
  return (withoutCommentLines(source).match(RAW_MESSAGE_READ) ?? []).length
}

/** Counts `{ no: x.message, en: x.message }` fallbacks: the same raw text, in both languages. */
function countRawMessageAsFallback(source: string): number {
  return (withoutCommentLines(source).match(RAW_MESSAGE_AS_FALLBACK) ?? []).length
}

const UNREVIEWED =
  'not part of the sites converted in rounds 3 to 5; not yet reviewed for whether the raw text can reach a person'

const NOT_CONVERTED: Record<string, { sites: number; reason: string }> = {
  'demo/ScenePage.tsx': {
    sites: 2,
    reason: 'the product-scene runtime (VITE_SCENE_MODE only) logs a scene failure for the capture operator; it is not a person-facing surface and is not in a normal build',
  },
  'demo/install.ts': {
    sites: 1,
    reason: 'the product-scene runtime (VITE_SCENE_MODE only) writes a failure of its own fixture API into the scene log for the capture operator; not in a normal build',
  },
  'features/agents/components/AgentRunConsole.tsx': {
    sites: 1,
    reason:
      'a zero-retention refusal is shown from the catalogue (zdr-refusals.ts); any other agent-stream failure still shows the stream error text, unreviewed for what a person sees',
  },
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
  'features/spaces/components/SpaceRoomComposer.tsx': {
    sites: 2,
    reason: `${UNREVIEWED} (one line holds both error.message and String(error), the abort or failure of a send)`,
  },
  'features/support/components/RemoteSupportPage.tsx': {
    sites: 5,
    reason: 'remote-core results and events carry their own English text, one site reads String(error); unreviewed (the page has pre-existing type errors)',
  },
  'features/voice/VoiceModeDialog.tsx': { sites: 1, reason: UNREVIEWED },
  'shared/api/ag-ui-client.ts': {
    sites: 2,
    reason:
      'a connection failure on the agent stream becomes RUN_ERROR with code connection_error, which the controller treats as a lost transport; the text is unreviewed',
  },
  'shared/api/chat-client.ts': {
    sites: 6,
    reason:
      'the chat stream connection failure (code connection_error, pinned by chat-client.test) and the mid-run refusal message carry the ApiError text; unreviewed for what a person sees',
  },
  'shared/api/http.ts': {
    sites: 3,
    reason:
      "reviewed: this is the transport itself reading the gateway envelope's message field into ApiError.message (one line); describeApiError and translateApiError never show that text",
  },
  'shared/api/marketing/algolia-collection-client.ts': {
    sites: 1,
    reason:
      `reviewed: no caught error is read; \`String(x.reason)\` / \`String(x.refreshReason)\` stringifies a payload enumeration field before an \`includes\` check on the response shape (the name heuristic matches the field name \`reason\`)`,
  },
  'shared/api/marketing/outcomes-client.ts': {
    sites: 1,
    reason:
      `reviewed: no caught error is read; \`String(x.reason)\` / \`String(x.refreshReason)\` stringifies a payload enumeration field before an \`includes\` check on the response shape (the name heuristic matches the field name \`reason\`)`,
  },
  'shared/api/marketing/pagespeed-client.ts': {
    sites: 2,
    reason:
      `reviewed: no caught error is read; \`String(x.reason)\` / \`String(x.refreshReason)\` stringifies a payload enumeration field before an \`includes\` check on the response shape (the name heuristic matches the field name \`reason\`)`,
  },
  'shared/api/marketing/pulse-client.ts': {
    sites: 1,
    reason:
      `reviewed: no caught error is read; \`String(x.reason)\` / \`String(x.refreshReason)\` stringifies a payload enumeration field before an \`includes\` check on the response shape (the name heuristic matches the field name \`reason\`)`,
  },
  'shared/api/marketing/search-comparison-client.ts': {
    sites: 1,
    reason:
      `reviewed: no caught error is read; \`String(x.reason)\` / \`String(x.refreshReason)\` stringifies a payload enumeration field before an \`includes\` check on the response shape (the name heuristic matches the field name \`reason\`)`,
  },
  'shared/api/marketing/trends-client.ts': {
    sites: 1,
    reason:
      `reviewed: no caught error is read; \`String(x.reason)\` / \`String(x.refreshReason)\` stringifies a payload enumeration field before an \`includes\` check on the response shape (the name heuristic matches the field name \`reason\`)`,
  },
  'shared/api/knowledge-client.ts': { sites: 1, reason: UNREVIEWED },
  'shared/api/search-client.ts': { sites: 1, reason: UNREVIEWED },
  'shared/api/visibility/research-client.ts': {
    sites: 1,
    reason:
      `reviewed: no caught error is read; \`String(x.reason)\` / \`String(x.refreshReason)\` stringifies a payload enumeration field before an \`includes\` check on the response shape (the name heuristic matches the field name \`reason\`)`,
  },
}

// Fallback copy that is the raw message of a stream event, on purpose: the room shows the
// server's own sentence for a run error whose code has no copy, rather than a generic line
// that hides what happened. Each entry counts the `no:` and `en:` matches.
const RAW_MESSAGE_AS_FALLBACK_LISTED: Record<string, { sites: number; reason: string }> = {
  'features/spaces/components/SpaceRoomComposer.tsx': {
    sites: 2,
    reason:
      'onError of the Space run stream: a code with copy gets the room words, an unknown code keeps the model gateway event message in both languages on purpose (comment at the site)',
  },
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
const foundFallback = new Map<string, number>()
for (const path of sourceFiles(srcRoot)) {
  const text = readFileSync(path, 'utf8')
  const count = countRawMessageReads(text)
  if (count > 0) found.set(path.slice(srcRoot.length + 1), count)
  const fallback = countRawMessageAsFallback(text)
  if (fallback > 0) foundFallback.set(path.slice(srcRoot.length + 1), fallback)
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
      "} catch (reason) { setError(String(reason)) }",
      "} catch (error) { setNotice(String(error)) }",
      "setNotice(`Failed: ${err}`)",
      "catch (e) { show(String(e)) }",
    ]
    for (const shape of shapes) expect(countRawMessageReads(shape), shape).toBeGreaterThanOrEqual(1)
  })

  it('does not count comments, payload reads or a translated message', () => {
    expect(countRawMessageReads('// shows err.message directly')).toBe(0)
    expect(countRawMessageReads(' * the raw error.message is never shown')).toBe(0)
    expect(countRawMessageReads('const text = payload.message ?? data.message')).toBe(0)
    expect(countRawMessageReads("setError(translateApiError(err, i18n.tr, { no: 'a', en: 'b' }))")).toBe(0)
    expect(countRawMessageReads('const label = String(item.id)')).toBe(0)
    expect(countRawMessageReads('const label = `${item.name}`')).toBe(0)
    expect(countRawMessageReads('return reason ? `${label} - ${reason}` : label')).toBe(0)
  })

  it('sees a message used as the fallback copy of both languages', () => {
    expect(countRawMessageAsFallback('translateApiError({ code }, tr, { no: event.message, en: event.message })')).toBe(2)
    expect(countRawMessageAsFallback('{ no: payload?.message, en: "x" }')).toBe(1)
    expect(countRawMessageAsFallback("{ no: 'Kunne ikke.', en: 'Could not.' }")).toBe(0)
    expect(countRawMessageAsFallback('// { no: event.message, en: event.message }')).toBe(0)
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

  it('lists every fallback that is a raw message, with its reason, and no more', () => {
    const differing = [...new Set([...foundFallback.keys(), ...Object.keys(RAW_MESSAGE_AS_FALLBACK_LISTED)])]
      .filter((file) => (foundFallback.get(file) ?? 0) !== (RAW_MESSAGE_AS_FALLBACK_LISTED[file]?.sites ?? 0))
      .map((file) => `${file}: found ${foundFallback.get(file) ?? 0}, listed ${RAW_MESSAGE_AS_FALLBACK_LISTED[file]?.sites ?? 0}`)
    expect(differing).toEqual([])
    for (const [file, entry] of Object.entries(RAW_MESSAGE_AS_FALLBACK_LISTED)) {
      expect(entry.reason.length, `${file} names its reason`).toBeGreaterThan(20)
    }
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
