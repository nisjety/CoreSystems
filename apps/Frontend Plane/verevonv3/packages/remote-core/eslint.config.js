import tseslint from 'typescript-eslint'

export default tseslint.config(
  {
    ignores: ['dist', 'coverage', 'node_modules'],
  },
  ...tseslint.configs.recommended,
  {
    files: ['**/*.ts'],
    rules: {
      // remote-core must stay usable from any UI layer — never let a
      // framework leak into the dependency graph.
      'no-restricted-imports': [
        'error',
        {
          paths: [
            { name: 'react', message: '@verevon/remote-core must stay framework-agnostic — no React.' },
            { name: 'solid-js', message: '@verevon/remote-core must stay framework-agnostic — no Solid.' },
          ],
        },
      ],
      'no-console': 'error',
    },
  },
)
