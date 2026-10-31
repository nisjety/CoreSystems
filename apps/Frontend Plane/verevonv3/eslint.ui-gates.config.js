import { fileURLToPath } from 'node:url'
import solid from 'eslint-plugin-solid'
import tseslint from 'typescript-eslint'

// The UI copy and raw-colour gate for the chat and agents surfaces, and the
// restricted-syntax selectors that gate shares with eslint.config.js.
//
// Three consumers read this file:
//   - eslint.config.js applies the same selectors to UI_GATE_SCOPES, so
//     `pnpm lint` reports them;
//   - the default export is the standalone gate the spa-ui-gates CI job runs
//     (`eslint -c eslint.ui-gates.config.js src`);
//   - scripts/ui-gates-new-files.mjs applies the selectors to every src file a
//     change adds, with no baseline.
// Existing findings are recorded in eslint-suppressions.json. A baseline entry
// can only shrink: the ESLint CLI fails when an entry no longer matches, so a
// fixed leak is pruned in the same change with
// `pnpm exec eslint -c eslint.ui-gates.config.js --prune-suppressions src`.
// Never edit that file by hand.

// typescript-eslint >= 8.67 refuses to guess the tsconfig root when more than
// one candidate exists (this SPA and packages/remote-core each carry a
// tsconfig), failing every file with a parse error. Pin it to this directory.
const tsconfigRootDir = fileURLToPath(new URL('.', import.meta.url))

export const WEB_SPEECH_MESSAGE =
  'Browser speech APIs send audio and text to the browser vendor. Use Verevon’s governed speech path (features/voice: /api/v1/ai/dictate to record, speech-playback to read aloud).'
export const WEB_SPEECH_NAMES = ['SpeechRecognition', 'webkitSpeechRecognition', 'speechSynthesis', 'SpeechSynthesisUtterance']

// The no-restricted-globals and no-restricted-properties rules only see a bare
// global or a property read straight off `window`, `globalThis` or `self`.
// TypeScript reaches a vendor-prefixed API by casting the global or aliasing it
// first (`const win = window as …; win.webkitSpeechRecognition`), which is how
// the removed recogniser did it. These selectors catch the name wherever it
// appears: read off any object, as a string (`'speechSynthesis' in window`,
// `Reflect.get`, a computed key) or declared in an ad-hoc type shim.
const WEB_SPEECH_NAME_PATTERN = `/^(${WEB_SPEECH_NAMES.join('|')})$/`
const WEB_SPEECH_SYNTAX = [
  `MemberExpression[property.name=${WEB_SPEECH_NAME_PATTERN}]`,
  `Literal[value=${WEB_SPEECH_NAME_PATTERN}]`,
  `TemplateElement[value.cooked=${WEB_SPEECH_NAME_PATTERN}]`,
  `TSPropertySignature[key.name=${WEB_SPEECH_NAME_PATTERN}]`,
  `TSTypeReference[typeName.name=${WEB_SPEECH_NAME_PATTERN}]`,
].map((selector) => ({ selector, message: WEB_SPEECH_MESSAGE }))

// The selectors every product file gets: the fabrication guard and governed
// speech. A UI gate block replaces the no-restricted-syntax options for its
// files, so each block that applies to those files spreads this array first.
export const BASE_RESTRICTED_SYNTAX = [
  {
    // Phase 4 A1: a security/compliance-named declaration must NEVER render a
    // control as enabled from a boolean literal. Read real org state or show a
    // disabled/unknown state via the read-data substrate.
    selector: 'VariableDeclarator[id.name=/securit|complian|posture/i] Property > Literal[value=true]',
    message:
      'Fabricated security/compliance posture: a security control may not be hardcoded enabled from a boolean literal (Phase 4 A8). Read real org-security state or render a disabled/unknown state.',
  },
  ...WEB_SPEECH_SYNTAX,
]

// The globs end in the TypeScript extensions on purpose: a bare directory glob
// also makes ESLint lint the .mts files beneath it (chat-export/sample-exports.mts),
// which `pnpm lint` does not gate today.
export const UI_GATE_SCOPES = ['src/features/chat/**/*.{ts,tsx}', 'src/features/agents/**/*.{ts,tsx}']
export const TEST_FILE_IGNORES = ['**/*.test.{ts,tsx}', '**/*.spec.{ts,tsx}']

