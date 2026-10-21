import { fileURLToPath } from 'node:url'
import solid from 'eslint-plugin-solid'
import tseslint from 'typescript-eslint'
import {
  BASE_RESTRICTED_SYNTAX,
  TEST_FILE_IGNORES,
  UI_COLOR_SYNTAX,
  UI_COPY_SYNTAX,
  UI_GATE_SCOPES,
  WEB_SPEECH_MESSAGE,
  WEB_SPEECH_NAMES,
} from './eslint.ui-gates.config.js'

// typescript-eslint >= 8.67 refuses to guess the tsconfig root when more than
// one candidate exists (this SPA and packages/remote-core each carry a
// tsconfig), failing every file with a parse error. Pin it to this directory.
const tsconfigRootDir = fileURLToPath(new URL('.', import.meta.url))

const WEB_SPEECH_GLOBALS = WEB_SPEECH_NAMES.map((name) => ({ name, message: WEB_SPEECH_MESSAGE }))
const WEB_SPEECH_PROPERTIES = ['window', 'globalThis', 'self'].flatMap((object) =>
  WEB_SPEECH_NAMES.map((property) => ({ object, property, message: WEB_SPEECH_MESSAGE })),
)

export default tseslint.config(
  {
    // `apps/*` are separate sub-projects (the Rust gateway, and any sub-app)
    // with their own tooling — Verevon v3's lint only governs its own SPA `src`.
    // `.claude/worktrees/**` holds nested full-repo checkouts (git worktrees for
    // other sessions) that carry their own eslint.config.* with an unrelated
    // dependency tree — always exclude them regardless of what worktrees exist
    // on disk.
    // `packages/*` (remote-core) and `apps/remote-dev` are workspace sub-projects
    // with their own eslint.config.js and tsconfig; linting them from here both
    // double-lints them and confuses the parser's tsconfig-root detection.
    ignores: ['dist', 'coverage', 'node_modules', 'apps', 'packages', '**/.claude/worktrees/**'],
  },
  {
    files: ['**/*.{ts,tsx,js,mjs,cjs}'],
    languageOptions: { parserOptions: { tsconfigRootDir } },
  },
  {
    files: ['**/*.{ts,tsx}'],
    rules: {
      // `const { previewUrl: _previewUrl, ...rest } = x` is how a field is stripped
      // before persisting; the bound name is intentionally unused. Honour the
      // `_` prefix convention for the same reason.
      '@typescript-eslint/no-unused-vars': [
        'error',
        { args: 'after-used', argsIgnorePattern: '^_', ignoreRestSiblings: true, varsIgnorePattern: '^_' },
      ],
    },
  },
  ...tseslint.configs.recommended,
  {
    files: ['**/*.{ts,tsx}'],
    ...solid.configs['flat/typescript'],
  },
  {
    // Phase 0 fabrication guard: never re-introduce demo/mock data modules that
    // masquerade as live data (e.g. the removed inbox-demo-data). Use real data
    // or an honest empty/error state. Scoped to product source, not tests.
    files: ['src/**/*.{ts,tsx}'],
    ignores: ['**/*.test.{ts,tsx}', '**/*.spec.{ts,tsx}'],
    rules: {
      'no-restricted-imports': [
        'error',
        {
          patterns: [
            {
              group: ['**/*demo-data*', '**/*demo_data*', '**/*mock*'],
              message:
                'Fabricated demo/mock data modules are banned (Phase 0 de-fake). Use live data or an honest empty/error state; keep mocks in *.test/*.spec files.',
            },
          ],
        },
      ],
      // Phase 4 A8 — no-fabricated-state guard. Beyond banning mock modules,
      // flag the highest-signal in-file fabrication: a hardcoded security /
      // compliance posture (the A1 breach). This is precise and false-positive
      // free; an inline-array guard was evaluated and dropped because it cannot
      // distinguish legitimate static UI config (tabs, label rows) from
      // fabricated data, and the real A5/A6 blueprint/template arrays are
      // module consts (relabeled honestly in PR-3, not lint-caught).
      // The selectors live in eslint.ui-gates.config.js, which the UI gate below
      // also spreads, so both blocks enforce the same base set.
      'no-restricted-syntax': ['error', ...BASE_RESTRICTED_SYNTAX],
      // Governed speech only. The browser's recognisers and online voices send
      // audio and reply text to the browser vendor, outside Verevon's provider
      // and audit controls, so they are not a fallback either. Use the
      // dictate route and the governed player (features/voice) instead.
      'no-restricted-globals': ['error', ...WEB_SPEECH_GLOBALS],
      'no-restricted-properties': ['error', ...WEB_SPEECH_PROPERTIES],
    },
  },
  {
    // UI copy and raw-colour gate for chat and agents (eslint.ui-gates.config.js;
    // findings recorded in eslint-suppressions.json). The block replaces the
    // rule's options for these files, so it spreads the base selectors first.
    files: UI_GATE_SCOPES,
    ignores: TEST_FILE_IGNORES,
    rules: {
      'no-restricted-syntax': ['error', ...BASE_RESTRICTED_SYNTAX, ...UI_COPY_SYNTAX, ...UI_COLOR_SYNTAX],
    },
  },
)
