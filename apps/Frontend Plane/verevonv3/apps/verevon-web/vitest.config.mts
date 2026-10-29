import { fileURLToPath, URL } from 'node:url';
import { defineConfig } from 'vitest/config';

// React component tests of verevon-web. The repository root config compiles every
// .tsx file with the Solid JSX transform, so a React component cannot render there.
// The package test script excludes web .test.tsx files from the root pass; this
// config runs them with React JSX support.
// Run: pnpm --dir ../.. exec vitest run --config apps/verevon-web/vitest.config.mts
export default defineConfig({
	root: fileURLToPath(new URL('.', import.meta.url)),
	resolve: { alias: { '@': fileURLToPath(new URL('./src', import.meta.url)) } },
	test: { environment: 'jsdom', include: ['src/**/*.test.tsx'] },
});
