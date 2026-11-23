import { readdirSync, readFileSync } from 'node:fs';
import { basename, join, relative, resolve } from 'node:path';

import * as ts from 'typescript';
import { describe, expect, it } from 'vitest';

// WS16-S1: a ratchet on Norwegian copy written straight into the spaces sources. A Norwegian string
// belongs in a `tr(no, en)` call, in space-copy.ts, or beside its English twin in one of the pair shapes
// `hasEnglishTwin` recognises. This scan counts the string literals (and template text) that carry a
// Norwegian letter, sit outside any `tr(...)` call, and have no English twin beside them. The count may
// fall; it must not rise. Comments and JSX text are not string literals and are not counted. The run cwd
// is the package root (the same root the other source-scan tests use).

const SPACES_ROOT = resolve(process.cwd(), 'src/features/spaces');
// Measured on the tree after the English twins were recognised (77 before, when every pair was counted).
// The one literal left is the typed slash-command alias 'søk' in space-slash-commands.ts: input vocabulary the
// parser matches (beside 'search', which the composer displays, and 'sok'), not copy, so it is no pair. Lower it
// when the count falls; never raise it.
const BASELINE = 1;

// The calls that take a Norwegian text followed by its English twin as the very next argument. Only these count as
// a call pair: any other call with a Norwegian argument is counted, whatever stands beside it.
const PAIR_CALLEES: ReadonlySet<string> = new Set(['p', 'translate', 'tabCommand', 'limitError', 'failureText']);

// The tables of `key: [no, en]` rows. Only a two-element array that is a row of one of these counts as an array pair.
const PAIR_TABLES: ReadonlySet<string> = new Set(['ACTION_VERBS']);

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

/** A whole text: a string, a template with no substitutions, or a template expression (its parts joined). */
type TextNode = ts.StringLiteral | ts.NoSubstitutionTemplateLiteral | ts.TemplateExpression;

function textOf(node: TextNode): string {
  return ts.isTemplateExpression(node)
    ? [node.head.text, ...node.templateSpans.map((span) => span.literal.text)].join('')
    : node.text;
}

function isTextNode(node: ts.Node): node is TextNode {
  return ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node) || ts.isTemplateExpression(node);
}

/** The English side of a pair: a text that is not empty and carries no Norwegian letter. */
function isEnglishText(node: ts.Node | undefined): boolean {
  if (node === undefined || !isTextNode(node)) return false;
  const text = textOf(node);
  return text.trim().length > 0 && !NORWEGIAN_LETTER.test(text);
}

/** The English twin's name for a Norwegian name: `no` gives `en`, `labelNo` gives `labelEn`, `TITLE_NO` gives `TITLE_EN`. */
function englishNameFor(norwegianName: string): string | undefined {
  if (norwegianName === 'no') return 'en';
  if (norwegianName.endsWith('_NO')) return `${norwegianName.slice(0, -3)}_EN`;
  if (/[a-z]No$/.test(norwegianName)) return `${norwegianName.slice(0, -2)}En`;
  return undefined;
}

/** The name of an object-literal property, when it is a plain identifier or string key. */
function propertyKey(property: ts.ObjectLiteralElementLike): string | undefined {
  if (!ts.isPropertyAssignment(property)) return undefined;
  return ts.isIdentifier(property.name) || ts.isStringLiteral(property.name) ? property.name.text : undefined;
}

/** The called name: `p` for `p(...)`, `tr` for `i18n.tr(...)`. */
function calleeName(callee: ts.Expression): string | undefined {
  if (ts.isIdentifier(callee)) return callee.text;
  if (ts.isPropertyAccessExpression(callee)) return callee.name.text;
  return undefined;
}

/** True when the array is the value of one row of an object literal assigned to a variable named in `PAIR_TABLES`. */
function isPairTableRow(array: ts.ArrayLiteralExpression): boolean {
  const row = array.parent;
  if (!ts.isPropertyAssignment(row) || row.initializer !== array || !ts.isObjectLiteralExpression(row.parent)) return false;
  const table = row.parent.parent;
  return ts.isVariableDeclaration(table) && ts.isIdentifier(table.name) && PAIR_TABLES.has(table.name.text);
}

/**
 * True when a Norwegian text sits beside its English twin, in one of the pair shapes the app writes: an object
 * with a `no` (or `labelNo`) key and its `en` (or `labelEn`) key; an argument of one of the `PAIR_CALLEES` whose
 * next argument is the English text, as in `p(no, en)` or `translate(no, en)`; a `[no, en]` row of one of the
 * `PAIR_TABLES`; or a `const X_NO` declared beside a `const X_EN` in the same file. Any other neighbour, even an
 * ASCII one, is not a twin.
 */
