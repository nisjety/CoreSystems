/**
 * Dependency-free syntax highlighting for `code` artifacts.
 *
 * This project ships NO highlighter (no shiki / prism / highlight.js in
 * package.json, and the Markdown code-fence renderer emits a plain `<pre><code>`),
 * so rather than pull a heavyweight dependency into the SPA bundle for one panel
 * we tokenize with a small sticky-regex scanner. It is deliberately "basic":
 * comments, strings, numbers, keywords, and punctuation per language family —
 * enough to make generated source readable, with line numbers, and nothing that
 * pretends to be a parser.
 *
 * Output is line-oriented (`CodeLine[]`) so the viewer can render a gutter, and
 * multi-line constructs (block comments, template/triple-quoted strings) are
 * split across lines while keeping their token type.
 */

export type CodeTokenType = 'comment' | 'keyword' | 'number' | 'plain' | 'punctuation' | 'string' | 'tag'

export type CodeToken = {
  text: string
  type: CodeTokenType
}

export type CodeLine = {
  /** 1-based line number for the gutter. */
  number: number
  tokens: CodeToken[]
}

export type CodeFamily = 'c-like' | 'css' | 'markup' | 'plain' | 'python' | 'shell' | 'sql' | 'yaml'

/**
 * Past this many characters the scanner is skipped entirely and the source is
 * returned as plain lines. A model can emit a very large file; a regex scan of
 * hundreds of KB inside a render would visibly stall the panel, and unstyled
 * monospace text is a perfectly honest fallback.
 */
export const MAX_HIGHLIGHT_CHARS = 120_000

type Rule = {
  pattern: RegExp
  /** `identifier` resolves to `keyword` or `plain` via the language's keyword set. */
  type: CodeTokenType | 'identifier'
}

const LANGUAGE_FAMILIES: Record<string, CodeFamily> = {
  bash: 'shell', c: 'c-like', cpp: 'c-like', csharp: 'c-like', css: 'css',
  dart: 'c-like', go: 'c-like', graphql: 'c-like', html: 'markup', java: 'c-like',
  javascript: 'c-like', json: 'c-like', kotlin: 'c-like', lua: 'c-like',
  php: 'c-like', python: 'python', r: 'c-like', ruby: 'python', rust: 'c-like',
  scala: 'c-like', shell: 'shell', sql: 'sql', swift: 'c-like', toml: 'yaml',
  typescript: 'c-like', xml: 'markup', yaml: 'yaml', zsh: 'shell',
}

const C_LIKE_KEYWORDS = new Set([
  'abstract', 'as', 'async', 'await', 'break', 'case', 'catch', 'class', 'const',
  'constructor', 'continue', 'crate', 'declare', 'default', 'defer', 'delete',
  'do', 'else', 'enum', 'export', 'extends', 'extern', 'false', 'final',
  'finally', 'fn', 'for', 'from', 'func', 'function', 'go', 'if', 'impl',
  'implements', 'import', 'in', 'instanceof', 'interface', 'let', 'loop',
  'match', 'mod', 'mut', 'namespace', 'new', 'nil', 'null', 'override',
  'package', 'private', 'protected', 'pub', 'public', 'range', 'readonly',
  'record', 'ref', 'return', 'satisfies', 'sealed', 'self', 'static', 'struct',
  'super', 'switch', 'this', 'throw', 'trait', 'true', 'try', 'type', 'typeof',
  'undefined', 'union', 'unsafe', 'use', 'var', 'virtual', 'void', 'where',
  'while', 'with', 'yield',
])

const PYTHON_KEYWORDS = new Set([
  'and', 'as', 'assert', 'async', 'await', 'begin', 'break', 'class', 'continue',
  'def', 'del', 'do', 'elif', 'else', 'end', 'except', 'False', 'finally', 'for',
  'from', 'global', 'if', 'import', 'in', 'is', 'lambda', 'module', 'None',
  'nonlocal', 'not', 'or', 'pass', 'raise', 'require', 'return', 'self', 'True',
  'try', 'unless', 'until', 'while', 'with', 'yield',
])

const SHELL_KEYWORDS = new Set([
  'case', 'cd', 'do', 'done', 'echo', 'elif', 'else', 'esac', 'exit', 'export',
  'fi', 'for', 'function', 'if', 'in', 'local', 'read', 'return', 'set', 'shift',
  'source', 'then', 'unset', 'until', 'while',
])

