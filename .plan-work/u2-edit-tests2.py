import os
os.chdir("C:/dev/CoresSystem/.claude/worktrees/practical-mccarthy-1dc567/apps/Frontend Plane/verevonv3/src/features/spaces")

p = "lib/space-thread-presentation.test.ts"
b = open(p, "rb").read().decode("utf-8")
crlf = "\r\n" in b
b = b.replace("\r\n", "\n")
b = b.replace("  LIVE_RUN_STATUSES,\n  threadStatus,\n} from './space-thread-presentation'",
              "  LIVE_RUN_STATUSES,\n  formatRelativeWhen,\n  threadStatus,\n} from './space-thread-presentation'", 1)
b = b.rstrip("\n") + """

describe('formatRelativeWhen (reply summary)', () => {
  const now = Date.parse('2026-10-28T12:00:00Z')
  const ago = (seconds: number) => new Date(now - seconds * 1000).toISOString()

  it('says "now" inside a minute, then minutes, then hours, then days', () => {
    expect(formatRelativeWhen(ago(10), now)).toMatch(/nå|nå/i)
    expect(formatRelativeWhen(ago(4 * 60), now)).toMatch(/4\\s*min/)
    expect(formatRelativeWhen(ago(3 * 3600), now)).toMatch(/3\\s*t/)
    expect(formatRelativeWhen(ago(2 * 86_400), now)).toMatch(/2\\s*d/)
  })

  it('falls back to the plain date from a week on', () => {
    expect(formatRelativeWhen(ago(10 * 86_400), now)).toMatch(/18\\.\\s*okt/)
  })

  it('reads an absent or unreadable time as empty, never as a wrong one', () => {
    expect(formatRelativeWhen(undefined, now)).toBe('')
    expect(formatRelativeWhen('', now)).toBe('')
    expect(formatRelativeWhen('not a date', now)).toBe('')
  })
})
"""
if crlf:
    b = b.replace("\n", "\r\n")
open(p, "wb").write(b.encode("utf-8"))
print("ok")
