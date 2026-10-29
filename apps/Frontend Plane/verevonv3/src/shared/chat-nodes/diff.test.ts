import { describe, expect, it } from 'vitest'
import { diffText, looksLikeUnifiedDiff, parseUnifiedDiff, MAX_DIFF_LINES } from './diff'

describe('diffText', () => {
  it('reports no hunks for identical text', () => {
    const result = diffText('a\nb\nc', 'a\nb\nc')
    expect(result.hunks).toEqual([])
    expect(result.stat).toEqual({ added: 0, removed: 0 })
    expect(result.truncated).toBe(false)
  })

  it('finds a single changed line with surrounding context', () => {
    const result = diffText('a\nb\nc', 'a\nB\nc')
    expect(result.stat).toEqual({ added: 1, removed: 1 })
    expect(result.hunks).toHaveLength(1)
    const kinds = result.hunks[0]!.lines.map((line) => line.kind)
    expect(kinds).toContain('removed')
    expect(kinds).toContain('added')
    expect(kinds).toContain('context')
  })

  it('counts pure additions and pure removals', () => {
    expect(diffText('a', 'a\nb\nc').stat).toEqual({ added: 2, removed: 0 })
    expect(diffText('a\nb\nc', 'a').stat).toEqual({ added: 0, removed: 2 })
    expect(diffText('', 'a\nb').stat).toEqual({ added: 2, removed: 0 })
    expect(diffText('a\nb', '').stat).toEqual({ added: 0, removed: 2 })
  })

  /**
   * A trailing newline on one side only must not read as a changed line — it is
   * the most common spurious diff in generated documents.
   */
  it('ignores a difference in trailing newline alone', () => {
    expect(diffText('a\nb', 'a\nb\n').stat).toEqual({ added: 0, removed: 0 })
    expect(diffText('a\nb\n', 'a\nb').hunks).toEqual([])
  })

  it('splits distant changes into separate hunks', () => {
    const before = ['1', '2', '3', '4', '5', '6', '7', '8', '9', '10', '11', '12'].join('\n')
    const after = ['1', 'X', '3', '4', '5', '6', '7', '8', '9', '10', 'Y', '12'].join('\n')
    const result = diffText(before, after)
    expect(result.hunks.length).toBe(2)
    expect(result.stat).toEqual({ added: 2, removed: 2 })
  })

  it('keeps adjacent changes in one hunk', () => {
    const result = diffText('1\n2\n3\n4', '1\nX\nY\n4')
    expect(result.hunks).toHaveLength(1)
  })

  /**
   * This runs during render. A very large revision must degrade to a stat-only
   * summary rather than block the tab on an O(n·m) table — and it must SAY it
   * degraded, or an empty hunk list reads as "no changes".
   */
  it('degrades honestly above the size cap instead of blocking', () => {
    const before = Array.from({ length: MAX_DIFF_LINES + 10 }, (_, i) => `line ${i}`).join('\n')
    const after = `${before}\nextra`
    const result = diffText(before, after)
    expect(result.truncated).toBe(true)
    expect(result.hunks).toEqual([])
    // A line-count delta, explicitly not presented as a real add/remove count.
    expect(result.stat.added).toBe(1)
  })

  it('hunk line numbers point into the right sides', () => {
    // Change on line 5 of both sides.
    const before = ['a', 'b', 'c', 'd', 'e', 'f', 'g'].join('\n')
    const after = ['a', 'b', 'c', 'd', 'E', 'f', 'g'].join('\n')
    const hunk = diffText(before, after).hunks[0]!
    expect(hunk.oldStart).toBe(2)
    expect(hunk.newStart).toBe(2)
    // The hunk covers the 3 leading context lines, the change, and the trailing
    // context — so its line counts must be > 1.
    expect(hunk.oldLines).toBeGreaterThan(1)
    expect(hunk.newLines).toBeGreaterThan(1)
  })
})

describe('looksLikeUnifiedDiff', () => {
  it('recognises a real patch by its hunk header', () => {
    expect(
      looksLikeUnifiedDiff('--- a/f\n+++ b/f\n@@ -1,3 +1,3 @@\n a\n-b\n+B\n c'),
    ).toBe(true)
    // A single-line hunk omits the counts.
    expect(looksLikeUnifiedDiff('@@ -1 +1 @@\n-a\n+b')).toBe(true)
  })

  /**
   * Deliberately strict. `+`/`-` prefixes alone match prose, markdown lists and
   * log output, and a wrong diff card is more confusing than a generic one.
   */
  it('does not mistake prose, lists or logs for a patch', () => {
    for (const text of [
      '- first item\n- second item\n+ a plus line',
      'ERROR - failed\nINFO + retried',
      '',
      '{"results":[]}',
      'a\n-b\n+c',
    ]) {
      expect(looksLikeUnifiedDiff(text)).toBe(false)
    }
  })
})

describe('parseUnifiedDiff', () => {
  it('parses hunks, counts and content from a tool-produced patch', () => {
    const patch = [
      '--- a/config.yml',
      '+++ b/config.yml',
      '@@ -1,4 +1,4 @@',
      ' name: svc',
      '-replicas: 1',
      '+replicas: 3',
      ' image: svc:1',
    ].join('\n')
    const result = parseUnifiedDiff(patch)
    expect(result.hunks).toHaveLength(1)
    expect(result.stat).toEqual({ added: 1, removed: 1 })
    const hunk = result.hunks[0]!
    expect(hunk.oldStart).toBe(1)
    expect(hunk.oldLines).toBe(4)
    expect(hunk.lines.find((line) => line.kind === 'added')?.text).toBe('replicas: 3')
  })

  /** `---`/`+++` are file headers, not changed lines. */
  it('does not count file headers as changes', () => {
    const patch = '--- a/f\n+++ b/f\n@@ -1 +1 @@\n-old\n+new'
    expect(parseUnifiedDiff(patch).stat).toEqual({ added: 1, removed: 1 })
  })

  it('handles multiple hunks and an omitted line count', () => {
    const patch = ['@@ -1 +1 @@', '-a', '+A', '@@ -10,2 +10,2 @@', ' x', '-y', '+Y'].join('\n')
    const result = parseUnifiedDiff(patch)
    expect(result.hunks).toHaveLength(2)
    expect(result.hunks[0]!.oldLines).toBe(1)
    expect(result.hunks[1]!.oldStart).toBe(10)
    expect(result.stat).toEqual({ added: 2, removed: 2 })
  })

  it('returns nothing for text with no hunk header', () => {
    expect(parseUnifiedDiff('just some output').hunks).toEqual([])
  })
})