const SQL_KEYWORDS = new Set([
  'all', 'alter', 'and', 'as', 'asc', 'between', 'by', 'case', 'cast', 'create',
  'cross', 'delete', 'desc', 'distinct', 'drop', 'else', 'end', 'exists', 'from',
  'full', 'group', 'having', 'in', 'index', 'inner', 'insert', 'into', 'is',
  'join', 'left', 'like', 'limit', 'not', 'null', 'offset', 'on', 'or', 'order',
  'outer', 'primary', 'right', 'select', 'set', 'table', 'then', 'union',
  'update', 'using', 'values', 'view', 'when', 'where', 'with',
])

const CSS_KEYWORDS = new Set([
  '!important', 'and', 'from', 'important', 'not', 'only', 'to',
])

const YAML_KEYWORDS = new Set(['false', 'no', 'null', 'off', 'on', 'true', 'yes'])

const JSON_KEYWORDS = new Set(['false', 'null', 'true'])

const KEYWORDS: Record<CodeFamily, Set<string>> = {
  'c-like': C_LIKE_KEYWORDS,
  css: CSS_KEYWORDS,
  markup: new Set<string>(),
  plain: new Set<string>(),
  python: PYTHON_KEYWORDS,
  shell: SHELL_KEYWORDS,
  sql: SQL_KEYWORDS,
  yaml: YAML_KEYWORDS,
}

const NUMBER_RULE: Rule = {
  pattern: /(?:0[xXbBoO][0-9a-fA-F_]+|\d[\d_]*(?:\.\d[\d_]*)?(?:[eE][+-]?\d+)?)/y,
  type: 'number',
}

const IDENTIFIER_RULE: Rule = { pattern: /[A-Za-z_$][A-Za-z0-9_$]*/y, type: 'identifier' }

const PUNCTUATION_RULE: Rule = { pattern: /[{}()[\].,;:?!<>=+\-*/%&|^~@#\\]+/y, type: 'punctuation' }

const DOUBLE_QUOTED: Rule = { pattern: /"(?:\\[\s\S]|[^"\\\n])*"?/y, type: 'string' }
const SINGLE_QUOTED: Rule = { pattern: /'(?:\\[\s\S]|[^'\\\n])*'?/y, type: 'string' }

