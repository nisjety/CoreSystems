// @ts-check
import eslint from '@eslint/js';
import eslintPluginPrettierRecommended from 'eslint-plugin-prettier/recommended';
import globals from 'globals';
import tseslint from 'typescript-eslint';

export default tseslint.config(
  {
    // Generated protobuf/grpc stubs (ts-proto) — hand-written grpc code lives
    // directly in src/grpc/ and must stay linted.
    ignores: [
      'eslint.config.mjs',
      'src/grpc/auth/v1/**',
      'src/grpc/google/**',
      'src/grpc/proto/**',
    ],
  },
  eslint.configs.recommended,
  ...tseslint.configs.recommendedTypeChecked,
  eslintPluginPrettierRecommended,
  {
    languageOptions: {
      globals: {
        ...globals.node,
      },
      sourceType: 'commonjs',
      parserOptions: {
        projectService: true,
        tsconfigRootDir: import.meta.dirname,
      },
    },
  },
  {
    // Jest globals only where Jest actually runs — otherwise they shadow
    // node:test's `test` import in plain .mjs test scripts.
    files: ['src/**/*.spec.ts', 'test/**/*.ts'],
    languageOptions: {
      globals: {
        ...globals.jest,
      },
    },
  },
  {
    // Plain JS scripts/configs are not part of any tsconfig — type-checked
    // rules cannot run on them and would surface as parsing errors. CommonJS
    // require() is idiomatic in these scripts, not an import-style error.
    files: ['**/*.js', '**/*.mjs', '**/*.cjs'],
    extends: [tseslint.configs.disableTypeChecked],
    rules: {
      '@typescript-eslint/no-require-imports': 'off',
    },
  },
);