function hasEnglishTwin(text: TextNode, variables: ReadonlyMap<string, ts.Expression | undefined>): boolean {
  const parent = text.parent;
  if (ts.isPropertyAssignment(parent) && ts.isObjectLiteralExpression(parent.parent)) {
    const wanted = englishNameFor(propertyKey(parent) ?? '');
    return (
      wanted !== undefined &&
      parent.parent.properties.some(
        (property) => ts.isPropertyAssignment(property) && propertyKey(property) === wanted && isEnglishText(property.initializer),
      )
    );
  }
  if (ts.isCallExpression(parent)) {
    const name = calleeName(parent.expression);
    if (name === undefined || !PAIR_CALLEES.has(name)) return false;
    const args: readonly ts.Node[] = parent.arguments;
    return isEnglishText(args[args.indexOf(text) + 1]);
  }
  if (ts.isArrayLiteralExpression(parent)) {
    return parent.elements.length === 2 && parent.elements[0] === text && isEnglishText(parent.elements[1]) && isPairTableRow(parent);
  }
  if (ts.isVariableDeclaration(parent) && ts.isIdentifier(parent.name)) {
    const wanted = englishNameFor(parent.name.text);
    return wanted !== undefined && isEnglishText(variables.get(wanted));
  }
  return false;
}

/** The file's named variables and their initial values, so a `const X_NO` can find its `X_EN` beside it. */
function variableInitializers(source: ts.SourceFile): Map<string, ts.Expression | undefined> {
  const found = new Map<string, ts.Expression | undefined>();
  const visit = (node: ts.Node): void => {
    if (ts.isVariableDeclaration(node) && ts.isIdentifier(node.name)) found.set(node.name.text, node.initializer);
    ts.forEachChild(node, visit);
  };
  visit(source);
  return found;
}

/** The whole text a literal belongs to: every part of a template belongs to the one template expression. */
function textExpressionOf(literal: LiteralNode): TextNode {
  if (ts.isTemplateHead(literal)) return literal.parent as ts.TemplateExpression;
  if (ts.isTemplateMiddle(literal) || ts.isTemplateTail(literal)) return (literal.parent as ts.TemplateSpan).parent;
  return literal;
}

/**
 * How many literals in one source carry a Norwegian letter, sit outside any translator call, and have no English
 * twin beside them (see `hasEnglishTwin`).
 */
function norwegianLiteralCount(fileName: string, text: string): number {
  const kind = fileName.endsWith('.tsx') ? ts.ScriptKind.TSX : ts.ScriptKind.TS;
  const source = ts.createSourceFile(fileName, text, ts.ScriptTarget.Latest, true, kind);
  const variables = variableInitializers(source);
  let count = 0;
  const visit = (node: ts.Node): void => {
    if (
      isLiteralNode(node) &&
      NORWEGIAN_LETTER.test(node.text) &&
      !insideTranslatorCall(node) &&
      !hasEnglishTwin(textExpressionOf(node), variables)
    ) {
      count += 1;
    }
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

  it('does not count a Norwegian text with its English twin beside it, in any of the pair shapes', () => {
    const source = [
      "const a = { no: 'Åpne', en: 'Open' };",
      "const b = { labelNo: 'Søk', labelEn: 'Search' };",
      "const c = p('Åpne', 'Open');",
      "const ACTION_VERBS: Record<string, [string, string]> = { navigate: ['Åpnet side', 'Opened page'] };",
      "const E_NO = 'Åpne';",
      "const E_EN = 'Open';",
      'const f = translate(`Åpne ${n}`, `Open ${n}`);',
      "const g = i18n.p('Lukk', 'Close');",
      "const h = tabCommand('work', ['work', 'arbeid'], 'arbeid', 'Gå til Arbeid', 'Go to Work');",
      "const k = failureText(error, 'Filen ble ikke fjernet.', 'The file was not removed.');",
      "const l = limitError('Gruppen finnes allerede.', 'Section already exists.');",
    ].join('\n');
    expect(norwegianLiteralCount('sample.ts', source)).toBe(0);
  });

  it('counts a Norwegian text beside an ASCII neighbour outside the known pair shapes', () => {
    const source = [
      "el.setAttribute('title', 'Åpne');",
      "const a = g('Åpne', 'Lagre');",
      "const b = ['Lukk', 'Åpne'];",
      "const c = ['Åpnet side', 'Opened page'];",
      "const OTHER = { navigate: ['Åpnet side', 'Opened page'] };",
      "const d = p('Open', 'Åpne');",
      "const ACTION_VERBS = { navigate: ['Opened page', 'Åpnet side'] };",
    ].join('\n');
    // Each line holds one Norwegian text with no recognised twin: a callee outside PAIR_CALLEES, an array that is
    // not a PAIR_TABLES row, an English text before (not after) the Norwegian one, and a table row in en-no order.
    expect(norwegianLiteralCount('sample.ts', source)).toBe(7);
  });

  it('counts a Norwegian text whose neighbour is not an English twin', () => {
    const source = [
      "const a = { no: 'Åpne', hidden: true };",
      "const b = { no: 'Åpne', en: 'Åpne igjen' };",
      "const c = { labelNo: 'Søk' };",
      "const d = ['Åpne', 'Søk', 'Lukk'];",
      "const e = f('Åpne', g);",
      "const h = p('Åpne', '');",
    ].join('\n');
    // a: 1. b: both Norwegian halves, 2. c: 1. d: the two Norwegian names of a three-name list, 2. e: 1.
    // h: an empty string is no English twin, 1.
    expect(norwegianLiteralCount('sample.ts', source)).toBe(8);
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