const RULES: Record<CodeFamily, Rule[]> = {
  'c-like': [
    { pattern: /\/\/[^\n]*/y, type: 'comment' },
    { pattern: /\/\*[\s\S]*?(?:\*\/|$)/y, type: 'comment' },
    { pattern: /`(?:\\[\s\S]|[^`\\])*`?/y, type: 'string' },
    DOUBLE_QUOTED,
    SINGLE_QUOTED,
    NUMBER_RULE,
    IDENTIFIER_RULE,
    PUNCTUATION_RULE,
  ],
  css: [
    { pattern: /\/\*[\s\S]*?(?:\*\/|$)/y, type: 'comment' },
    DOUBLE_QUOTED,
    SINGLE_QUOTED,
    { pattern: /[.#][A-Za-z_-][\w-]*/y, type: 'tag' },
    { pattern: /@[A-Za-z-]+/y, type: 'keyword' },
    { pattern: /(?:\d+\.?\d*)(?:px|rem|em|%|vh|vw|s|ms|deg|fr)?/y, type: 'number' },
    IDENTIFIER_RULE,
    PUNCTUATION_RULE,
  ],
  markup: [
    { pattern: /<!--[\s\S]*?(?:-->|$)/y, type: 'comment' },
    { pattern: /<[!?/]?[A-Za-z][\w:.-]*/y, type: 'tag' },
    DOUBLE_QUOTED,
    SINGLE_QUOTED,
    NUMBER_RULE,
    IDENTIFIER_RULE,
    { pattern: /[<>/=&;{}()[\].,:?!+\-*%|^~@#]+/y, type: 'punctuation' },
  ],
  plain: [],
  python: [
    { pattern: /#[^\n]*/y, type: 'comment' },
    { pattern: /[rbfuRBFU]{0,2}"""[\s\S]*?(?:"""|$)/y, type: 'string' },
    { pattern: /[rbfuRBFU]{0,2}'''[\s\S]*?(?:'''|$)/y, type: 'string' },
    { pattern: /[rbfuRBFU]{0,2}"(?:\\[\s\S]|[^"\\\n])*"?/y, type: 'string' },
    { pattern: /[rbfuRBFU]{0,2}'(?:\\[\s\S]|[^'\\\n])*'?/y, type: 'string' },
    { pattern: /@[A-Za-z_][\w.]*/y, type: 'tag' },
    NUMBER_RULE,
    IDENTIFIER_RULE,
    PUNCTUATION_RULE,
  ],
  shell: [
    { pattern: /#[^\n]*/y, type: 'comment' },
    DOUBLE_QUOTED,
    SINGLE_QUOTED,
    { pattern: /\$(?:\{[^}\n]*\}?|[A-Za-z_][\w]*|[0-9@*#?$!])/y, type: 'tag' },
    { pattern: /(?:^|\s)-{1,2}[A-Za-z][\w-]*/y, type: 'number' },
    IDENTIFIER_RULE,
    PUNCTUATION_RULE,
  ],
  sql: [
    { pattern: /--[^\n]*/y, type: 'comment' },
    { pattern: /\/\*[\s\S]*?(?:\*\/|$)/y, type: 'comment' },
    { pattern: /'(?:''|[^'\n])*'?/y, type: 'string' },
    { pattern: /"(?:[^"\n])*"?/y, type: 'string' },
    NUMBER_RULE,
    IDENTIFIER_RULE,
    PUNCTUATION_RULE,
  ],
  yaml: [
    { pattern: /#[^\n]*/y, type: 'comment' },
    DOUBLE_QUOTED,
    SINGLE_QUOTED,
    { pattern: /[A-Za-z_][\w.\- ]*(?=\s*:)/y, type: 'tag' },
    NUMBER_RULE,
    IDENTIFIER_RULE,
    PUNCTUATION_RULE,
  ],
}

/** Language id → scanner family. Unknown languages fall back to plain text. */
export function codeFamilyForLanguage(language: string): CodeFamily {
  return LANGUAGE_FAMILIES[language.trim().toLowerCase()] ?? 'plain'
}

function keywordSetFor(language: string, family: CodeFamily): Set<string> {
  if (language.trim().toLowerCase() === 'json') return JSON_KEYWORDS
  return KEYWORDS[family]
}

/**
 * Tokenizes `source` into per-line tokens. Never throws and always accounts for
 * every character, so `lines.flatMap(tokens).map(text).join('\n')` reproduces
 * the input exactly.
 */
export function highlightCode(source: string, language: string): CodeLine[] {
  const normalized = source.replace(/\r\n/g, '\n')
  const family = codeFamilyForLanguage(language)
  const rules = RULES[family]
  if (rules.length === 0 || normalized.length > MAX_HIGHLIGHT_CHARS) {
    return toLines(normalized.length > 0 ? [{ text: normalized, type: 'plain' }] : [])
  }
  return toLines(scan(normalized, rules, keywordSetFor(language, family)))
}

function scan(source: string, rules: Rule[], keywords: Set<string>): CodeToken[] {
  const tokens: CodeToken[] = []
  let index = 0
  let plainStart = 0

  const flushPlain = (end: number) => {
    if (end > plainStart) tokens.push({ text: source.slice(plainStart, end), type: 'plain' })
  }

  while (index < source.length) {
    let matched: { length: number; type: CodeTokenType } | null = null
    for (const rule of rules) {
      rule.pattern.lastIndex = index
      const result = rule.pattern.exec(source)
      if (!result || result[0].length === 0) continue
      const type = rule.type === 'identifier'
        ? (keywords.has(result[0]) ? 'keyword' : 'plain')
        : rule.type
      matched = { length: result[0].length, type }
      break
    }
    if (!matched) {
      index += 1
      continue
    }
    // A resolved-to-plain identifier is merged into the surrounding plain run so
    // the DOM stays small; only styled tokens become their own element.
    if (matched.type === 'plain') {
      index += matched.length
      continue
    }
    flushPlain(index)
    tokens.push({ text: source.slice(index, index + matched.length), type: matched.type })
    index += matched.length
    plainStart = index
  }
  flushPlain(source.length)
  return tokens
}

function toLines(tokens: CodeToken[]): CodeLine[] {
  let current: CodeLine = { number: 1, tokens: [] }
  const lines: CodeLine[] = [current]
  for (const token of tokens) {
    const segments = token.text.split('\n')
    segments.forEach((segment, segmentIndex) => {
      if (segmentIndex > 0) {
        current = { number: lines.length + 1, tokens: [] }
        lines.push(current)
      }
      if (segment.length > 0) current.tokens.push({ text: segment, type: token.type })
    })
  }
  // A trailing newline produces one empty line no editor would show.
  if (lines.length > 1 && current.tokens.length === 0) lines.pop()
  return lines
}
