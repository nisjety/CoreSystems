import solid from 'eslint-plugin-solid'
import tseslint from 'typescript-eslint'

export default tseslint.config(
  {
    // `apps/*` are separate sub-projects (the Rust gateway, and any sub-app)
    // with their own tooling — Velion v3's lint only governs its own SPA `src`.
    ignores: ['dist', 'coverage', 'node_modules', 'apps'],
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
      'no-restricted-syntax': [
        'error',
        {
          // A1: a security/compliance-named declaration must NEVER render a
          // control as enabled from a boolean literal — read real org state or
          // show a disabled/unknown state via the read-data substrate.
          selector:
            "VariableDeclarator[id.name=/securit|complian|posture/i] Property > Literal[value=true]",
          message:
            'Fabricated security/compliance posture: a security control may not be hardcoded enabled from a boolean literal (Phase 4 A8). Read real org-security state or render a disabled/unknown state.',
        },
      ],
    },
  },
)
