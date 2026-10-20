import { readdirSync, readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { describe, expect, it } from 'vitest'

// A catch site that shows `reason instanceof Error ? reason.message : 'fallback'`
// puts the raw text of the failure in front of the person: English operator wording,
// an upstream sentence, or a status line. The sites that matter were converted to
// `translateApiError(reason, i18n.tr, { no, en })`, which answers by the gateway code
// or the status class. This scan keeps them converted: a source file may only hold the
// number of raw-message sites listed below, so reverting a converted site, or writing
// a new one, fails here. A file that drops below its number must lower it (the test
// says so), so the list can only shrink.
//
// Sites that remain are listed per file. None of them has been reviewed yet for whether
// the raw text can reach a person: converting them (or writing down why the raw text is
// right there) is open work, and each conversion lowers a number here.

const RAW_MESSAGE_SITE =
  /instanceof\s+Error\s*\?\s*[A-Za-z_.]+\.message|instanceof\s+Error\)\s*return\s+[A-Za-z_.]+\.message/g

const UNREVIEWED = 'not part of the 44 sites converted in round 3; not yet reviewed for whether the raw text can reach a person'

const NOT_CONVERTED: Record<string, { sites: number; reason: string }> = {
  'features/agents/lib/use-chatbot-support-status.ts': {
    sites: 1,
    reason: UNREVIEWED,
  },
  'features/auth/components/AuthPage.tsx': {
    sites: 6,
    reason: UNREVIEWED,
  },
  'features/billing/components/NexiCheckout.tsx': {
    sites: 1,
    reason: UNREVIEWED,
  },
  'features/chat/components/use-chat-controller.ts': {
    sites: 2,
    reason: UNREVIEWED,
  },
  'features/core/components/OrgDeletionBanner.tsx': {
    sites: 1,
    reason: UNREVIEWED,
  },
  'features/dashboard/home/BrowserTimelineDetail.tsx': {
    sites: 1,
    reason: UNREVIEWED,
  },
  'features/dashboard/home/DashboardCards.tsx': {
    sites: 3,
    reason: UNREVIEWED,
  },
  'features/dashboard/home/DashboardComposer.tsx': {
    sites: 3,
    reason: UNREVIEWED,
  },
  'features/ingestions/components/VerevonIngestionsPage.tsx': {
    sites: 7,
    reason: UNREVIEWED,
  },
  'features/router-policy/components/RouterPolicyPage.tsx': {
    sites: 1,
    reason: UNREVIEWED,
  },
  'features/settings/components/OrgDeletionDangerZone.tsx': {
    sites: 1,
    reason: UNREVIEWED,
  },
  'features/settings/components/TwoFactorEnrollmentSection.tsx': {
    sites: 1,
    reason: UNREVIEWED,
  },
  'features/settings/components/WorkspaceSettingsPage.tsx': {
    sites: 2,
    reason: UNREVIEWED,
  },
  'features/spaces/components/SpaceRoomComposer.tsx': {
    sites: 1,
    reason: UNREVIEWED,
  },
  'features/studio/components/StudioPage.tsx': {
    sites: 2,
    reason: UNREVIEWED,
  },
  'features/support/components/RemoteSupportPage.tsx': {
    sites: 1,
    reason: UNREVIEWED,
  },
  'features/voice/VoiceModeDialog.tsx': {
    sites: 1,
    reason: UNREVIEWED,
  },
  'shared/api/ag-ui-client.ts': {
    sites: 2,
    reason: UNREVIEWED,
  },
  'shared/api/chat-client.ts': {
    sites: 2,
    reason: UNREVIEWED,
  },
  'shared/api/knowledge-client.ts': {
    sites: 1,
    reason: UNREVIEWED,
  },
  'shared/api/search-client.ts': {
    sites: 1,
    reason: UNREVIEWED,
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
for (const path of sourceFiles(srcRoot)) {
  const count = (readFileSync(path, 'utf8').match(RAW_MESSAGE_SITE) ?? []).length
  if (count > 0) found.set(path.slice(srcRoot.length + 1), count)
}

describe('catch sites that show the raw text of a failure', () => {
  it('has read the sources', () => {
    expect(found.size).toBeGreaterThan(5)
    // A converted file reads clean.
    expect(found.has('features/finetune/components/FinetuneJobsPage.tsx')).toBe(false)
  })

  it('has no file with more raw-message sites than it is listed with', () => {
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
