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
    },
  },
)