// The selectors below match literal values by regular expression. A word is a
// run of two or more Unicode letters, so Norwegian and English copy both count.
// The `u` flag is parsed by esquery 1.7, the selector parser ESLint 10 uses.
const WORDS = String.raw`\p{L}{2,}`
const NAMING_ATTRIBUTE = 'title|aria-label|aria-description|alt|placeholder'
const DATA_LABEL = 'label|title|text|message|detail|description|placeholder|heading|caption|tooltip|hint'
const NORWEGIAN_LETTERS = String.raw`[æøåÆØÅ]`
const COLOUR = String.raw`#[0-9a-fA-F]{3,8}\b|\b(rgb|rgba|hsl|hsla)\(`

const COPY_MESSAGE =
  'Bare copy in chat or agents: route it through i18n.tr(no, en) so Norwegian and English switch together.'
const NAMING_MESSAGE =
  'Accessible name is bare copy: route it through i18n.tr(no, en), since a screen reader reads this text in the active locale.'
const BRANCH_MESSAGE =
  'Bare copy in a conditional or logical JSX expression: route each branch through i18n.tr(no, en).'
const LABEL_MESSAGE =
  'Bare label in a data table: translate it where it renders with i18n.tr(no, en), not in the data.'
const NORWEGIAN_MESSAGE =
  'Norwegian text outside i18n.tr(no, en): the English locale would show it unchanged.'
const COLOUR_MESSAGE = 'Raw colour: use a design token (var(--name)) instead of a hex, rgb() or hsl() value.'

export const UI_COPY_SYNTAX = [
  { selector: `JSXText[value=/${WORDS}/u]`, message: COPY_MESSAGE },
  {
    selector: `JSXAttribute[name.name=/^(${NAMING_ATTRIBUTE})$/] > Literal[value=/${WORDS}/u]`,
    message: NAMING_MESSAGE,
  },
  {
    selector: `JSXAttribute[name.name=/^(${NAMING_ATTRIBUTE})$/] > JSXExpressionContainer > ConditionalExpression > Literal[value=/${WORDS}/u]`,
    message: NAMING_MESSAGE,
  },
  {
    selector: `:matches(JSXElement, JSXFragment) > JSXExpressionContainer > :matches(ConditionalExpression, LogicalExpression) > Literal[value=/${WORDS}/u]`,
    message: BRANCH_MESSAGE,
  },
  { selector: `Property[key.name=/^(${DATA_LABEL})$/] > Literal[value=/${WORDS}/u]`, message: LABEL_MESSAGE },
  {
    selector: `Literal[value=/${NORWEGIAN_LETTERS}/]:not(CallExpression[callee.property.name='tr'] > Literal)`,
    message: NORWEGIAN_MESSAGE,
  },
  {
    selector: `TemplateElement[value.cooked=/${NORWEGIAN_LETTERS}/]:not(CallExpression[callee.property.name='tr'] > TemplateLiteral > TemplateElement)`,
    message: NORWEGIAN_MESSAGE,
  },
]

export const UI_COLOR_SYNTAX = [
  { selector: `Literal[value=/${COLOUR}/]`, message: COLOUR_MESSAGE },
  { selector: `TemplateElement[value.cooked=/${COLOUR}/]`, message: COLOUR_MESSAGE },
]

export default tseslint.config(
  {
    // Product sources disable solid rules inline. Registering the plugin lets
    // those directives resolve here; this gate enables none of its rules.
    files: ['**/*.{ts,tsx}'],
    plugins: { solid },
    languageOptions: { parser: tseslint.parser, parserOptions: { tsconfigRootDir } },
    // Inline directives for the solid and no-console rules, which this gate
    // does not run, would otherwise be reported as unused on every run.
    linterOptions: { reportUnusedDisableDirectives: 'off' },
  },
  {
    files: UI_GATE_SCOPES,
    ignores: TEST_FILE_IGNORES,
    rules: {
      'no-restricted-syntax': ['error', ...BASE_RESTRICTED_SYNTAX, ...UI_COPY_SYNTAX, ...UI_COLOR_SYNTAX],
    },
  },
)
