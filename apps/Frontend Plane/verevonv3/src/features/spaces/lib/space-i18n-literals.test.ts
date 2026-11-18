import { readdirSync, readFileSync } from 'node:fs';
import { basename, join, relative, resolve } from 'node:path';

import * as ts from 'typescript';
import { describe, expect, it } from 'vitest';

// WS16-S1: a ratchet on Norwegian copy written straight into the spaces sources. A Norwegian string
// belongs in a `tr(no, en)` call or in space-copy.ts, where the English twin lives beside it. This scan
// counts the string literals (and template text) that carry a Norwegian letter and sit outside any
// `tr(...)` call. The count may fall; it must not rise. Comments and JSX text are not string literals
// and are not counted. The run cwd is the package root (the same root the other source-scan tests use).

const SPACES_ROOT = resolve(process.cwd(), 'src/features/spaces');
// Measured on the tree when this ratchet was written. Lower it when the count falls; never raise it.
const BASELINE = 77;

const NORWEGIAN_LETTER = /[æøåÆØÅ]/;
const TEST_FILE = /\.(test|spec)\.tsx?$/;

type LiteralNode = ts.StringLiteral | ts.NoSubstitutionTemplateLiteral | ts.TemplateHead | ts.TemplateMiddle | ts.TemplateTail;

function isLiteralNode(node: ts.Node): node is LiteralNode {
  return (
    ts.isStringLiteral(node) ||
    ts.isNoSubstitutionTemplateLiteral(node) ||
    ts.isTemplateHead(node) ||
    ts.isTemplateMiddle(node) ||
    ts.isTemplateTail(node)
  );
}

/** True when the node sits anywhere inside a `tr(...)` or `something.tr(...)` call. */
function insideTranslatorCall(node: ts.Node): boolean {
  for (let parent = node.parent; parent; parent = parent.parent) {
    if (ts.isCallExpression(parent)) {
      const callee = parent.expression;
      if (ts.isIdentifier(callee) && callee.text === 'tr') return true;
      if (ts.isPropertyAccessExpression(callee) && callee.name.text === 'tr') return true;
    }
  }
  return false;
}

/** How many literals in one source carry a Norwegian letter and are not inside a translator call. */
function norwegianLiteralCount(fileName: string, text: string): number {
  const kind = fileName.endsWith('.tsx') ? ts.ScriptKind.TSX : ts.ScriptKind.TS;
  const source = ts.createSourceFile(fileName, text, ts.ScriptTarget.Latest, true, kind);
  let count = 0;
  const visit = (node: ts.Node): void => {
    if (isLiteralNode(node) && NORWEGIAN_LETTER.test(node.text) && !insideTranslatorCall(node)) count += 1;
    ts.forEachChild(node, visit);
  };
  visit(source);
  return count;
}

function sourceFiles(dir: string): string[] {
  const found: string[] = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) found.push(...sourceFiles(path));
    else if (/\.tsx?$/.test(entry.name)) found.push(path);
  }
  return found;
}

/** The non-test spaces sources the ratchet covers: every .ts and .tsx file except space-copy.ts. */
function scannedFiles(): string[] {
  return sourceFiles(SPACES_ROOT)
    .filter((path) => !TEST_FILE.test(path) && basename(path) !== 'space-copy.ts')
    .sort();
}

function countsByFile(): Map<string, number> {
  const counts = new Map<string, number>();
  for (const path of scannedFiles()) {
    const count = norwegianLiteralCount(path, readFileSync(path, 'utf8'));
    if (count > 0) counts.set(relative(SPACES_ROOT, path).split('\\').join('/'), count);
  }
  return counts;
}

describe('Norwegian literals outside tr() (WS16-S1)', () => {
  it('counts a Norwegian literal and skips tr() calls, comments and English-only text', () => {
    const source = [
      "// Åpne i en kommentar, ikke en streng",
      "const a = 'Åpne';",
      "const b = i18n.tr('Åpne', 'Open');",
      "const c = tr('Å lagre', 'Save');",
      'const d = `Å ${name}`;',
      "const e = 'Open';",
    ].join('\n');
    expect(norwegianLiteralCount('sample.ts', source)).toBe(2);
  });

  it('counts a Norwegian attribute value in a .tsx file', () => {
    expect(norwegianLiteralCount('sample.tsx', "const el = <button title='Åpne'>Open</button>;")).toBe(1);
  });

  it('scans the spaces sources, and only those (a wrong root would count nothing)', () => {
    const files = scannedFiles();
    expect(files.length).toBeGreaterThan(100);
    expect(files.some((path) => basename(path) === 'space-copy.ts')).toBe(false);
    expect(files.some((path) => TEST_FILE.test(path))).toBe(false);
  });

  it('stays at or below the checked-in baseline: the count may fall, it must not rise', () => {
    const counts = countsByFile();
    const total = [...counts.values()].reduce((sum, count) => sum + count, 0);
    const worst = [...counts].sort((a, b) => b[1] - a[1]).slice(0, 15);
    const detail = worst.map(([path, count]) => `  ${count}  ${path}`).join('\n');
    expect(
      total,
      `Norwegian literals outside tr() rose above the baseline of ${BASELINE}. Move the new copy into tr(no, en).\n${detail}`,
    ).toBeLessThanOrEqual(BASELINE);
  });
});
